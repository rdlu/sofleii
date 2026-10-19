//! Rendering of command results: human-readable text, the versioned JSON
//! contract from #44 (`"schema": 1`, snake_case keys, explicit `null`s), or
//! waybar's custom-module JSON for `status` and `battery`.

use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::Error;
use crate::commands::{
    battery::{Reading, Thresholds},
    info::Info,
    profiles::Profiles,
    status::{BatteryLevel, Status},
    update::{self, short},
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
    /// For waybar's `format-icons` (`battery`: the lower half's level).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<u8>,
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
///
/// - Over USB, USB counts as connected (we're talking over it).
/// - Over BLE, whether USB is plugged in is unknown, so only the priority
///   and the active profile's connection count: a connected profile means
///   `ble`. That also matches the firmware, which only answers Studio on the
///   selected endpoint, so a reply over BLE means BLE is selected (and, with
///   priority `usb`, that USB isn't connected).
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
        percentage: None,
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
        percentage: None,
    }
}

/// Glyphs in `battery`'s waybar `text`: nf-md-keyboard U+F030C, and
/// nf-md-keyboard_off U+F0310 when the keyboard isn't found.
pub const GLYPH_KEYBOARD: &str = "\u{f030c}";
pub const GLYPH_KEYBOARD_OFF: &str = "\u{f0310}";

fn pct_or_dash(p: Option<u8>) -> String {
    p.map_or_else(|| "-".to_string(), |p| p.to_string())
}

/// `battery --format waybar`: `"󰌌 L 100 R 77"`, class by the lower half.
pub fn waybar_battery(r: &Reading, t: Thresholds) -> Waybar {
    let class = t.classify(r).as_str();
    let pct = |p: Option<u8>| p.map_or_else(|| "-".to_string(), |p| format!("{p}%"));
    let mut tooltip = format!(
        "{}\nleft: {}\nright: {}\nsource: BlueZ GATT {}",
        r.device.name,
        pct(r.left.percent),
        pct(r.right.percent),
        r.device.address
    );
    if let Some(n) = &r.note {
        tooltip.push_str(&format!("\nnote: {n}"));
    }
    Waybar {
        text: format!(
            "{GLYPH_KEYBOARD} L {} R {}",
            pct_or_dash(r.left.percent),
            pct_or_dash(r.right.percent)
        ),
        tooltip,
        class,
        alt: class,
        percentage: r.min_percent(),
    }
}

/// waybar output for a failed `battery`: still valid JSON.
pub fn waybar_battery_error(err: &Error) -> Waybar {
    let headline = match err {
        Error::NotFound(_) => "Eyelash Sofle not connected over BLE",
        _ => "Eyelash Sofle: battery read failed",
    };
    Waybar {
        text: GLYPH_KEYBOARD_OFF.to_string(),
        tooltip: format!("{headline}\n{err}"),
        class: "disconnected",
        alt: "disconnected",
        percentage: None,
    }
}

/// `sofleii battery` in any format. Human: two lines, `left 100%` and
/// `right 77%` (the note, if any, goes to stderr from the CLI).
pub fn battery(style: Style, r: &Reading, t: Thresholds) -> String {
    match style.format {
        Format::Json => {
            #[derive(Serialize)]
            struct Env<'a> {
                schema: u32,
                #[serde(flatten)]
                reading: &'a Reading,
            }
            serde_json::to_string_pretty(&Env {
                schema: SCHEMA,
                reading: r,
            })
            .expect("serialising plain data cannot fail")
        }
        Format::Waybar => waybar_battery(r, t).to_line(),
        Format::Human => {
            let line = |label: &str, p: Option<u8>| {
                let value = p.map_or_else(|| "-".to_string(), |p| format!("{p}%"));
                let value = match p {
                    Some(p) if p < t.critical_below => style.paint("31", &value),
                    Some(p) if p < t.warn_below => style.paint("33", &value),
                    _ => value,
                };
                format!("{} {value}\n", style.dim(&format!("{label:<5}")))
            };
            line("left", r.left.percent) + &line("right", r.right.percent)
        }
    }
}

