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
    /// Flash firmware through the UF2 bootloader (double-tap reset on a
    /// half), checking first that the chip is the half meant; or just
    /// identify the drive.
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
    /// Flash the left half: the newest firmware release unless a source
    /// flag says otherwise. Asks for a double-tap of reset, checks the
    /// chip is the left half, copies, waits for the reboot. Exit 0 flashed,
    /// 1 refused or stopped, 2 no bootloader drive in time.
    Left {
        #[command(flatten)]
        args: FlashArgs,
        /// Flash settings_reset first (clears BLE bonds and settings), then
        /// the firmware: two double-taps.
        #[arg(long)]
        reset: bool,
    },
    /// Flash the right half (see `left`).
    Right {
        #[command(flatten)]
        args: FlashArgs,
        /// Flash settings_reset first, then the firmware.
        #[arg(long)]
        reset: bool,
    },
    /// Flash both halves, left then right (see `left`).
    Both {
        #[command(flatten)]
        args: FlashArgs,
        /// Flash settings_reset first on each half, then its firmware.
        #[arg(long)]
        reset: bool,
    },
    /// Flash only settings_reset (clears BLE bonds and settings) onto a
    /// half or both; the half's firmware goes back on with `flash <half>`.
    Reset {
        #[arg(value_enum)]
        half: HalvesArg,
        #[command(flatten)]
        args: FlashArgs,
    },
    /// The chips sofleii remembers, by USB serial (learned when flashing).
    Chips {
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Forget a chip (a controller that moved to another keyboard); its
    /// next flash identifies it by its firmware again, or adopts it.
    Forget {
        /// The chip's USB serial, as `flash chips` lists it.
        serial: String,
        /// Print the versioned JSON contract instead of text.
        #[arg(long)]
        json: bool,
    },
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

impl From<HalfArg> for Half {
    fn from(h: HalfArg) -> Self {
        match h {
            HalfArg::Left => Half::Left,
            HalfArg::Right => Half::Right,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum HalvesArg {
    Left,
    Right,
    Both,
}

impl HalvesArg {
    fn halves(self) -> Vec<Half> {
        match self {
            HalvesArg::Left => vec![Half::Left],
            HalvesArg::Right => vec![Half::Right],
            HalvesArg::Both => vec![Half::Left, Half::Right],
        }
    }
}

/// Where the firmware comes from, and how the flash runs.
#[derive(clap::Args, Debug)]
struct FlashArgs {
    /// A firmware release by tag (default: the newest sofleii-fw-v*
    /// release).
    #[arg(long, value_name = "TAG", conflicts_with_all = ["ci", "run", "file"])]
    release: Option<String>,
    /// The newest successful CI build on BRANCH (default: main). Needs
    /// GH_TOKEN or GITHUB_TOKEN, e.g. GH_TOKEN=$(gh auth token).
    #[arg(long, value_name = "BRANCH", num_args = 0..=1, default_missing_value = "main",
          conflicts_with_all = ["run", "file"])]
    ci: Option<String>,
    /// One CI build run by id (token as for --ci).
    #[arg(long, value_name = "ID", conflicts_with = "file")]
    run: Option<u64>,
    /// Local .uf2 files (repeat for several); each is identified by its
    /// content, so the order and names don't matter.
    #[arg(long, value_name = "PATH")]
    file: Vec<std::path::PathBuf>,
    /// Flash a chip nothing vouches for (a first flash, foreign or erased
    /// firmware) or whose firmware says it is the other half: `--adopt
    /// left` states the half; plain `--adopt` asks you to type it.
    /// Remembered by serial afterwards.
    #[arg(long, value_enum, value_name = "HALF", num_args = 0..=1)]
    adopt: Option<Option<HalfArg>>,
    /// Seconds to wait for each bootloader drive.
    #[arg(long, value_name = "SECS", default_value_t = 60,
          value_parser = clap::value_parser!(u64).range(1..=600))]
    wait: u64,
    /// GitHub repository for --release/--ci/--run and the default release.
    #[arg(long, value_name = "OWNER/NAME", default_value = DEFAULT_REPO,
          value_parser = parse_repo)]
    repo: String,
    /// Print the versioned JSON result instead of text (the running
    /// commentary still goes to stderr).
    #[arg(long)]
    json: bool,
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
            command:
                FlashCommand::Identify { json, .. }
                | FlashCommand::Chips { json }
                | FlashCommand::Forget { json, .. },
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
        Command::Flash {
            command:
                FlashCommand::Left { ref args, .. }
                | FlashCommand::Right { ref args, .. }
                | FlashCommand::Both { ref args, .. }
                | FlashCommand::Reset { ref args, .. },
        } => args.json,
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

