//! Rendering of command results: human-readable text or the versioned JSON
//! contract from #44 (`"schema": 1`, snake_case keys, explicit `null`s).

use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::Error;
use crate::commands::{info::Info, profiles::Profiles};
use crate::transport::TransportInfo;

/// Bumped on any breaking change to the JSON output.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

/// Output style decided once from the flags and the environment.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub format: Format,
    pub color: bool,
}

impl Style {
    /// Colour only for human output on a TTY, and never with `NO_COLOR` set
    /// (to anything non-empty, per no-color.org).
    pub fn detect(json: bool) -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Self {
            format: if json { Format::Json } else { Format::Human },
            color: !json && !no_color && std::io::stdout().is_terminal(),
        }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
}

#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
    schema: u32,
    transport: &'a TransportInfo,
    #[serde(flatten)]
    data: &'a T,
}

fn json<T: Serialize>(transport: &TransportInfo, data: &T) -> String {
    let env = Envelope {
        schema: SCHEMA,
        transport,
        data,
    };
    serde_json::to_string_pretty(&env).expect("serialising plain data cannot fail")
}

/// JSON for a failed command, printed to stdout under `--json` so scripts
/// always get a parseable object.
pub fn error_json(err: &Error) -> String {
    #[derive(Serialize)]
    struct ErrorBody {
        kind: &'static str,
        message: String,
        exit_code: u8,
    }
    #[derive(Serialize)]
    struct ErrorEnvelope {
        schema: u32,
        error: ErrorBody,
    }
    let env = ErrorEnvelope {
        schema: SCHEMA,
        error: ErrorBody {
            kind: err.kind(),
            message: err.to_string(),
            exit_code: err.exit_code(),
        },
    };
    serde_json::to_string_pretty(&env).expect("serialising plain data cannot fail")
}

fn or_dash(v: Option<&str>) -> &str {
    v.unwrap_or("-")
}

pub fn info(style: Style, transport: &TransportInfo, i: &Info) -> String {
    if style.format == Format::Json {
        return json(transport, i);
    }
    let mut out = Vec::new();
    let w = &mut out;
    let label = |s: &str| style.dim(&format!("{s:<16}"));
    let _ = writeln!(w, "{}", style.bold(&i.device.name));
    let _ = writeln!(
        w,
        "{}{} {}",
        label("transport"),
        transport.kind,
        transport.device
    );
    let _ = writeln!(
        w,
        "{}{}",
        label("serial number"),
        or_dash(i.device.serial_number.as_deref())
    );
    let _ = writeln!(
        w,
        "{}{}",
        label("studio"),
        if i.studio.locked {
            "locked"
        } else {
            "unlocked"
        }
    );
    let _ = writeln!(
        w,
        "{}{}",
        label("build"),
        if i.build.available {
            "available"
        } else {
            "unavailable (firmware lacks build info, #46)"
        }
    );
    let _ = writeln!(w, "{}", label("subsystems"));
    if i.custom_subsystems.is_empty() {
        let _ = writeln!(w, "  (none)");
    }
    for s in &i.custom_subsystems {
        let _ = writeln!(w, "  {:>2}  {}", s.index, s.identifier);
    }
    String::from_utf8(out).expect("utf-8")
}

pub fn profiles(style: Style, transport: &TransportInfo, p: &Profiles) -> String {
    if style.format == Format::Json {
        return json(transport, p);
    }
    let mut out = Vec::new();
    let w = &mut out;
    let yes = |b: bool| if b { "yes" } else { "-" };
    let _ = writeln!(
        w,
        "{}",
        style.bold(&format!(
            "{:<3} {:<6} {:<9} {:<6} {:<17} {}",
            "#", "active", "connected", "bonded", "address", "name"
        ))
    );
    for pr in &p.profiles {
        let line = format!(
            "{:<3} {:<6} {:<9} {:<6} {:<17} {}",
            pr.index,
            if pr.active { "*" } else { "" },
            yes(pr.connected),
            yes(pr.bonded),
            or_dash(pr.address.as_deref()),
            or_dash(pr.name.as_deref()),
        );
        let _ = writeln!(w, "{}", if pr.active { style.green(&line) } else { line });
    }
    let _ = writeln!(
        w,
        "{}",
        style.dim(&format!(
            "{} of {} slots bonded; output priority: {}",
            p.profiles.iter().filter(|x| x.bonded).count(),
            p.max_profiles,
            or_dash(p.output_priority)
        ))
    );
    String::from_utf8(out).expect("utf-8")
}
