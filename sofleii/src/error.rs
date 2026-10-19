//! Error type shared by the core, with the exit codes from #44.

use std::fmt;
use std::time::Duration;

use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Process exit codes (the stable contract from #44).
pub mod exit {
    pub const OK: u8 = 0;
    /// Check failed / attention needed (`battery --warn-below`, an update
    /// available or an unknown build in `update check`).
    pub const CHECK_FAILED: u8 = 1;
    pub const NOT_FOUND: u8 = 2;
    pub const USAGE: u8 = 3;
    pub const TRANSPORT: u8 = 4;
}

#[derive(Debug)]
pub enum Error {
    /// No keyboard found, or the given device path does not exist.
    NotFound(String),
    /// Bad arguments that clap could not catch.
    Usage(String),
    /// I/O on the transport failed (open, read, write).
    Io {
        context: String,
        source: std::io::Error,
    },
    /// No response within the deadline. `hint` is the transport's guess at
    /// why (BLE: Studio only answers on the selected output endpoint).
    Timeout {
        what: String,
        after: Duration,
        hint: Option<String>,
    },
    /// A frame or message could not be decoded, or the reply made no sense.
    Protocol(String),
    /// The keyboard answered with an RPC-level error.
    Rpc(String),
    /// A D-Bus or GATT call to BlueZ failed (battery levels).
    Bluez(String),
    /// The firmware lacks what the command needs (`update check` on a build
    /// without the `zmk__device_info` subsystem).
    Unsupported(String),
    /// A GitHub REST API call failed: network, TLS, rate limit, bad status.
    GitHub(String),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::NotFound(_) => exit::NOT_FOUND,
            Error::Usage(_) => exit::USAGE,
            Error::Io { .. }
            | Error::Timeout { .. }
            | Error::Protocol(_)
            | Error::Rpc(_)
            | Error::Bluez(_)
            | Error::Unsupported(_)
            | Error::GitHub(_) => exit::TRANSPORT,
        }
    }

    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    /// A stable, machine-readable kind for `--json` error output.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::Usage(_) => "usage",
            Error::Io { .. } => "io",
            Error::Timeout { .. } => "timeout",
            Error::Protocol(_) => "protocol",
            Error::Rpc(_) => "rpc",
            Error::Bluez(_) => "bluez",
            Error::Unsupported(_) => "unsupported",
            Error::GitHub(_) => "github",
        }
    }

    /// This error as plain data, see [`ErrorView`].
    pub fn view(&self) -> ErrorView {
        ErrorView {
            kind: self.kind(),
            message: self.to_string(),
            exit_code: self.exit_code(),
        }
    }
}

/// An [`Error`] as plain data: `Clone`, so it can cross a channel (the TUI's
/// session thread), and `Serialize`, as the `error` object of `--json`.
/// `Error` holds an `io::Error`, which is neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorView {
    /// [`Error::kind`].
    pub kind: &'static str,
    /// The `Display` text.
    pub message: String,
    /// [`Error::exit_code`].
    pub exit_code: u8,
}

impl From<&Error> for ErrorView {
    fn from(e: &Error) -> Self {
        e.view()
    }
}

impl fmt::Display for ErrorView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(m) => write!(f, "keyboard not found: {m}"),
            Error::Usage(m) => write!(f, "usage: {m}"),
            Error::Io { context, source } => write!(f, "{context}: {source}"),
            Error::Timeout { what, after, hint } => {
                write!(
                    f,
                    "timed out after {} ms waiting for {what}",
                    after.as_millis()
                )?;
                match hint {
                    Some(h) => write!(f, " ({h})"),
                    None => Ok(()),
                }
            }
            Error::Protocol(m) => write!(f, "protocol error: {m}"),
            Error::Rpc(m) => write!(f, "keyboard returned an error: {m}"),
            Error::Bluez(m) => write!(f, "BlueZ error: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported by this firmware: {m}"),
            Error::GitHub(m) => write!(f, "GitHub API: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_carries_kind_message_and_exit_code() {
        let e = Error::io(
            "opening /dev/ttyACM1",
            std::io::Error::new(std::io::ErrorKind::ResourceBusy, "port busy"),
        );
        let v = e.view();
        assert_eq!(
            v,
            ErrorView {
                kind: "io",
                message: "opening /dev/ttyACM1: port busy".into(),
                exit_code: exit::TRANSPORT,
            }
        );
        assert_eq!(ErrorView::from(&e), v.clone());
        assert_eq!(v.to_string(), e.to_string());
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            serde_json::json!({
                "kind": "io",
                "message": "opening /dev/ttyACM1: port busy",
                "exit_code": 4
            })
        );
        let v = Error::NotFound("no serial port".into()).view();
        assert_eq!((v.kind, v.exit_code), ("not_found", exit::NOT_FOUND));
        let v = Error::Usage("bad".into()).view();
        assert_eq!((v.kind, v.exit_code), ("usage", exit::USAGE));
    }

    #[test]
    fn error_view_crosses_threads() {
        let v = Error::Rpc("x".into()).view();
        let got = std::thread::spawn(move || v).join().unwrap();
        assert_eq!(got.kind, "rpc");
    }
}
