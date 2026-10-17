//! sofleii CLI entry point: argument parsing, connection, exit codes.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use sofleii::bluez::Bluez;
use sofleii::client::Client;
use sofleii::commands::battery::{self, DEFAULT_CRITICAL_BELOW, DEFAULT_WARN_BELOW, Thresholds};
use sofleii::commands::status::Battery;
use sofleii::error::exit;
use sofleii::output::{self, Format, Style};
use sofleii::transport::{Transport, ble::BleTransport, serial::SerialTransport};
use sofleii::{Result, commands};

/// `status` reads the batteries with at most this timeout, to stay snappy.
const STATUS_BATTERY_TIMEOUT: Duration = Duration::from_millis(1000);

/// Sofle Interactive Interface: talk to the Eyelash Sofle over ZMK Studio RPC.
#[derive(Parser, Debug)]
#[command(name = "sofleii", version, about, long_about = None)]
struct Cli {
    /// Serial device to use instead of auto-detecting "Eyelash Sofle".
    #[arg(long, global = true, value_name = "PATH")]
    device: Option<String>,

    /// BLE address to read batteries from instead of matching the BlueZ
    /// device named "Eyelash Sofle" (`battery`, and `status`'s battery).
    #[arg(long, global = true, value_name = "MAC", value_parser = parse_mac)]
    bt_address: Option<String>,

    /// Transport to use.
    #[arg(long, global = true, value_enum, default_value_t = TransportArg::Usb)]
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
    Usb,
    /// Not implemented yet.
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
    /// Device name, serial number, Studio lock state and custom RPC subsystems.
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
        // No subcommand: usage for now (the TUI will open here on a TTY).
        let _ = Cli::command().print_help();
        return ExitCode::from(exit::OK);
    };

    let json_flag = match *command {
        Command::Status { json }
        | Command::Battery { json, .. }
        | Command::Info { json }
        | Command::Profiles { json } => json,
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

fn connect(device: &Option<String>, transport: TransportArg) -> Result<Box<dyn Transport>> {
    match transport {
        TransportArg::Usb => Ok(Box::new(SerialTransport::open(device.as_deref())?)),
        TransportArg::Ble => Ok(Box::new(BleTransport::connect(device.as_deref())?)),
    }
}

/// Reads both batteries over BlueZ, printing warnings to stderr.
fn read_battery(bt_address: &Option<String>, timeout: Duration) -> Result<battery::Reading> {
    let mut bluez = Bluez::connect(timeout)?;
    let r = battery::run(
        &mut bluez,
        &battery::Query {
            address: bt_address.clone(),
        },
    )?;
    for w in &r.warnings {
        eprintln!("sofleii: warning: {w}");
    }
    Ok(r)
}

/// Runs a command; returns the text for stdout and the exit code.
fn run(cli: &Cli, command: &Command, style: Style) -> Result<(String, u8)> {
    let timeout = Duration::from_millis(cli.timeout);
    if let Command::Battery {
        warn_below,
        critical_below,
        ..
    } = *command
    {
        let r = read_battery(&cli.bt_address, timeout)?;
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

    let mut client = Client::new(connect(&cli.device, cli.transport)?, timeout);
    let info = client.transport_info();
    let text = match command {
        Command::Status { .. } => {
            let mut s = commands::status::run(&mut client)?;
            // The battery never fails `status`: unavailable is a note.
            s.battery = Battery::from_reading(read_battery(
                &cli.bt_address,
                timeout.min(STATUS_BATTERY_TIMEOUT),
            ));
            output::status(style, &info, &s)
        }
        Command::Info { .. } => output::info(style, &info, &commands::info::run(&mut client)?),
        Command::Profiles { .. } => {
            output::profiles(style, &info, &commands::profiles::run(&mut client)?)
        }
        Command::Battery { .. } => unreachable!("handled above"),
    };
    let n = client.notifications().len();
    if n > 0 {
        eprintln!("sofleii: {n} notification(s) received and set aside");
    }
    Ok((text, exit::OK))
}
