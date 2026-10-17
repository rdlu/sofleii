//! Error type shared by the core, with the exit codes from #44.

use std::fmt;
use std::time::Duration;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Process exit codes (the stable contract from #44).
pub mod exit {
    pub const OK: u8 = 0;
    /// Check failed / attention needed (no command uses it yet).
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
    /// No response within the deadline.
    Timeout { what: String, after: Duration },
    /// A frame or message could not be decoded, or the reply made no sense.
    Protocol(String),
    /// The keyboard answered with an RPC-level error.
    Rpc(String),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::NotFound(_) => exit::NOT_FOUND,
            Error::Usage(_) => exit::USAGE,
            Error::Io { .. } | Error::Timeout { .. } | Error::Protocol(_) | Error::Rpc(_) => {
                exit::TRANSPORT
            }
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
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(m) => write!(f, "keyboard not found: {m}"),
            Error::Usage(m) => write!(f, "usage: {m}"),
            Error::Io { context, source } => write!(f, "{context}: {source}"),
            Error::Timeout { what, after } => {
                write!(
                    f,
                    "timed out after {} ms waiting for {what}",
                    after.as_millis()
                )
            }
            Error::Protocol(m) => write!(f, "protocol error: {m}"),
            Error::Rpc(m) => write!(f, "keyboard returned an error: {m}"),
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
