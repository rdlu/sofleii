//! sofleii entry point: argument parsing, connection, exit codes; with no
//! subcommand on a terminal, the TUI.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use sofleii::commands::battery::{DEFAULT_CRITICAL_BELOW, DEFAULT_WARN_BELOW, Thresholds};
use sofleii::commands::update::DEFAULT_REPO;
use sofleii::connect::{self, Connected, Connector, STATUS_BATTERY_TIMEOUT, read_battery};
use sofleii::error::exit;
use sofleii::flash::Half;
use sofleii::github::GitHub;
use sofleii::output::{self, Format, Style};
use sofleii::session::jobs::{Checker, UPDATE_TTL, default_cache_dir};
use sofleii::transport::{Preference, TransportKind};
use sofleii::{Error, Result, commands};

/// Each GitHub API request (`update check`) gets this long, all in.
const GITHUB_TIMEOUT: Duration = Duration::from_secs(10);

/// Sofle Interactive Interface: talk to the Eyelash Sofle over ZMK Studio RPC.
#[derive(Parser, Debug)]
#[command(name = "sofleii", version, long_version = sofleii::version::LONG, about, long_about = None)]
struct Cli {
    /// Serial device to use instead of auto-detecting "Eyelash Sofle".
    /// Implies `--transport usb`.
    #[arg(long, global = true, value_name = "PATH")]
    device: Option<String>,

    /// BLE address to use instead of matching the BlueZ device named
    /// "Eyelash Sofle" (batteries, and Studio RPC; implies `--transport ble`).
    #[arg(long, global = true, value_name = "MAC", value_parser = parse_mac)]
    bt_address: Option<String>,

    /// Transport for Studio RPC. `auto`: USB if the keyboard's serial port
    /// exists, else BLE if it's connected to BlueZ.
    #[arg(long, global = true, value_enum, default_value_t = TransportArg::Auto)]
    transport: TransportArg,

