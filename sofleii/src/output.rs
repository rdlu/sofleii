//! Rendering of command results: human-readable text, the versioned JSON
//! contract from #44 (`"schema": 1`, snake_case keys, explicit `null`s), or
//! waybar's custom-module JSON for `status`.

use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::Error;
use crate::commands::{
    info::Info,
    profiles::Profiles,
    status::{BatteryLevel, Status},
};
use crate::transport::{TransportInfo, TransportKind};

/// Bumped on any breaking change to the JSON output.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
    /// waybar `custom` module JSON (one line); only `status` supports it.
    Waybar,
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
    pub fn detect(format: Format) -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Self {
            format,
            color: format == Format::Human && !no_color && std::io::stdout().is_terminal(),
        }
    }

    /// No colour regardless of the terminal (tooltips, tests).
    pub fn plain(format: Format) -> Self {
        Self {
            format,
            color: false,
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

/// waybar custom-module output (`return-type: json`), always one line.
#[derive(Debug, Serialize)]
pub struct Waybar {
    pub text: String,
    pub tooltip: String,
    pub class: &'static str,
    pub alt: &'static str,
}

impl Waybar {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("serialising plain data cannot fail")
    }
}

/// Nerd Font glyphs in the waybar `text` (Material Design set):
/// nf-md-usb U+F0553, nf-md-bluetooth U+F00AF, nf-md-bluetooth_off U+F00B2.
pub const GLYPH_USB: &str = "\u{f0553}";
pub const GLYPH_BLE: &str = "\u{f00af}";
pub const GLYPH_DISCONNECTED: &str = "\u{f00b2}";

/// Best guess of where keystrokes go: the output priority, with ZMK's
/// fallback to the other endpoint when the preferred one isn't connected.
/// USB counts as connected while we talk to the keyboard over USB.
pub fn effective_output(transport: &TransportInfo, s: &Status) -> Option<&'static str> {
    let usb = transport.kind == TransportKind::Usb;
    let ble = s.profile.as_ref().is_some_and(|p| p.connected);
    match (s.output_priority, usb, ble) {
        (Some("ble"), _, true) => Some("ble"),
        (_, true, _) => Some("usb"),
        (_, false, true) => Some("ble"),
        _ => None,
    }
}

pub fn waybar_status(transport: &TransportInfo, s: &Status) -> Waybar {
    let (class, glyph) = match effective_output(transport, s) {
        Some("usb") => ("usb", GLYPH_USB),
        Some("ble") => ("ble", GLYPH_BLE),
        _ => ("disconnected", GLYPH_DISCONNECTED),
    };
    let text = match s.active_profile {
        Some(n) => format!("{glyph} {n}"),
        None => glyph.to_string(),
    };
    // waybar renders tooltips in a proportional font: "label: value", no padding.
    let mut tooltip = s.device.name.clone();
    for (label, value) in status_rows(transport, s) {
        tooltip.push_str(&format!("\n{label}: {value}"));
    }
    Waybar {
        text,
        tooltip,
        class,
        alt: class,
    }
}

/// waybar output for a failed `status`, so the bar module never breaks.
pub fn waybar_error(err: &Error) -> Waybar {
    let headline = match err {
        Error::NotFound(_) => "Eyelash Sofle not connected",
        _ => "Eyelash Sofle: error",
    };
    Waybar {
        text: GLYPH_DISCONNECTED.to_string(),
        tooltip: format!("{headline}\n{err}"),
        class: "disconnected",
        alt: "disconnected",
    }
}

/// Label/value rows shared by the human output and the waybar tooltip.
fn status_rows(transport: &TransportInfo, s: &Status) -> Vec<(&'static str, String)> {
    let output = match (effective_output(transport, s), s.output_priority) {
        (Some(o), Some(p)) => format!("{o} (priority {p})"),
        (Some(o), None) => o.to_string(),
        (None, Some(p)) => format!("none (priority {p})"),
        (None, None) => "-".to_string(),
    };
    let profile = match &s.profile {
        Some(p) => {
            let state = match (p.connected, p.bonded) {
                (true, _) => "connected",
                (false, true) => "bonded, not connected",
                (false, false) => "empty",
            };
            let mut line = format!("{} {state}", p.index);
            if let Some(a) = &p.address {
                line.push_str(&format!(" {a}"));
            }
            if let Some(n) = &p.name {
                line.push_str(&format!(" \"{n}\""));
            }
            line
        }
        None => "-".to_string(),
    };
    let split = match &s.split {
        Some(sp) if sp.peripheral_connected => "connected",
        Some(_) => "disconnected",
        None => "unknown (not readable yet)",
    };
    let pct = |b: Option<BatteryLevel>| match b.and_then(|b| b.percent) {
        Some(p) => format!("{p}%"),
        None => "-".to_string(),
    };
    let mut battery = format!(
        "left {}, right {}",
        pct(s.battery.left),
        pct(s.battery.right)
    );
    if s.battery.note.is_some() {
        battery.push_str(" (needs #45)");
    }
    let mut rows = vec![
        (
            "transport",
            format!("{} {}", transport.kind, transport.device),
        ),
        ("output", output),
        ("ble profile", profile),
    ];
    if let (Some(b), Some(m)) = (s.bonded_profiles, s.max_profiles) {
        rows.push(("bonded slots", format!("{b} of {m}")));
    }
    let studio = if s.studio.locked {
        "locked"
    } else {
        "unlocked"
    };
    rows.push(("studio", studio.to_string()));
    rows.push(("split link", split.to_string()));
    rows.push(("battery", battery));
    rows
}

