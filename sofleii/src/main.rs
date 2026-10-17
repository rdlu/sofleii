//! sofleii CLI entry point: argument parsing, connection, exit codes.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use sofleii::client::Client;
use sofleii::error::exit;
use sofleii::output::{self, Style};
use sofleii::transport::{Transport, ble::BleTransport, serial::SerialTransport};
use sofleii::{Result, commands};

/// Sofle Interactive Interface: talk to the Eyelash Sofle over ZMK Studio RPC.
#[derive(Parser, Debug)]
#[command(name = "sofleii", version, about, long_about = None)]
struct Cli {
    /// Serial device to use instead of auto-detecting "Eyelash Sofle".
    #[arg(long, global = true, value_name = "PATH")]
    device: Option<String>,

    /// Transport to use.
    #[arg(long, global = true, value_enum, default_value_t = TransportArg::Usb)]
    transport: TransportArg,

    /// How long to wait for each response, in milliseconds.
    #[arg(long, global = true, value_name = "MS", default_value_t = 3000,
          value_parser = clap::value_parser!(u64).range(1..=60_000))]
    timeout: u64,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum TransportArg {
    Usb,
    /// Not implemented yet.
    Ble,
}

#[derive(Subcommand, Debug)]
enum Command {
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

    let Some(command) = cli.command else {
        // No subcommand: usage for now (the TUI will open here on a TTY).
        let _ = Cli::command().print_help();
        return ExitCode::from(exit::OK);
    };

    let json = match command {
        Command::Info { json } | Command::Profiles { json } => json,
    };
    let style = Style::detect(json);

    match run(
        &cli.device,
        cli.transport,
        Duration::from_millis(cli.timeout),
        &command,
        style,
    ) {
        Ok(text) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(text.as_bytes());
            if json {
                let _ = out.write_all(b"\n");
            }
            ExitCode::from(exit::OK)
        }
        Err(err) => {
            eprintln!("sofleii: {err}");
            if json {
                println!("{}", output::error_json(&err));
            }
            ExitCode::from(err.exit_code())
        }
    }
}

fn connect(device: &Option<String>, transport: TransportArg) -> Result<Box<dyn Transport>> {
    match transport {
        TransportArg::Usb => Ok(Box::new(SerialTransport::open(device.as_deref())?)),
        TransportArg::Ble => Ok(Box::new(BleTransport::connect(device.as_deref())?)),
    }
}

fn run(
    device: &Option<String>,
    transport: TransportArg,
    timeout: Duration,
    command: &Command,
    style: Style,
) -> Result<String> {
    let mut client = Client::new(connect(device, transport)?, timeout);
    let info = client.transport_info();
    let text = match command {
        Command::Info { .. } => output::info(style, &info, &commands::info::run(&mut client)?),
        Command::Profiles { .. } => {
            output::profiles(style, &info, &commands::profiles::run(&mut client)?)
        }
    };
    let n = client.notifications().len();
    if n > 0 {
        eprintln!("sofleii: {n} notification(s) received and set aside");
    }
    Ok(text)
}