    /// How long to wait for each response, in milliseconds.
    #[arg(long, global = true, value_name = "MS", default_value_t = 3000,
          value_parser = clap::value_parser!(u64).range(1..=60_000))]
    timeout: u64,

    /// Output format; `--json` is the same as `--format json`.
    /// `waybar` is only supported by `status` and `battery`.
    #[arg(long, global = true, value_enum, default_value_t = FormatArg::Human)]
    format: FormatArg,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum TransportArg {
    Auto,
    /// USB serial (left half).
    Usb,
    /// BLE GATT through BlueZ (already bonded and connected).
    Ble,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum FormatArg {
    Human,
    Json,
    /// waybar custom-module JSON (one line); `status` and `battery` only.
    Waybar,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// One-shot summary: transport, device, active BLE profile, output
    /// priority, Studio lock, split link and batteries.
    Status {
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Both halves' battery levels, read over BLE GATT through BlueZ.
    Battery {
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
        /// Exit 1 if either half is below this percentage (also the waybar
        /// `warning` threshold, default 20; waybar output never exits 1).
        #[arg(long, value_name = "PCT",
              value_parser = clap::value_parser!(u8).range(0..=100))]
        warn_below: Option<u8>,
        /// waybar `critical` class below this percentage.
        #[arg(long, value_name = "PCT", default_value_t = DEFAULT_CRITICAL_BELOW,
              value_parser = clap::value_parser!(u8).range(0..=100))]
        critical_below: u8,
    },
    /// Device name, firmware build (versions, config commit, board, build time), Studio lock state and custom RPC subsystems.
    Info {
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// BLE profiles (active, connected, bonded, address, name).
    Profiles {
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// The live keymap: show it, or diff it against
    /// config/eyelash_sofle.keymap. Read-only.
    Keymap {
        #[command(subcommand)]
        command: KeymapCommand,
    },
    /// Firmware and sofleii updates against the GitHub releases.
    Update {
        #[command(subcommand)]
        command: UpdateCommand,
    },
    /// The UF2 bootloader drive of a half in bootloader mode (double-tap
    /// reset). Read-only.
    Flash {
        #[command(subcommand)]
        command: FlashCommand,
    },
    /// Switch, rename or unpair a BLE profile. <N> is 0-based, as
    /// `sofleii profiles` lists it (and as `&bt BT_SEL n` counts).
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Set the output priority: where keystrokes (and Studio RPC) go when
    /// both USB and BLE are connected. Switching away from the transport in
    /// use moves Studio to the other one; sofleii reads it back there.
    Output {
        /// `usb` or `ble`.
        #[arg(value_enum)]
        target: OutputArg,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ProfileCommand {
    /// Make profile <N> the active one.
    Switch {
        /// Profile index, 0-based (as `sofleii profiles` lists it).
        #[arg(value_name = "N")]
        profile: u32,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Name bonded profile <N> (at most 31 UTF-8 bytes; "" clears it).
    Rename {
        /// Profile index, 0-based (as `sofleii profiles` lists it).
        #[arg(value_name = "N")]
        profile: u32,
        name: String,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Remove profile <N>'s bond (the host has to pair again). Asks first
    /// unless --yes; with neither a terminal nor --yes it refuses (exit 3).
    Unpair {
        /// Profile index, 0-based (as `sofleii profiles` lists it).
        #[arg(value_name = "N")]
        profile: u32,
        /// Don't ask for confirmation.
        #[arg(long, short)]
        yes: bool,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum OutputArg {
    Usb,
    Ble,
}

#[derive(Subcommand, Debug)]
enum KeymapCommand {
    /// Every layer (or one) as a grid on the physical layout.
    Show {
        /// Only this layer: its index or name (any case).
        #[arg(long, value_name = "N|NAME")]
        layer: Option<String>,
        /// The .keymap whose behaviour labels to use (default:
        /// config/eyelash_sofle.keymap in this directory or above, if any).
        #[arg(long, value_name = "PATH")]
        keymap: Option<std::path::PathBuf>,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Keys and layers that differ between the keyboard and the .keymap
    /// (saved Studio edits survive a flash). Exit 0 no drift, 1 drift.
    Diff {
        /// The .keymap to compare with (default:
        /// config/eyelash_sofle.keymap in this directory or above).
        #[arg(long, value_name = "PATH")]
        keymap: Option<std::path::PathBuf>,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum FlashCommand {
    /// Which keyboard and half the mounted bootloader drive belongs to,
    /// read from the firmware on the chip (its CURRENT.UF2). Exit 0 when it
    /// is this keyboard (and the --expect half), 1 when it isn't or can't
    /// be told: don't flash on 1.
    Identify {
        /// The drive's mount point (default: the one mounted drive with an
        /// INFO_UF2.TXT).
        #[arg(long, value_name = "DIR")]
        mount: Option<std::path::PathBuf>,
        /// The half you mean to flash; anything else exits 1.
        #[arg(long, value_enum)]
        expect: Option<HalfArg>,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum HalfArg {
    Left,
    Right,
}

#[derive(Subcommand, Debug)]
enum UpdateCommand {
    /// Compare the running firmware with the newest firmware release (a
    /// `sofleii-fw-v*` tag), and this sofleii with the newest sofleii
    /// release (a `sofleii-tui-v*` tag). Answers from the shared 6 h cache when it can.
    /// Exit 0 when nothing needs doing, 1 when either checked part is
    /// behind (or the firmware build is unknown; no sofleii release yet is
    /// not a failure).
    Check {
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
        /// GitHub repository whose releases to compare with.
        #[arg(long, value_name = "OWNER/NAME", default_value = DEFAULT_REPO,
              value_parser = parse_repo)]
        repo: String,
        /// Only check the firmware.
        #[arg(long, conflicts_with = "cli")]
        firmware: bool,
        /// Only check sofleii itself (no keyboard needed).
        #[arg(long)]
        cli: bool,
        /// Ask GitHub even if the cache is fresh (the answer is still cached).
        #[arg(long)]
        refresh: bool,
    },
}

fn parse_repo(s: &str) -> std::result::Result<String, String> {
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    match s.split_once('/') {
        Some((owner, name)) if ok(owner) && ok(name) => Ok(s.to_string()),
        _ => Err("expected OWNER/NAME, like rdlu/sofleii".into()),
    }
}

fn parse_mac(s: &str) -> std::result::Result<String, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
    {
        Ok(s.to_ascii_uppercase())
    } else {
        Err("expected a BLE address like DD:1F:23:76:6E:71".into())
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // --help / --version are not errors; everything else is exit 3.
            let code = if e.use_stderr() {
                exit::USAGE
            } else {
                exit::OK
            };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };

    let Some(command) = &cli.command else {
        // No subcommand: the TUI on a terminal, usage when piped.
        if cli.format != FormatArg::Human {
            return usage_error("--format needs a subcommand");
        }
        if std::io::stdout().is_terminal() {
            let link = connect_config(&cli);
            if let Err(e) = link.check() {
                eprintln!("sofleii: {e}");
                return ExitCode::from(e.exit_code());
            }
            return match sofleii::tui::run(link) {
                Ok(()) => ExitCode::from(exit::OK),
                Err(e) => {
                    eprintln!("sofleii: terminal: {e}");
                    ExitCode::from(exit::TRANSPORT)
                }
            };
        }
        let _ = Cli::command().print_help();
        return ExitCode::from(exit::OK);
    };
    sofleii::logger::install_stderr();

    let json_flag = match *command {
        Command::Status { json }
        | Command::Battery { json, .. }
        | Command::Info { json }
        | Command::Profiles { json }
        | Command::Update {
            command: UpdateCommand::Check { json, .. },
        }
        | Command::Output { json, .. }
        | Command::Flash {
            command: FlashCommand::Identify { json, .. },
        }
        | Command::Keymap {
            command: KeymapCommand::Show { json, .. } | KeymapCommand::Diff { json, .. },
        }
        | Command::Profile {
            command:
                ProfileCommand::Switch { json, .. }
                | ProfileCommand::Rename { json, .. }
                | ProfileCommand::Unpair { json, .. },
        } => json,
    };
    let is_battery = matches!(command, Command::Battery { .. });
    let format = match (json_flag, cli.format) {
        (false, FormatArg::Human) => Format::Human,
        (true, FormatArg::Human | FormatArg::Json) | (false, FormatArg::Json) => Format::Json,
        (true, FormatArg::Waybar) => {
            return usage_error("--json conflicts with --format waybar");
        }
        (false, FormatArg::Waybar) => {
            if !matches!(command, Command::Status { .. } | Command::Battery { .. }) {
                return usage_error("--format waybar is only supported by status and battery");
            }
            Format::Waybar
        }
    };
    let style = Style::detect(format);

    match run(&cli, command, style) {
        Ok((text, code)) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(text.as_bytes());
            if format != Format::Human {
                let _ = out.write_all(b"\n");
            }
            ExitCode::from(code)
        }
        Err(err) => {
            eprintln!("sofleii: {err}");
            match format {
                Format::Json => println!("{}", output::error_json(&err)),
                // A bar module must always get valid JSON, even on failure.
                Format::Waybar if is_battery => {
                    println!("{}", output::waybar_battery_error(&err).to_line())
                }
                Format::Waybar => println!("{}", output::waybar_error(&err).to_line()),
                Format::Human => {}
            }
            ExitCode::from(err.exit_code())
        }
    }
}

fn usage_error(msg: &str) -> ExitCode {
    let _ = Cli::command()
        .error(clap::error::ErrorKind::ArgumentConflict, msg)
        .print();
    ExitCode::from(exit::USAGE)
}

/// The connection settings `--transport`/`--device`/`--bt-address`/
/// `--timeout` ask for.
fn connect_config(cli: &Cli) -> connect::Config {
    connect::Config {
        transport: match cli.transport {
            TransportArg::Auto => Preference::Auto,
            TransportArg::Usb => Preference::Usb,
            TransportArg::Ble => Preference::Ble,
        },
        device: cli.device.clone(),
        bt_address: cli.bt_address.clone(),
        timeout: Duration::from_millis(cli.timeout),
    }
}

/// Asks before `profile unpair` (the caller already checked for a TTY).
/// Anything but `y`/`yes` cancels, as a usage error: nothing was sent.
fn confirm_unpair(
    t: &commands::profile::Target,
    input: &mut impl BufRead,
    prompt: &mut impl Write,
) -> Result<()> {
    let mut what = format!("profile {}", t.profile);
    if let Some(s) = t.slot() {
        if let Some(a) = &s.address {
            what.push_str(&format!(" {a}"));
        }
        if let Some(n) = &s.name {
            what.push_str(&format!(" \"{n}\""));
        }
        if s.active {
            what.push_str(", the active profile");
        }
        if s.connected {
            what.push_str(", connected");
        }
    }
    let mut warning = String::from("The host has to pair again to use it.");
    if t.kind == TransportKind::Ble && t.before.active_profile == Some(t.profile) {
        warning.push_str(" sofleii is talking over this connection, so it will drop.");
    }
    let _ = write!(prompt, "Unpair {what}? {warning} [y/N] ");
    let _ = prompt.flush();
    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .map_err(|e| Error::io("reading the confirmation", e))?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(Error::Usage("unpair cancelled; nothing was sent".into())),
    }
}

/// `update check`: the keyboard first when the firmware is checked (so "not
/// found" exits 2 without touching the network), then the shared cache or
/// GitHub (one release list for both parts).
fn update_check(
    cli: &Cli,
    style: Style,
    repo: &str,
    firmware: bool,
    cli_part: bool,
    refresh: bool,
) -> Result<(String, u8)> {
    let (transport, running) = if firmware {
        let mut connector = Connector::new(connect_config(cli));
        let Connected { mut client, info } = connector.connect()?;
        let running = commands::update::read_running(&mut client)?;
        (Some(info), Some(running))
    } else {
        (None, None)
    };
    let checker = Checker::new(repo.to_string(), UPDATE_TTL, default_cache_dir());
    let mut github = GitHub::new(GITHUB_TIMEOUT);
    let now = SystemTime::now();
    let c = checker.check(&mut github, running.as_ref(), cli_part, refresh, now)?;
    let text = output::update_check(style, transport.as_ref(), &c, now);
    Ok((text, c.exit_code()))
}

/// Runs a command; returns the text for stdout and the exit code.
fn run(cli: &Cli, command: &Command, style: Style) -> Result<(String, u8)> {
    let config = connect_config(cli);
    let timeout = config.timeout;
    if let Command::Battery {
        warn_below,
        critical_below,
        ..
    } = *command
    {
        let r = read_battery(cli.bt_address.as_deref(), timeout)?;
        if let Some(n) = &r.note {
            eprintln!("sofleii: note: {n}");
        }
        let t = Thresholds {
            warn_below: warn_below.unwrap_or(DEFAULT_WARN_BELOW),
            critical_below,
        };
        // Exit 1 only for an explicit --warn-below, and never for waybar
        // (the class carries it; the bar wants exit 0 for a reading).
        let mut code = exit::OK;
        if let (Some(w), Some(min)) = (warn_below, r.min_percent())
            && min < w
            && style.format != Format::Waybar
        {
            eprintln!("sofleii: battery at {min}%, below --warn-below {w}%");
            code = exit::CHECK_FAILED;
        }
        return Ok((output::battery(style, &r, t), code));
    }

    if let Command::Update {
        command:
            UpdateCommand::Check {
                repo,
                firmware,
                cli: cli_only,
                refresh,
                ..
            },
    } = command
    {
        // Neither flag: both parts.
        let (want_fw, want_cli) = match (*firmware, *cli_only) {
            (false, false) => (true, true),
            flags => flags,
        };
        return update_check(cli, style, repo, want_fw, want_cli, *refresh);
    }

    if let Command::Flash {
        command: FlashCommand::Identify { mount, expect, .. },
    } = command
    {
        let expect = expect.map(|h| match h {
            HalfArg::Left => Half::Left,
            HalfArg::Right => Half::Right,
        });
        let r = commands::flash::identify(mount.as_deref(), expect)?;
        return Ok((output::flash_identify(style, &r), r.exit_code()));
    }

    // Usage errors that need no keyboard come before connecting; so does
    // reading the .keymap.
    let keymap_file = match command {
        Command::Keymap {
            command: KeymapCommand::Show { keymap, .. },
        } => commands::keymap::load(keymap.as_deref(), false)?,
        Command::Keymap {
            command: KeymapCommand::Diff { keymap, .. },
        } => commands::keymap::load(keymap.as_deref(), true)?,
        _ => None,
    };
    match command {
        Command::Profile {
            command: ProfileCommand::Rename { name, .. },
        } => commands::profile::validate_name(name)?,
        Command::Profile {
            command: ProfileCommand::Unpair { yes: false, .. },
        } if !std::io::stdin().is_terminal() => {
            return Err(Error::Usage(
                "unpair asks for confirmation, and stdin is not a terminal; pass --yes".into(),
            ));
        }
        _ => {}
    }

    let mut connector = Connector::new(config);
    let Connected { mut client, info } = connector.connect()?;
    let mut code = exit::OK;
    let text = match command {
        Command::Status { .. } => {
            let mut s = commands::status::run(&mut client)?;
            // The battery never fails `status`: unavailable is a note. A
            // right half known to be off the split link shows disconnected.
            s.set_battery(read_battery(
                cli.bt_address.as_deref(),
                timeout.min(STATUS_BATTERY_TIMEOUT),
            ));
            output::status(style, &info, &s)
        }
        Command::Info { .. } => output::info(style, &info, &commands::info::run(&mut client)?),
        Command::Profiles { .. } => {
            output::profiles(style, &info, &commands::profiles::run(&mut client)?)
        }
        Command::Keymap {
            command: KeymapCommand::Show { layer, .. },
        } => {
            let s = commands::keymap::show(&mut client, keymap_file.as_ref(), layer.as_deref())?;
            if style.format == Format::Human {
                for n in &s.notes {
                    eprintln!("sofleii: note: {n}");
                }
            }
            output::keymap_show(style, &info, &s)
        }
        Command::Keymap {
            command: KeymapCommand::Diff { .. },
        } => {
            let loaded = keymap_file.as_ref().expect("diff requires the file");
            let d = commands::keymap::diff(&mut client, loaded)?;
            if style.format == Format::Human {
                for n in &d.notes {
                    eprintln!("sofleii: note: {n}");
                }
            }
            code = d.exit_code();
            output::keymap_diff(style, &info, &d)
        }
        Command::Output { target, .. } => {
            let target = match target {
                OutputArg::Usb => TransportKind::Usb,
                OutputArg::Ble => TransportKind::Ble,
            };
            let o = commands::output::set(&mut client, target, || connector.open_other(info.kind))?;
            output::output_action(style, &info, &o)
        }
        Command::Profile { command } => {
            let profile = match *command {
                ProfileCommand::Switch { profile, .. }
                | ProfileCommand::Rename { profile, .. }
                | ProfileCommand::Unpair { profile, .. } => profile,
            };
            let t = commands::profile::prepare(&mut client, profile)?;
            let other = || connector.open_other(info.kind);
            let o = match command {
                ProfileCommand::Switch { .. } => commands::profile::switch(&mut client, &t, other)?,
                ProfileCommand::Rename { name, .. } => {
                    commands::profile::rename(&mut client, &t, name)?
                }
                ProfileCommand::Unpair { yes, .. } => {
                    if t.slot().is_some_and(|s| s.bonded) && !yes {
                        confirm_unpair(&t, &mut std::io::stdin().lock(), &mut std::io::stderr())?;
                    }
                    commands::profile::unpair(&mut client, &t, other)?
                }
            };
            output::profile_action(style, &info, &o)
        }
        Command::Battery { .. } | Command::Update { .. } | Command::Flash { .. } => {
            unreachable!("handled above")
        }
    };
    let n = client.notifications().len();
    if n > 0 {
        eprintln!("sofleii: {n} notification(s) received and set aside");
    }
    Ok((text, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofleii::commands::profile::Target;
    use sofleii::commands::profiles::{Profile, Profiles};

    fn target(kind: TransportKind, profile: u32) -> Target {
        let slot = |index: u32| Profile {
            index,
            active: index == 0,
            connected: index == 0,
            bonded: true,
            address: Some("84:9E:56:52:6A:D2".into()),
            name: (index == 1).then(|| "Mac".into()),
        };
        Target {
            subsystem: 0,
            before: Profiles {
                max_profiles: 2,
                active_profile: Some(0),
                output_priority: Some("usb"),
                profiles: vec![slot(0), slot(1)],
            },
            profile,
            kind,
        }
    }

    fn ask(t: &Target, answer: &str) -> (Result<()>, String) {
        let mut prompt = Vec::new();
        let r = confirm_unpair(t, &mut answer.as_bytes(), &mut prompt);
        (r, String::from_utf8(prompt).unwrap())
    }

    #[test]
    fn unpair_confirms_only_on_yes() {
        let t = target(TransportKind::Usb, 1);
        for yes in ["y\n", "YES\n", " yes "] {
            assert!(ask(&t, yes).0.is_ok(), "{yes:?}");
        }
        for no in ["\n", "n\n", "", "yep\n"] {
            let err = ask(&t, no).0.unwrap_err();
            assert_eq!(err.exit_code(), exit::USAGE, "{no:?}");
        }
        let (_, prompt) = ask(&t, "n\n");
        assert!(
            prompt.contains("profile 1 84:9E:56:52:6A:D2 \"Mac\""),
            "{prompt}"
        );
        assert!(prompt.ends_with("[y/N] "), "{prompt}");
    }

    #[test]
    fn unpair_prompt_warns_about_the_active_ble_link() {
        let (_, prompt) = ask(&target(TransportKind::Ble, 0), "n\n");
        assert!(prompt.contains("the active profile, connected"), "{prompt}");
        assert!(prompt.contains("it will drop"), "{prompt}");
        let (_, prompt) = ask(&target(TransportKind::Usb, 0), "n\n");
        assert!(!prompt.contains("it will drop"), "{prompt}");
    }
}