/// Label/value rows shared by the human output and the waybar tooltip.
fn status_rows(transport: &TransportInfo, s: &Status) -> Vec<(&'static str, String)> {
    let over_ble = transport.kind == TransportKind::Ble;
    let output = match (effective_output(transport, s), s.output_priority) {
        (Some("ble"), Some("usb")) if over_ble => "ble (priority usb; USB not connected)".into(),
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
    if let Some(note) = &s.battery.note {
        battery.push_str(&format!(" ({note})"));
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
    let b = &i.build;
    if b.available {
        let dirty = |v: Option<&str>, d: Option<bool>| match (v, d) {
            // `git describe --dirty` already appends "-dirty"; show it once.
            (Some(v), Some(true)) => format!(
                "{} {}",
                v.strip_suffix("-dirty").unwrap_or(v),
                style.paint("33", "(dirty)")
            ),
            (v, _) => or_dash(v).to_string(),
        };
        let h = &i.hardware;
        let reset = match (h.reset_cause, &h.reset_causes) {
            (Some(m), Some(names)) if !names.is_empty() => {
                format!("{} (0x{m:x})", names.join(", "))
            }
            (Some(m), _) => format!("0x{m:x}"),
            _ => "-".into(),
        };
        let rows = [
            (
                "firmware",
                dirty(b.zmk_config_version.as_deref(), b.zmk_config_dirty),
            ),
            ("zmk", dirty(b.zmk_version.as_deref(), b.zmk_dirty)),
            ("zephyr", or_dash(b.zephyr_version.as_deref()).to_string()),
            ("board", or_dash(b.board.as_deref()).to_string()),
            ("built", or_dash(b.build_timestamp.as_deref()).to_string()),
            ("build hash", or_dash(b.build_hash.as_deref()).to_string()),
            ("device id", or_dash(h.device_id.as_deref()).to_string()),
            ("reset cause", reset),
        ];
        for (l, v) in rows {
            let _ = writeln!(w, "{}{v}", label(l));
        }
    } else {
        let _ = writeln!(w, "{}{}", label("build"), or_dash(b.note.as_deref()));
    }
    let _ = writeln!(w, "{}", label("subsystems"));
    if i.custom_subsystems.is_empty() {
        let _ = writeln!(w, "  (none)");
    }
    for s in &i.custom_subsystems {
        let _ = writeln!(w, "  {:>2}  {}", s.index, s.identifier);
    }
    String::from_utf8(out).expect("utf-8")
}

fn commits(n: Option<u32>) -> String {
    match n {
        Some(1) => "1 commit".into(),
        Some(n) => format!("{n} commits"),
        None => "? commits".into(),
    }
}

/// The one-line verdict: `up to date (v3.1.0, 5c53b30)`, `3 commits behind
/// v3.1.0`, `2 commits ahead of v3.1.0`, `diverged from v3.1.0: …`.
pub fn update_headline(c: &update::Check) -> String {
    let tag = &c.latest.tag;
    match c.status {
        update::Status::UpToDate => format!("up to date ({tag}, {})", short(&c.latest.commit)),
        update::Status::Behind => format!("{} behind {tag}", commits(c.commits_behind)),
        update::Status::Ahead => format!("{} ahead of {tag}", commits(c.commits_ahead)),
        update::Status::Diverged => format!(
            "diverged from {tag}: {} behind, {} ahead",
            commits(c.commits_behind),
            commits(c.commits_ahead)
        ),
        update::Status::Unknown => format!("unknown build, can't compare with {tag}"),
    }
}

/// `sofleii update check`: the verdict, then `running`/`latest` rows and a
/// note for dirty or unknown builds.
pub fn update_check(style: Style, transport: &TransportInfo, c: &update::Check) -> String {
    if style.format == Format::Json {
        return json(transport, c);
    }
    let mut out = Vec::new();
    let w = &mut out;
    let label = |s: &str| style.dim(&format!("{s:<16}"));
    let headline = update_headline(c);
    let headline = match c.status {
        update::Status::UpToDate | update::Status::Ahead => style.green(&headline),
        _ => style.paint("33", &headline),
    };
    let _ = writeln!(w, "{}", style.bold(&headline));
    let r = &c.running;
    let mut running = match &r.version {
        // `git describe --dirty` already appends "-dirty"; show it once.
        Some(v) => v.strip_suffix("-dirty").unwrap_or(v).to_string(),
        None => "-".into(),
    };
    // An exact tag carries no SHA: show the commit it resolved to.
    if let Some(commit) = &r.commit
        && !running.contains(commit.as_str())
    {
        running.push_str(&format!(" ({})", short(commit)));
    }
    if r.dirty {
        running.push_str(&format!(" {}", style.paint("33", "(dirty)")));
    }
    let _ = writeln!(w, "{}{running}", label("running"));
    let published = c
        .latest
        .published_at
        .as_deref()
        .map(|p| format!(", published {p}"))
        .unwrap_or_default();
    let _ = writeln!(
        w,
        "{}{} {}{published}",
        label("latest"),
        c.latest.tag,
        short(&c.latest.commit)
    );
    if r.dirty {
        let base = r.commit.as_deref().map_or("its commit", short);
        let _ = writeln!(
            w,
            "{}dirty build: uncommitted changes on top of {base}, compared as {base}",
            label("note")
        );
    }
    if let Some(n) = &c.note {
        let _ = writeln!(w, "{}{n}", label("note"));
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
    use crate::commands::info::{self, Device};
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
        // Over BLE with the profile not connected: nothing known to be up.
        let v = parse(&status(
            Style::plain(Format::Waybar),
            &transport(TransportKind::Ble),
            &sample("usb", false),
        ));
        assert_eq!(v["class"], "disconnected");
    }

    #[test]
    fn waybar_over_ble_follows_the_profile() {
        // Over BLE, USB is unknown: a connected profile means ble, whatever
        // the priority (ZMK only answers Studio on the selected endpoint).
        let t = TransportInfo {
            kind: TransportKind::Ble,
            device: "DD:1F:23:76:6E:71".into(),
        };
        for priority in ["usb", "ble"] {
            let v = parse(&status(
                Style::plain(Format::Waybar),
                &t,
                &sample(priority, true),
            ));
            assert_eq!(v["class"], "ble", "priority {priority}");
            assert_eq!(v["text"], format!("{GLYPH_BLE} 2"));
        }
        let v = parse(&status(
            Style::plain(Format::Waybar),
            &t,
            &sample("usb", true),
        ));
        let tip = v["tooltip"].as_str().unwrap();
        assert!(tip.contains("transport: ble DD:1F:23:76:6E:71"), "{tip}");
        assert!(
            tip.contains("output: ble (priority usb; USB not connected)"),
            "{tip}"
        );
        let v: Value = serde_json::from_str(&status(
            Style::plain(Format::Json),
            &t,
            &sample("ble", true),
        ))
        .unwrap();
        assert_eq!(
            v["transport"],
            serde_json::json!({"kind": "ble", "address": "DD:1F:23:76:6E:71"})
        );
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

    fn reading() -> Reading {
        use crate::commands::battery::{self, Query, fake::FakeGatt};
        battery::run(&mut FakeGatt::measured(), &Query::default()).unwrap()
    }

    #[test]
    fn waybar_battery_ok() {
        let v = parse(&battery(
            Style::plain(Format::Waybar),
            &reading(),
            Thresholds::default(),
        ));
        assert_eq!(v["text"], format!("{GLYPH_KEYBOARD} L 100 R 77"));
        assert_eq!(v["class"], "ok");
        assert_eq!(v["alt"], "ok");
        assert_eq!(v["percentage"], 77);
        let tip = v["tooltip"].as_str().unwrap();
        assert!(
            tip.contains("left: 100%") && tip.contains("right: 77%"),
            "{tip}"
        );
        assert!(tip.contains("BlueZ GATT DD:1F:23:76:6E:71"), "{tip}");
    }

    #[test]
    fn waybar_battery_warning_and_critical() {
        let mut r = reading();
        r.right.percent = Some(15);
        let v = parse(&battery(
            Style::plain(Format::Waybar),
            &r,
            Thresholds::default(),
        ));
        assert_eq!(v["class"], "warning");
        assert_eq!(v["text"], format!("{GLYPH_KEYBOARD} L 100 R 15"));
        r.right.percent = None;
        r.left.percent = Some(5);
        let v = parse(&battery(
            Style::plain(Format::Waybar),
            &r,
            Thresholds::default(),
        ));
        assert_eq!(v["class"], "critical");
        assert_eq!(v["text"], format!("{GLYPH_KEYBOARD} L 5 R -"));
    }

    #[test]
    fn waybar_battery_not_found() {
        let err = Error::NotFound("BLE device named \"Eyelash Sofle\" is known to BlueZ but not connected to this host over BLE".into());
        let v = parse(&waybar_battery_error(&err).to_line());
        assert_eq!(v["class"], "disconnected");
        assert_eq!(v["text"], GLYPH_KEYBOARD_OFF);
        assert!(v.get("percentage").is_none());
        assert!(v["tooltip"].as_str().unwrap().contains("not connected"));
    }

    #[test]
    fn battery_json_and_human() {
        let v: Value = serde_json::from_str(&battery(
            Style::plain(Format::Json),
            &reading(),
            Thresholds::default(),
        ))
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "schema": 1, "source": "bluez",
                "device": {"name": "Eyelash Sofle", "address": "DD:1F:23:76:6E:71"},
                "left": {"percent": 100}, "right": {"percent": 77}, "note": null
            })
        );
        let human = battery(
            Style::plain(Format::Human),
            &reading(),
            Thresholds::default(),
        );
        assert_eq!(human, "left  100%\nright 77%\n");
    }

    #[test]
    fn status_shows_battery_row() {
        let mut s = sample("usb", true);
        s.battery = status::Battery::from_reading(Ok(reading()));
        let out = status(
            Style::plain(Format::Human),
            &transport(TransportKind::Usb),
            &s,
        );
        assert!(out.contains("left 100%, right 77%\n"), "{out}");
        let v = parse(&status(
            Style::plain(Format::Waybar),
            &transport(TransportKind::Usb),
            &s,
        ));
        assert!(
            v["tooltip"]
                .as_str()
                .unwrap()
                .contains("battery: left 100%, right 77%")
        );
    }

    fn info_sample(build: info::Build, hardware: info::Hardware) -> Info {
        Info {
            device: Device {
                name: "Eyelash Sofle".into(),
                serial_number: Some("9470".into()),
            },
            build,
            hardware,
            studio: info::Studio { locked: false },
            custom_subsystems: vec![info::CustomSubsystem {
                index: 2,
                identifier: "zmk__device_info".into(),
                ui_urls: vec![],
            }],
        }
    }

    #[test]
    fn info_json_golden_with_build_info() {
        let (b, h) = info::from_response(info::tests::sample_response());
        let got: Value = serde_json::from_str(&super::info(
            Style::plain(Format::Json),
            &transport(TransportKind::Usb),
            &info_sample(b, h),
        ))
        .unwrap();
        let want = serde_json::json!({
            "schema": 1,
            "transport": {"kind": "usb", "device": "/dev/ttyACM1"},
            "device": {"name": "Eyelash Sofle", "serial_number": "9470"},
            "build": {
                "available": true,
                "note": null,
                "zmk_version": "v0.3-198-ge5c9b691",
                "zmk_dirty": false,
                "zmk_config_version": "v3.0.0-65-g1e468b7-dirty",
                "zmk_config_dirty": true,
                "module_version": "8cedd2d",
                "module_dirty": false,
                "zephyr_version": "4.1.0",
                "build_timestamp": "2026-10-17T20:50:06Z",
                "board": "nice_nano",
                "build_hash": "1e37f75107aeb7993596e9d27b668701ed790839"
            },
            "hardware": {
                "device_id": "a1b2c3d4e5f60718",
                "reset_cause": 9,
                "reset_causes": ["pin", "por"],
                "flash_size_kb": 1024,
                "sram_size_kb": 256
            },
            "studio": {"locked": false},
            "custom_subsystems": [
                {"index": 2, "identifier": "zmk__device_info", "ui_urls": []}
            ]
        });
        assert_eq!(got, want);
    }

    #[test]
    fn info_json_golden_without_subsystem() {
        let got: Value = serde_json::from_str(&super::info(
            Style::plain(Format::Json),
            &transport(TransportKind::Usb),
            &info_sample(
                info::Build::unavailable(info::BUILD_MISSING_NOTE),
                info::Hardware::default(),
            ),
        ))
        .unwrap();
        let want_build = serde_json::json!({
            "available": false,
            "note": info::BUILD_MISSING_NOTE,
            "zmk_version": null, "zmk_dirty": null,
            "zmk_config_version": null, "zmk_config_dirty": null,
            "module_version": null, "module_dirty": null,
            "zephyr_version": null, "build_timestamp": null,
            "board": null, "build_hash": null
        });
        assert_eq!(got["build"], want_build);
        assert_eq!(
            got["hardware"],
            serde_json::json!({
                "device_id": null, "reset_cause": null, "reset_causes": null,
                "flash_size_kb": null, "sram_size_kb": null
            })
        );
    }

    #[test]
    fn info_human_shows_versions_and_dirty_flag() {
        let (b, h) = info::from_response(info::tests::sample_response());
        let out = super::info(
            Style::plain(Format::Human),
            &transport(TransportKind::Usb),
            &info_sample(b, h),
        );
        for want in [
            "firmware        v3.0.0-65-g1e468b7 (dirty)\n",
            "zmk             v0.3-198-ge5c9b691\n",
            "zephyr          4.1.0",
            "board           nice_nano",
            "built           2026-10-17T20:50:06Z",
            "reset cause     pin, por (0x9)",
        ] {
            assert!(out.contains(want), "missing {want:?} in:\n{out}");
        }

        let out = super::info(
            Style::plain(Format::Human),
            &transport(TransportKind::Usb),
            &info_sample(
                info::Build::unavailable(info::BUILD_MISSING_NOTE),
                info::Hardware::default(),
            ),
        );
        assert!(out.contains("build           unavailable: this firmware has no zmk__device_info"));
        assert!(!out.contains("\nfirmware "));
    }

    fn update_sample(
        version: &str,
        status: update::Status,
        behind: u32,
        ahead: u32,
    ) -> update::Check {
        let known = status != update::Status::Unknown;
        update::Check {
            running: update::Running {
                version: Some(version.into()),
                commit: Some(update::parse_describe(version).0).and_then(|d| match d {
                    update::Describe::Commit(c) => Some(c),
                    _ => None,
                }),
                dirty: version.ends_with("-dirty"),
            },
            latest: update::Latest {
                tag: "v3.1.0".into(),
                commit: "5c53b30fce99031611442b1a3ccbbe7052327020".into(),
                published_at: Some("2026-10-19T19:20:31Z".into()),
            },
            status,
            commits_behind: known.then_some(behind),
            commits_ahead: known.then_some(ahead),
            note: None,
        }
    }

    #[test]
    fn update_check_human() {
        let t = transport(TransportKind::Usb);
        let out = update_check(
            Style::plain(Format::Human),
            &t,
            &update_sample("5c53b30", update::Status::UpToDate, 0, 0),
        );
        assert_eq!(
            out,
            "up to date (v3.1.0, 5c53b30)\n\
             running         5c53b30\n\
             latest          v3.1.0 5c53b30, published 2026-10-19T19:20:31Z\n"
        );
        let out = update_check(
            Style::plain(Format::Human),
            &t,
            &update_sample("v3.0.0-68-gf9a665f-dirty", update::Status::Behind, 3, 0),
        );
        assert!(out.starts_with("3 commits behind v3.1.0\n"), "{out}");
        assert!(
            out.contains("running         v3.0.0-68-gf9a665f (dirty)\n"),
            "{out}"
        );
        assert!(
            out.contains("note            dirty build: uncommitted changes on top of f9a665f"),
            "{out}"
        );
        for (status, b, a, want) in [
            (update::Status::Ahead, 0, 1, "1 commit ahead of v3.1.0\n"),
            (
                update::Status::Diverged,
                4,
                2,
                "diverged from v3.1.0: 4 commits behind, 2 commits ahead\n",
            ),
            (
                update::Status::Unknown,
                0,
                0,
                "unknown build, can't compare with v3.1.0\n",
            ),
        ] {
            let out = update_check(
                Style::plain(Format::Human),
                &t,
                &update_sample("abc1234", status, b, a),
            );
            assert!(out.starts_with(want), "{out}");
        }
        // An exact tag shows the commit it resolved to.
        let mut c = update_sample("v3.1.0", update::Status::UpToDate, 0, 0);
        c.running.commit = Some(c.latest.commit.clone());
        let out = update_check(Style::plain(Format::Human), &t, &c);
        assert!(out.contains("running         v3.1.0 (5c53b30)\n"), "{out}");
    }

    #[test]
    fn update_check_json_envelope() {
        let v: Value = serde_json::from_str(&update_check(
            Style::plain(Format::Json),
            &transport(TransportKind::Usb),
            &update_sample("5c53b30", update::Status::UpToDate, 0, 0),
        ))
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "schema": 1,
                "transport": {"kind": "usb", "device": "/dev/ttyACM1"},
                "running": {"version": "5c53b30", "commit": "5c53b30", "dirty": false},
                "latest": {"tag": "v3.1.0",
                           "commit": "5c53b30fce99031611442b1a3ccbbe7052327020",
                           "published_at": "2026-10-19T19:20:31Z"},
                "status": "up_to_date",
                "commits_behind": 0,
                "commits_ahead": 0,
                "note": null
            })
        );
    }
}