pub fn status(style: Style, transport: &TransportInfo, s: &Status) -> String {
    match style.format {
        Format::Json => return json(transport, s),
        Format::Waybar => return waybar_status(transport, s).to_line(),
        Format::Human => {}
    }
    let mut out = format!("{}\n", style.bold(&s.device.name));
    for (label, value) in status_rows(transport, s) {
        out.push_str(&format!("{}{value}\n", style.dim(&format!("{label:<16}"))));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::info::Device;
    use crate::commands::profiles::{Profile, Profiles};
    use crate::commands::status;
    use serde_json::Value;

    fn transport(kind: TransportKind) -> TransportInfo {
        TransportInfo {
            kind,
            device: "/dev/ttyACM1".into(),
        }
    }

    fn sample(priority: &'static str, connected: bool) -> Status {
        status::build(
            Device {
                name: "Eyelash Sofle".into(),
                serial_number: None,
            },
            false,
            Some(Profiles {
                max_profiles: 5,
                active_profile: Some(2),
                output_priority: Some(priority),
                profiles: vec![Profile {
                    index: 2,
                    active: true,
                    connected,
                    bonded: true,
                    address: Some("84:9E:56:52:6A:D2".into()),
                    name: None,
                }],
            }),
        )
    }

    fn parse(line: &str) -> Value {
        assert!(!line.contains('\n'), "waybar output must be one line");
        let v: Value = serde_json::from_str(line).unwrap();
        for k in ["text", "tooltip", "class", "alt"] {
            assert!(v[k].is_string(), "missing {k}");
        }
        v
    }

    #[test]
    fn waybar_ble_connected() {
        let s = sample("ble", true);
        let v = parse(&status(
            Style::plain(Format::Waybar),
            &transport(TransportKind::Usb),
            &s,
        ));
        assert_eq!(v["class"], "ble");
        assert_eq!(v["alt"], "ble");
        assert_eq!(v["text"], format!("{GLYPH_BLE} 2"));
        let tip = v["tooltip"].as_str().unwrap();
        assert!(tip.starts_with("Eyelash Sofle\n"), "{tip}");
        assert!(tip.contains("84:9E:56:52:6A:D2"));
        assert!(!tip.contains('\x1b'));
    }

    #[test]
    fn waybar_ble_priority_falls_back_to_usb() {
        // Prefers BLE but the profile is not connected: ZMK sends over USB.
        let v = parse(&status(
            Style::plain(Format::Waybar),
            &transport(TransportKind::Usb),
            &sample("ble", false),
        ));
        assert_eq!(v["class"], "usb");
        assert_eq!(v["text"], format!("{GLYPH_USB} 2"));
    }

    #[test]
    fn waybar_disconnected_over_ble_transport() {
        // Future BLE transport, profile not connected, no USB: nothing is up.
        let v = parse(&status(
            Style::plain(Format::Waybar),
            &transport(TransportKind::Ble),
            &sample("usb", false),
        ));
        assert_eq!(v["class"], "disconnected");
    }

    #[test]
    fn waybar_not_found() {
        let err = Error::NotFound("/dev/nonexistent does not exist".into());
        let v = parse(&waybar_error(&err).to_line());
        assert_eq!(v["class"], "disconnected");
        assert_eq!(v["alt"], "disconnected");
        assert_eq!(v["text"], GLYPH_DISCONNECTED);
        assert!(v["tooltip"].as_str().unwrap().contains("not connected"));
        assert_eq!(err.exit_code(), crate::error::exit::NOT_FOUND);
    }

    #[test]
    fn status_json_envelope() {
        let v: Value = serde_json::from_str(&status(
            Style::plain(Format::Json),
            &transport(TransportKind::Usb),
            &sample("usb", true),
        ))
        .unwrap();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["transport"]["kind"], "usb");
        assert_eq!(v["active_profile"], 2);
        assert_eq!(v["split"], Value::Null);
        assert_eq!(
            v["battery"],
            serde_json::json!({"left": null, "right": null, "note": status::BATTERY_NOTE})
        );
    }
}