/// `flash …`: identify, the chip memory, or a flash job.
fn flash(cli: &Cli, command: &FlashCommand, style: Style) -> Result<(String, u8)> {
    use commands::flash::{chips, job, source};
    let (steps, args) = match command {
        FlashCommand::Identify { mount, expect, .. } => {
            let r = commands::flash::identify(mount.as_deref(), expect.map(Half::from))?;
            return Ok((output::flash_identify(style, &r), r.exit_code()));
        }
        FlashCommand::Chips { .. } => {
            let path = chips::default_path();
            return Ok((
                output::flash_chips(style, &chips::load(&path), &path),
                exit::OK,
            ));
        }
        FlashCommand::Forget { serial, .. } => {
            let path = chips::default_path();
            let mut c = chips::load(&path);
            if !c.forget(serial) {
                return Err(Error::Usage(format!(
                    "no chip {serial} in {}; `sofleii flash chips` lists them",
                    path.display()
                )));
            }
            chips::save(&path, &c)?;
            return Ok((output::flash_chips(style, &c, &path), exit::OK));
        }
        FlashCommand::Left { args, reset } => (job::plan(&[Half::Left], *reset), args),
        FlashCommand::Right { args, reset } => (job::plan(&[Half::Right], *reset), args),
        FlashCommand::Both { args, reset } => (job::plan(&[Half::Left, Half::Right], *reset), args),
        FlashCommand::Reset { half, args } => (job::plan_reset(&half.halves()), args),
    };

    // Usage errors before the network and the double-tap.
    let ask_half = matches!(args.adopt, Some(None));
    if ask_half && !std::io::stdin().is_terminal() {
        return Err(Error::Usage(
            "--adopt without a half asks you to type it, and stdin is not a terminal; \
             pass --adopt left or --adopt right"
                .into(),
        ));
    }
    let adopt = args.adopt.flatten().map(Half::from);
    if let Some(h) = adopt
        && !steps.iter().any(|s| s.half == h)
    {
        return Err(Error::Usage(format!(
            "--adopt {} names a half this command doesn't flash",
            h.as_str()
        )));
    }
    let src = if !args.file.is_empty() {
        source::Source::Files(args.file.clone())
    } else if let Some(id) = args.run {
        source::Source::Run(id)
    } else if let Some(branch) = &args.ci {
        source::Source::Ci {
            branch: branch.clone(),
        }
    } else if let Some(tag) = &args.release {
        source::Source::Release(tag.clone())
    } else {
        source::Source::LatestRelease
    };

    let err_style = Style::stderr();
    let bundle = source::fetch(&src, &args.repo, &mut GitHub::new(GITHUB_TIMEOUT))?;
    eprint!("{}", output::flash_bundle(err_style, &bundle));

    let path = chips::default_path();
    let mut known = chips::load(&path);
    let mut save = |c: &sofleii::flash::Chips| chips::save(&path, c);
    let now = || {
        jiff::Timestamp::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let mut memory = job::Memory {
        chips: &mut known,
        save: &mut save,
        now: &now,
    };
    let opts = job::Options {
        wait: Duration::from_secs(args.wait),
        adopt,
        ..job::Options::default()
    };
    let mut ui = CliUi {
        style: err_style,
        ask_half,
        wait: opts.wait,
        last_half: None,
        progress: std::io::stderr().is_terminal(),
    };
    let mut drives = commands::flash::drive::Sys::default();
    let mut report = job::run(&steps, &bundle, &mut memory, &mut drives, &mut ui, &opts)?;

    // The left half answers over Studio once it's back: read what it runs.
    if let Some(s) = report.steps.iter_mut().find(|s| {
        s.step.role == sofleii::flash::Role::Left
            && matches!(s.outcome, job::Outcome::Flashed { .. })
    }) {
        eprintln!(
            "{:<10} reading the left half's version over Studio…",
            "confirm"
        );
        match read_back(cli) {
            Ok(v) => s.running = Some(v),
            Err(e) => eprintln!("sofleii: note: couldn't read the left half's version back: {e}"),
        }
    }
    Ok((output::flash_report(style, &report), report.exit_code()))
}

/// The left half's `zmk_config_version`, retried while it boots and its
/// USB or BLE link comes up (up to 20 s).
fn read_back(cli: &Cli) -> Result<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let r = Connector::new(connect_config(cli))
            .connect()
            .and_then(|Connected { mut client, .. }| commands::update::read_running(&mut client));
        match r {
            Ok(running) => {
                return running
                    .version
                    .ok_or_else(|| Error::Unsupported("the firmware reports no version".into()));
            }
            Err(e @ Error::Unsupported(_)) => return Err(e),
            Err(e) if std::time::Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}

/// The flash job's front end on a terminal: commentary on stderr, the
/// double-tap prompts as instructions (no confirmation, so it runs without
/// a TTY), and the typed half for `--adopt` without one.
struct CliUi {
    style: Style,
    ask_half: bool,
    wait: Duration,
    last_half: Option<Half>,
    progress: bool,
}

impl commands::flash::job::Ui for CliUi {
    fn event(&mut self, e: &commands::flash::job::Event) {
        use commands::flash::job::Event;
        match e {
            Event::Step {
                index,
                total,
                step,
                file,
            } => {
                let again = self.last_half == Some(step.half);
                self.last_half = Some(step.half);
                eprintln!(
                    "{}",
                    output::flash_step(self.style, *index, *total, *step, file, again, self.wait)
                );
            }
            Event::Writing { written, total } => {
                if self.progress {
                    eprint!("\r{}", output::flash_progress(*written, *total));
                    if written == total {
                        eprintln!();
                    }
                }
            }
            e => {
                if let Some(line) = output::flash_event(self.style, e) {
                    eprintln!("{line}");
                }
            }
        }
    }

    fn confirm(&mut self, _: &commands::flash::job::Prompt<'_>) -> bool {
        true
    }

    fn adopt(&mut self, p: &commands::flash::job::Prompt<'_>) -> Option<Half> {
        if !self.ask_half {
            return None;
        }
        eprint!("{}", output::flash_adopt(self.style, p));
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer).ok()?;
        Half::parse(&answer.trim().to_ascii_lowercase())
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

    if let Command::Flash { command } = command {
        return flash(cli, command, style);
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
