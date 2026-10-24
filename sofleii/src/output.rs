//! Rendering of command results: human-readable text, the versioned JSON
//! contract from #44 (`"schema": 1`, snake_case keys, explicit `null`s), or
//! waybar's custom-module JSON for `status` and `battery`. `keymap show`
//! draws each layer as a text grid on the keyboard's physical layout.

use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::commands::{
    battery::{Reading, Thresholds},
    info::Info,
    profiles::Profiles,
    status::{BatteryLevel, Split, Status, effective_output},
    update::{self, short},
};
use crate::commands::{flash as flash_cmd, keymap as keymap_cmd, output as output_cmd, profile};
use crate::error::{Error, ErrorView};
use crate::keymap::render::LEGEND_WIDTH;
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
    struct ErrorEnvelope {
        schema: u32,
        error: ErrorView,
    }
    let env = ErrorEnvelope {
        schema: SCHEMA,
        error: err.view(),
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

/// `flash identify`: whose chip the mounted bootloader drive is. No
/// `transport` in the JSON: nothing talks to the keyboard.
pub fn flash_identify(style: Style, r: &flash_cmd::Identified) -> String {
    use crate::flash::Firmware;
    if style.format == Format::Json {
        #[derive(Serialize)]
        struct Env<'a> {
            schema: u32,
            #[serde(flatten)]
            r: &'a flash_cmd::Identified,
        }
        return serde_json::to_string_pretty(&Env { schema: SCHEMA, r })
            .expect("serialising plain data cannot fail");
    }
    let label = |s: &str| style.dim(&format!("{s:<11}"));
    let b = &r.bootloader;
    let bootloader = match (&b.version, &b.model) {
        (Some(v), Some(m)) => format!("{m}, {v}"),
        (v, m) => or_dash(m.as_deref().or(v.as_deref())).to_string(),
    };
    let firmware = match r.identity.firmware {
        Firmware::Sofleii => "sofleii (identity record)",
        Firmware::Legacy => "legacy (no identity record; told by its strings)",
        Firmware::SettingsReset => "settings_reset",
        Firmware::Unknown => "unknown",
    };
    let mut out = String::new();
    for (k, v) in [
        ("drive", r.mount.display().to_string()),
        ("bootloader", bootloader),
        ("firmware", firmware.to_string()),
        (
            "keyboard",
            or_dash(r.identity.keyboard.as_deref()).to_string(),
        ),
        (
            "half",
            or_dash(r.identity.half.map(|h| h.as_str())).to_string(),
        ),
    ] {
        out.push_str(&format!("{} {v}\n", label(k)));
    }
    let verdict = match r.verdict {
        flash_cmd::Verdict::Match => style.green("match"),
        flash_cmd::Verdict::Mismatch => style.paint("31", "MISMATCH: do not flash"),
        flash_cmd::Verdict::Unknown => style.paint("33", "unknown: not safe to flash"),
    };
    let against = r.expected.map_or_else(
        || "this keyboard".to_string(),
        |h| format!("{} half", h.as_str()),
    );
    out.push_str(&format!("{} {against}: {verdict}\n", label("expected")));
    for n in &r.identity.notes {
        out.push_str(&format!("{} {n}\n", label("note")));
    }
    out
}

/// Why output priority usb over a BLE link means no USB: only the central
/// (left) half does USB (ZMK's `ZMK_USB` depends on `!ZMK_SPLIT ||
/// ZMK_SPLIT_ROLE_CENTRAL`), so a cable in the right half only charges it.
pub const USB_LEFT_ONLY: &str =
    "USB not connected to the left half (the right half's USB only charges)";

/// The split link in words, for `status` and its waybar tooltip.
pub fn split_text(s: &Split) -> &'static str {
    match s.connected {
        Some(true) => "connected",
        Some(false) => "disconnected (right half off or out of range)",
        None if s.known => "unknown (not a split central)",
        None => "unknown (firmware without the split-info fix)",
    }
}

/// Label/value rows shared by the human output and the waybar tooltip.
fn status_rows(transport: &TransportInfo, s: &Status) -> Vec<(&'static str, String)> {
    let over_ble = transport.kind == TransportKind::Ble;
    let output = match (effective_output(transport, s), s.output_priority) {
        (Some("ble"), Some("usb")) if over_ble => format!("ble (priority usb); {USB_LEFT_ONLY}"),
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
    let split = match (&s.split, s.split_note.as_deref()) {
        (Some(sp), _) => split_text(sp).to_string(),
        (None, Some(note)) => note.to_string(),
        (None, None) => "-".to_string(),
    };
    let pct = |b: Option<BatteryLevel>| match b {
        Some(BatteryLevel {
            percent: Some(p), ..
        }) => format!("{p}%"),
        Some(BatteryLevel {
            state: Some(st), ..
        }) => st.to_string(),
        _ => "-".to_string(),
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

/// The running build as a version: `v3.1.0 (f9a665f)`, `v3.0.0+68
/// (f9a665f)`, `v3.2.0 (built from 0858966)` for a build that differs from
/// the release only outside the firmware inputs, or just the commit (or
/// raw version) with no release.
fn running_version(r: &update::Running) -> String {
    let id = match (&r.commit, &r.version) {
        (Some(c), _) => short(c).to_string(),
        // `git describe --dirty` already appends "-dirty"; the row says it once.
        (None, Some(v)) => v.strip_suffix("-dirty").unwrap_or(v).to_string(),
        (None, None) => "-".into(),
    };
    match &r.release {
        Some(rel) if rel.equivalent => format!("{} (built from {id})", rel.label()),
        Some(rel) => format!("{} ({id})", rel.label()),
        None => id,
    }
}

/// The one-line verdict, led by versions: `up to date: v3.1.0 (f9a665f)`,
/// `v3.1.0 available, running v3.0.0+68 (f9a665f)`, `ahead of v3.1.0:
/// running v3.1.0+3 (abc1234)`. The commit counts stay in the JSON.
pub fn update_headline(c: &update::Check) -> String {
    let tag = c.latest.label();
    let mut running = running_version(&c.running);
    if c.running.release.is_none() {
        running.push_str(" (no release)");
    }
    match c.status {
        update::Status::UpToDate => format!("up to date: {running}"),
        update::Status::Behind => format!("{tag} available, running {running}"),
        update::Status::Ahead => format!("ahead of {tag}: running {running}"),
        update::Status::Unknown => format!("can't compare with {tag}: running {running}"),
    }
}

/// `update check --json`: the firmware fields at the top level as before
/// (all `null` under `--cli`), plus the additive `cli` object (`null`
/// under `--firmware`) and the cache fields.
#[derive(Serialize)]
struct UpdateJson<'a> {
    schema: u32,
    /// `null` under `--cli`: the keyboard isn't asked.
    transport: Option<&'a TransportInfo>,
    running: Option<&'a update::Running>,
    latest: Option<&'a update::Latest>,
    status: Option<update::Status>,
    commits_behind: Option<u32>,
    commits_ahead: Option<u32>,
    files_changed: Option<u32>,
    firmware_files_changed: Option<u32>,
    note: Option<&'a str>,
    cli: Option<&'a update::CliCheck>,
    /// When GitHub was asked (by whoever filled the cache, when cached).
    checked_at_ms: u64,
    from_cache: bool,
}

/// The CLI part's verdict: `up to date: v0.1.0`, `v0.2.0 available,
/// running v0.1.0`, `ahead of v0.1.0: running v0.2.0`, `no sofleii
/// release yet (running v0.1.0)`.
pub fn cli_headline(c: &update::CliCheck) -> String {
    let mine = format!("v{}", c.version);
    let latest = c.latest.as_deref().map(|l| format!("v{l}"));
    match (c.status, latest) {
        (update::Status::UpToDate, _) => format!("up to date: {mine}"),
        (update::Status::Behind, Some(l)) => format!("{l} available, running {mine}"),
        (update::Status::Ahead, Some(l)) => format!("ahead of {l}: running {mine}"),
        (_, None) => format!("no sofleii release yet (running {mine})"),
        (_, Some(l)) => format!("can't compare with {l}: running {mine}"),
    }
}

/// `12s ago`, `2m ago`, `3h ago`, `2d ago`.
pub fn ago(d: std::time::Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s ago"),
        60..3600 => format!("{}m ago", s / 60),
        3600..86_400 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// `sofleii update check`: the firmware verdict with its `running`/`latest`
/// rows and notes, then the `sofleii` row (the CLI part), then where the
/// answer came from when it was cached. `transport` is `None` under
/// `--cli`; `now` dates a cached answer.
pub fn update_check(
    style: Style,
    transport: Option<&TransportInfo>,
    c: &update::Checked,
    now: std::time::SystemTime,
) -> String {
    if style.format == Format::Json {
        let fw = c.firmware.as_ref();
        let env = UpdateJson {
            schema: SCHEMA,
            transport,
            running: fw.map(|f| &f.running),
            latest: fw.map(|f| &f.latest),
            status: fw.map(|f| f.status),
            commits_behind: fw.and_then(|f| f.commits_behind),
            commits_ahead: fw.and_then(|f| f.commits_ahead),
            files_changed: fw.and_then(|f| f.files_changed),
            firmware_files_changed: fw.and_then(|f| f.firmware_files_changed),
            note: fw.and_then(|f| f.note.as_deref()),
            cli: c.cli.as_ref(),
            checked_at_ms: c
                .checked_at
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            from_cache: c.from_cache,
        };
        return serde_json::to_string_pretty(&env).expect("serialising plain data cannot fail");
    }
    let label = |s: &str| style.dim(&format!("{s:<16}"));
    let mut text = c
        .firmware
        .as_ref()
        .map(|f| firmware_check(style, f))
        .unwrap_or_default();
    if let Some(cli) = &c.cli {
        let line = cli_headline(cli);
        let line = match cli.status {
            update::Status::UpToDate | update::Status::Ahead => style.green(&line),
            update::Status::Behind => style.paint("33", &line),
            update::Status::Unknown => line,
        };
        text.push_str(&format!("{}{line}\n", label("sofleii")));
        if let Some(n) = cli.note.as_deref().filter(|_| cli.latest.is_some()) {
            text.push_str(&format!("{}{n}\n", label("note")));
        }
    }
    if c.from_cache {
        let age = now.duration_since(c.checked_at).unwrap_or_default();
        text.push_str(&format!(
            "{}{}, cached (--refresh asks GitHub again)\n",
            label("checked"),
            ago(age)
        ));
    }
    text
}

/// The firmware part of `update check`'s text.
fn firmware_check(style: Style, c: &update::Check) -> String {
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
    let mut running = running_version(r);
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
        c.latest.label(),
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

/// The lines after a write's headline: the note, and where Studio answers
/// now when that's not the transport the command used.
fn write_tail(
    style: Style,
    transport: &TransportInfo,
    studio: Option<TransportKind>,
    note: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(k) = studio.filter(|k| *k != transport.kind) {
        out.push_str(&format!(
            "{}\n",
            style.dim(&format!("studio rpc now answers on {k}"))
        ));
    }
    if let Some(n) = note {
        out.push_str(&format!("{}\n", style.dim(&format!("note: {n}"))));
    }
    out
}

/// `sofleii profile switch|rename|unpair`.
pub fn profile_action(style: Style, transport: &TransportInfo, o: &profile::Outcome) -> String {
    if style.format == Format::Json {
        return json(transport, o);
    }
    let n = o.profile;
    let was = o
        .previous_active_profile
        .map_or(String::new(), |p| format!(" (was {p})"));
    let unverified = if o.verified { "" } else { ", not verified" };
    let headline = match o.action {
        "profile_switch" if o.verified => format!("profile {n} is now active{was}"),
        "profile_switch" => format!("profile {n} selected{was}{unverified}"),
        "profile_rename" => match &o.name {
            Some(name) => format!("profile {n} renamed to \"{name}\"{unverified}"),
            None => format!("profile {n} name cleared{unverified}"),
        },
        _ if !o.sent => format!("profile {n} is not bonded; nothing to do"),
        _ => format!("profile {n} unpaired{unverified}"),
    };
    // The no-op unpair's note only repeats the headline.
    let note = o.note.as_deref().filter(|_| o.sent);
    format!("{}\n", style.bold(&headline)) + &write_tail(style, transport, o.studio_transport, note)
}

/// `sofleii output usb|ble`.
pub fn output_action(style: Style, transport: &TransportInfo, o: &output_cmd::Outcome) -> String {
    if style.format == Format::Json {
        return json(transport, o);
    }
    let was = o
        .previous_output_priority
        .map_or(String::new(), |p| format!(" (was {p})"));
    let unverified = if o.verified { "" } else { ", not verified" };
    let headline = format!("output priority: {}{was}{unverified}", o.output_priority);
    format!("{}\n", style.bold(&headline))
        + &write_tail(style, transport, o.studio_transport, o.note.as_deref())
}

/// Columns per key unit in the keymap grids: a 6-wide legend plus a gap,
/// so the Sofle's 16 units fit in 112 columns.
const KEY_COLS: usize = 7;

/// `keymap show`: every layer as a grid on the physical layout.
pub fn keymap_show(style: Style, transport: &TransportInfo, s: &keymap_cmd::Show) -> String {
    if style.format == Format::Json {
        return json(transport, s);
    }
    let live = &s.live;
    let keys = live.physical_layout.as_ref().map(|l| l.keys.as_slice());
    let mut out = String::new();
    let layout = live
        .physical_layout
        .as_ref()
        .map_or(String::new(), |l| format!(", layout \"{}\"", l.name));
    let unsaved = match live.unsaved_changes {
        Some(true) => ", unsaved Studio changes",
        _ => "",
    };
    out.push_str(&format!(
        "{}\n",
        style.dim(&format!(
            "{} layer(s) on the keyboard{layout}{unsaved}",
            live.layers.len()
        ))
    ));
    for layer in &live.layers {
        out.push('\n');
        out.push_str(&keymap_grid(style, layer, keys));
    }
    out
}

/// One layer: a header, then two lines per key row (the tap legend, then
/// the hold side or the kind of key), placed by the keys' x and y.
pub fn keymap_grid(
    style: Style,
    layer: &crate::keymap::Layer,
    keys: Option<&[keymap_cmd::Key]>,
) -> String {
    use std::collections::BTreeMap;
    // (row, column) per position: from the layout, or 16 to a row without.
    let mut rows: BTreeMap<i32, Vec<(usize, &crate::keymap::Binding)>> = BTreeMap::new();
    for b in &layer.bindings {
        let (row, col) = match keys.and_then(|k| k.get(b.position)) {
            Some(k) => (
                (k.y + 50).div_euclid(100),
                ((k.x.max(0) as usize * KEY_COLS) + 50) / 100,
            ),
            None => (
                1000 + (b.position / 16) as i32,
                (b.position % 16) * KEY_COLS,
            ),
        };
        rows.entry(row).or_default().push((col, b));
    }
    let width = LEGEND_WIDTH;
    let mut body = String::new();
    let mut max_end = 0;
    for cells in rows.values_mut() {
        cells.sort_by_key(|&(col, b)| (col, b.position));
        // Keys closer than a cell (rotated thumbs) are pushed right.
        let mut next = 0;
        for c in cells.iter_mut() {
            c.0 = c.0.max(next);
            next = c.0 + width + 1;
        }
        max_end = max_end.max(next);
        // The second line, often blank, also spaces the rows apart.
        for line in 0..2 {
            let mut s = String::new();
            let mut at = 0;
            for &(col, b) in cells.iter() {
                let text = if line == 0 {
                    b.legend.tap.as_str()
                } else {
                    b.legend.hold.as_deref().unwrap_or("")
                };
                let cell = fit(text, width);
                s.push_str(&" ".repeat(col - at));
                let faint = line == 1 || b.legend.tap == "▽";
                s.push_str(&if faint { style.dim(&cell) } else { cell });
                at = col + width;
            }
            body.push_str(s.trim_end());
            body.push('\n');
        }
    }
    let title = format!("{} {}", layer.index, layer.name);
    let id = if layer.id as usize != layer.index {
        format!(" (id {})", layer.id)
    } else {
        String::new()
    };
    let used = title.chars().count() + id.chars().count() + 4;
    let rule = "─".repeat(max_end.saturating_sub(used).max(4));
    format!(
        "{} {}{} {}\n{body}",
        style.dim("──"),
        style.bold(&title),
        style.dim(&id),
        style.dim(&rule)
    )
}

/// `s` centred in `width` columns, cut with `…` when longer.
fn fit(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n > width {
        let cut: String = s.chars().take(width - 1).collect();
        return format!("{cut}…");
    }
    let left = (width - n) / 2;
    format!("{}{s}{}", " ".repeat(left), " ".repeat(width - n - left))
}

/// `keymap diff`: what differs, layer by layer, then a one-line verdict.
pub fn keymap_diff(style: Style, transport: &TransportInfo, r: &keymap_cmd::DiffReport) -> String {
    if style.format == Format::Json {
        return json(transport, r);
    }
    let d = &r.diff;
    let mut out = format!("{}\n", style.dim(&format!("keyboard vs {}", r.keymap_file)));
    for l in d.layers.iter().filter(|l| l.differences > 0) {
        let keys = if l.differences == 1 {
            "key differs"
        } else {
            "keys differ"
        };
        out.push_str(&format!(
            "\n{} {}: {} {keys}\n",
            style.bold(&l.live_index.to_string()),
            style.bold(&l.name),
            l.differences
        ));
        for x in d
            .differences
            .iter()
            .filter(|x| x.live_index == l.live_index)
        {
            out.push_str(&format!(
                "  {}  {} {:<24} {} {}\n",
                style.dim(&format!("pos {:>2}", x.position)),
                style.dim("file"),
                or_dash(x.file.as_deref()),
                style.dim("keyboard"),
                or_dash(x.live.as_deref()),
            ));
        }
    }
    let mut layer_lines = Vec::new();
    for m in &d.reordered {
        layer_lines.push(format!(
            "moved: {} (file index {}, keyboard index {})",
            m.name, m.file_index, m.live_index
        ));
    }
    for n in &d.renamed {
        layer_lines.push(format!(
            "renamed: index {} is {} in the file, {} on the keyboard",
            n.live_index, n.file_name, n.live_name
        ));
    }
    for a in &d.added {
        layer_lines.push(format!(
            "only on the keyboard: {} (index {})",
            a.name, a.index
        ));
    }
    for rm in &d.removed {
        layer_lines.push(format!(
            "only in the file: {} (index {})",
            rm.name, rm.index
        ));
    }
    if !layer_lines.is_empty() {
        out.push('\n');
        for l in layer_lines {
            out.push_str(&format!("{l}\n"));
        }
    }
    let keys: usize = d.differences.len();
    let layers = d.layers.iter().filter(|l| l.differences > 0).count();
    let verdict = if d.drift {
        let mut v = format!("drift: {keys} key(s) differ on {layers} layer(s)");
        let moved = d.reordered.len() + d.renamed.len() + d.added.len() + d.removed.len();
        if moved > 0 {
            v.push_str(&format!(", {moved} layer change(s)"));
        }
        style.paint("33", &v)
    } else {
        style.green(&format!("no drift: {} layer(s) match", d.layers.len()))
    };
    out.push_str(&format!("\n{verdict}\n"));
    if r.unsaved_changes == Some(true) {
        out.push_str("the keyboard also has unsaved Studio changes\n");
    }
    out
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
            tip.contains(
                "output: ble (priority usb); USB not connected to the left half \
                 (the right half's USB only charges)"
            ),
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
    fn human_says_usb_is_left_half_only() {
        // Text only: the JSON keeps `output_priority` and nothing else.
        let t = TransportInfo {
            kind: TransportKind::Ble,
            device: "DD:1F:23:76:6E:71".into(),
        };
        let out = status(Style::plain(Format::Human), &t, &sample("usb", true));
        assert!(
            out.contains(
                "output          ble (priority usb); USB not connected to the left half \
                 (the right half's USB only charges)\n"
            ),
            "{out}"
        );
        let json = status(Style::plain(Format::Json), &t, &sample("usb", true));
        assert!(!json.contains("left half"), "{json}");
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
                "left": {"percent": 100, "state": null}, "right": {"percent": 77, "state": null}, "note": null
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

    fn split(connected: Option<bool>) -> crate::Result<Split> {
        Ok(Split {
            known: connected.is_some(),
            connected,
            is_split: connected.map(|_| true),
            is_central: connected.map(|_| true),
        })
    }

    #[test]
    fn status_split_rows() {
        let usb = transport(TransportKind::Usb);
        let cases = [
            (Some(true), "connected"),
            (Some(false), "disconnected (right half off or out of range)"),
            (None, "unknown (firmware without the split-info fix)"),
        ];
        for (connected, want) in cases {
            let mut s = sample("usb", true);
            s.set_split(split(connected));
            let out = status(Style::plain(Format::Human), &usb, &s);
            assert!(out.contains(&format!("split link      {want}\n")), "{out}");
            let v = parse(&status(Style::plain(Format::Waybar), &usb, &s));
            let tip = v["tooltip"].as_str().unwrap();
            assert!(tip.contains(&format!("\nsplit link: {want}\n")), "{tip}");
        }
        // Not read at all: the note.
        let out = status(Style::plain(Format::Human), &usb, &sample("usb", true));
        assert!(
            out.contains("split link      unavailable: not read\n"),
            "{out}"
        );
    }

    #[test]
    fn status_right_battery_disconnected() {
        let mut s = sample("usb", true);
        s.set_split(split(Some(false)));
        let mut r = reading();
        r.right.percent = Some(0);
        s.set_battery(Ok(r));
        let usb = transport(TransportKind::Usb);
        let out = status(Style::plain(Format::Human), &usb, &s);
        assert!(out.contains("left 100%, right disconnected\n"), "{out}");
        let v: Value = serde_json::from_str(&status(Style::plain(Format::Json), &usb, &s)).unwrap();
        assert_eq!(
            v["battery"]["right"],
            serde_json::json!({"percent": null, "state": "disconnected"})
        );
        assert_eq!(v["split"]["connected"], false);
        assert_eq!(v["split_note"], Value::Null);
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
        release: Option<(&str, u32)>,
    ) -> update::Check {
        let known = status != update::Status::Unknown;
        update::Check {
            running: update::Running {
                version: Some(version.into()),
                commit: match update::parse_describe(version).0 {
                    update::Describe::Commit(c) => Some(c),
                    _ => None,
                },
                dirty: version.ends_with("-dirty"),
                release: release.map(|(tag, n)| update::Release {
                    tag: tag.into(),
                    exact: n == 0,
                    commits_after: n,
                    equivalent: false,
                }),
            },
            latest: update::Latest {
                tag: "sofleii-fw-v3.1.0".into(),
                commit: "5c53b30fce99031611442b1a3ccbbe7052327020".into(),
                published_at: Some("2026-10-19T19:20:31Z".into()),
            },
            status,
            commits_behind: known.then_some(behind),
            commits_ahead: known.then_some(ahead),
            files_changed: known.then_some(0),
            firmware_files_changed: known.then_some(0),
            note: None,
        }
    }

    #[test]
    fn update_check_human() {
        use update::Status::*;
        let t = transport(TransportKind::Usb);
        let human = |c: &update::Check| {
            update_check(
                Style::plain(Format::Human),
                Some(&t),
                &checked(Some(c.clone()), None, false),
                t0(),
            )
        };
        let out = human(&update_sample(
            "f9a665f",
            UpToDate,
            1,
            0,
            Some(("sofleii-fw-v3.1.0", 0)),
        ));
        assert_eq!(
            out,
            "up to date: v3.1.0 (f9a665f)\n\
             running         v3.1.0 (f9a665f)\n\
             latest          v3.1.0 5c53b30, published 2026-10-19T19:20:31Z\n"
        );
        let out = human(&update_sample(
            "sofleii-fw-v3.0.0-68-gf9a665f-dirty",
            Behind,
            3,
            0,
            Some(("sofleii-fw-v3.0.0", 68)),
        ));
        assert!(
            out.starts_with("v3.1.0 available, running v3.0.0+68 (f9a665f)\n"),
            "{out}"
        );
        assert!(!out.contains("3 commits"), "{out}");
        assert!(
            out.contains("running         v3.0.0+68 (f9a665f) (dirty)\n"),
            "{out}"
        );
        assert!(
            out.contains("note            dirty build: uncommitted changes on top of f9a665f"),
            "{out}"
        );
        for (c, want) in [
            (
                update_sample("abc1234", Ahead, 0, 3, Some(("sofleii-fw-v3.1.0", 3))),
                "ahead of v3.1.0: running v3.1.0+3 (abc1234)\n",
            ),
            (
                update_sample("abc1234", Behind, 4, 2, Some(("sofleii-fw-v3.0.0", 9))),
                "v3.1.0 available, running v3.0.0+9 (abc1234)\n",
            ),
            (
                update_sample("abc1234", Behind, 4, 0, None),
                "v3.1.0 available, running abc1234 (no release)\n",
            ),
            (
                update_sample("abc1234", Unknown, 0, 0, None),
                "can't compare with v3.1.0: running abc1234 (no release)\n",
            ),
        ] {
            let out = human(&c);
            assert!(out.starts_with(want), "{out}");
        }
        // An exact tag shows the commit it resolved to.
        let mut c = update_sample("v3.1.0", UpToDate, 0, 0, Some(("sofleii-fw-v3.1.0", 0)));
        c.running.commit = Some(c.latest.commit.clone());
        assert!(human(&c).contains("running         v3.1.0 (5c53b30)\n"));
    }

    #[test]
    fn update_check_json_envelope() {
        let fw = update_sample(
            "5c53b30",
            update::Status::UpToDate,
            0,
            0,
            Some(("sofleii-fw-v3.1.0", 0)),
        );
        let v: Value = serde_json::from_str(&update_check(
            Style::plain(Format::Json),
            Some(&transport(TransportKind::Usb)),
            &checked(Some(fw), Some(cli_sample("0.1.0", Some("0.2.0"))), true),
            t0(),
        ))
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "schema": 1,
                "transport": {"kind": "usb", "device": "/dev/ttyACM1"},
                "running": {"version": "5c53b30", "commit": "5c53b30", "dirty": false,
                            "release": {"tag": "sofleii-fw-v3.1.0", "exact": true, "commits_after": 0,
                                        "equivalent": false}},
                "latest": {"tag": "sofleii-fw-v3.1.0",
                           "commit": "5c53b30fce99031611442b1a3ccbbe7052327020",
                           "published_at": "2026-10-19T19:20:31Z"},
                "status": "up_to_date",
                "commits_behind": 0,
                "commits_ahead": 0,
                "files_changed": 0,
                "firmware_files_changed": 0,
                "note": null,
                "cli": {"version": "0.1.0", "latest": "0.2.0", "tag": "sofleii-tui-v0.2.0",
                        "published_at": "2026-10-20T12:00:00Z", "status": "behind",
                        "note": null},
                "checked_at_ms": 1_800_000_000_000u64 - 2 * 3_600_000,
                "from_cache": true
            })
        );

        // --cli: no keyboard, so no transport and the firmware fields null.
        let v: Value = serde_json::from_str(&update_check(
            Style::plain(Format::Json),
            None,
            &checked(None, Some(cli_sample("0.1.0", None)), false),
            t0(),
        ))
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "schema": 1, "transport": null, "running": null, "latest": null,
                "status": null, "commits_behind": null, "commits_ahead": null,
                "files_changed": null, "firmware_files_changed": null, "note": null,
                "cli": {"version": "0.1.0", "latest": null, "tag": null, "published_at": null,
                        "status": "unknown", "note": update::NO_CLI_RELEASE},
                "checked_at_ms": 1_800_000_000_000u64 - 2 * 3_600_000,
                "from_cache": false
            })
        );
        // --firmware: `cli` is null.
        let v: Value = serde_json::from_str(&update_check(
            Style::plain(Format::Json),
            Some(&transport(TransportKind::Usb)),
            &checked(
                Some(update_sample(
                    "5c53b30",
                    update::Status::UpToDate,
                    0,
                    0,
                    None,
                )),
                None,
                false,
            ),
            t0(),
        ))
        .unwrap();
        assert_eq!(v["cli"], Value::Null);
        assert_eq!(v["status"], "up_to_date");
    }

    fn t0() -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_800_000_000_000)
    }

    /// Checked 2 h before [`t0`].
    fn checked(
        firmware: Option<update::Check>,
        cli: Option<update::CliCheck>,
        from_cache: bool,
    ) -> update::Checked {
        update::Checked {
            firmware,
            cli,
            checked_at: t0() - std::time::Duration::from_secs(2 * 3600),
            from_cache,
        }
    }

    fn cli_sample(mine: &str, latest: Option<&str>) -> update::CliCheck {
        let l = latest.map(|v| update::Listed {
            tag: format!("sofleii-tui-v{v}"),
            published_at: Some("2026-10-20T12:00:00Z".into()),
        });
        update::cli_check(mine, l.as_ref())
    }

    #[test]
    fn update_check_cli_rows() {
        let t = transport(TransportKind::Usb);
        let human =
            |c: &update::Checked| update_check(Style::plain(Format::Human), Some(&t), c, t0());
        for (mine, latest, want) in [
            (
                "0.1.0",
                Some("0.1.0"),
                "sofleii         up to date: v0.1.0\n",
            ),
            (
                "0.1.0",
                Some("0.2.0"),
                "sofleii         v0.2.0 available, running v0.1.0\n",
            ),
            (
                "0.2.0",
                Some("0.1.0"),
                "sofleii         ahead of v0.1.0: running v0.2.0\n",
            ),
            (
                "0.1.0",
                None,
                "sofleii         no sofleii release yet (running v0.1.0)\n",
            ),
        ] {
            // --cli alone: just the row.
            let out = human(&checked(None, Some(cli_sample(mine, latest)), false));
            assert_eq!(out, want);
        }
        // Both: the firmware block, then the row, then the cache line.
        let fw = update_sample(
            "f9a665f",
            update::Status::UpToDate,
            0,
            0,
            Some(("sofleii-fw-v3.1.0", 0)),
        );
        let out = human(&checked(Some(fw), Some(cli_sample("0.1.0", None)), true));
        assert_eq!(
            out,
            "up to date: v3.1.0 (f9a665f)\n\
             running         v3.1.0 (f9a665f)\n\
             latest          v3.1.0 5c53b30, published 2026-10-19T19:20:31Z\n\
             sofleii         no sofleii release yet (running v0.1.0)\n\
             checked         2h ago, cached (--refresh asks GitHub again)\n"
        );
    }

    #[test]
    fn output_action_json_golden_and_human() {
        let o = output_cmd::Outcome {
            action: "output",
            output_priority: "ble",
            previous_output_priority: Some("usb"),
            ok: true,
            reply: false,
            verified: true,
            verified_over: Some(TransportKind::Ble),
            studio_transport: Some(TransportKind::Ble),
            note: Some("no reply over usb".into()),
        };
        let t = transport(TransportKind::Usb);
        let v: Value =
            serde_json::from_str(&output_action(Style::plain(Format::Json), &t, &o)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "schema": 1,
                "transport": {"kind": "usb", "device": "/dev/ttyACM1"},
                "action": "output",
                "output_priority": "ble",
                "previous_output_priority": "usb",
                "ok": true,
                "reply": false,
                "verified": true,
                "verified_over": "ble",
                "studio_transport": "ble",
                "note": "no reply over usb"
            })
        );
        let text = output_action(Style::plain(Format::Human), &t, &o);
        assert_eq!(
            text,
            "output priority: ble (was usb)\nstudio rpc now answers on ble\n\
             note: no reply over usb\n"
        );
    }

    #[test]
    fn profile_action_json_has_explicit_nulls() {
        let o = profile::Outcome {
            action: "profile_switch",
            profile: 2,
            name: None,
            ok: true,
            sent: true,
            reply: true,
            verified: true,
            verified_over: Some(TransportKind::Usb),
            previous_active_profile: Some(0),
            active_profile: Some(2),
            slot: None,
            studio_transport: Some(TransportKind::Usb),
            note: None,
        };
        let t = transport(TransportKind::Usb);
        let v: Value =
            serde_json::from_str(&profile_action(Style::plain(Format::Json), &t, &o)).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["action"], "profile_switch");
        assert_eq!(v["profile"], 2);
        for k in ["name", "slot", "note"] {
            assert!(v[k].is_null() && v.get(k).is_some(), "{k}");
        }
        let text = profile_action(Style::plain(Format::Human), &t, &o);
        assert_eq!(text, "profile 2 is now active (was 0)\n");
    }

    fn keymap_layer() -> (crate::keymap::Layer, Vec<keymap_cmd::Key>) {
        use crate::keymap::{BehaviorRef, Binding, Context, keys, render};
        let ctx = Context::new(vec![], true);
        let bind = |position: usize, label: &str, p1: u32, p2: u32| {
            let r = render::render(label, None, p1, p2, &ctx);
            Binding {
                position,
                behavior: BehaviorRef {
                    id: 1,
                    name: Some(label.into()),
                    display_name: None,
                },
                param1: p1,
                param2: p2,
                legend: render::legend(&r),
                params: r.params,
                zmk: r.zmk,
            }
        };
        let k = |n| keys::value(n).unwrap();
        let layer = crate::keymap::Layer {
            index: 1,
            id: 4,
            name: "NAV".into(),
            bindings: vec![
                bind(0, "kp", k("A"), 0),
                bind(1, "mt", k("LALT"), k("S")),
                bind(2, "trans", 0, 0),
                bind(3, "kp", k("C_VOL_UP"), 0),
            ],
        };
        let key = |position, x, y| keymap_cmd::Key {
            position,
            x,
            y,
            width: 100,
            height: 100,
            r: 0,
            rx: 0,
            ry: 0,
        };
        // Two on a row, one a quarter key lower (same row), one below; 3
        // overlaps 0 and is pushed right.
        let keys = vec![
            key(0, 0, 0),
            key(1, 100, 25),
            key(2, 0, 100),
            key(3, 50, 100),
        ];
        (layer, keys)
    }

    #[test]
    fn keymap_grid_places_keys_by_geometry() {
        let (layer, keys) = keymap_layer();
        let g = keymap_grid(Style::plain(Format::Human), &layer, Some(&keys));
        let lines: Vec<&str> = g.lines().collect();
        assert!(lines[0].starts_with("── 1 NAV (id 4) ─"), "{g}");
        assert_eq!(lines[1], "  A      S");
        assert_eq!(lines[2], "        ALT");
        assert_eq!(lines[3], "  ▽     VOL+");
        assert_eq!(lines[4], "");
        assert_eq!(lines.len(), 5, "{g}");
        // Without a layout, 16 to a row.
        let g = keymap_grid(Style::plain(Format::Human), &layer, None);
        assert!(
            g.lines()
                .nth(1)
                .unwrap()
                .starts_with("  A      S      ▽     VOL+"),
            "{g}"
        );
        assert_eq!(fit("toolongname", 6), "toolo…");
        assert_eq!(fit("ab", 6), "  ab  ");
    }

    #[test]
    fn keymap_diff_text_and_verdicts() {
        use crate::keymap::diff::{Diff, Difference, LayerPair, LayerRef};
        let pair = |differences| LayerPair {
            name: "NAV".into(),
            file_index: 1,
            live_index: 1,
            live_id: 1,
            differences,
        };
        let mut r = keymap_cmd::DiffReport {
            keymap_file: "config/eyelash_sofle.keymap".into(),
            unsaved_changes: Some(false),
            diff: Diff {
                drift: false,
                layers: vec![pair(0)],
                differences: vec![],
                added: vec![],
                removed: vec![],
                reordered: vec![],
                renamed: vec![],
            },
            notes: vec![],
        };
        let t = keymap_diff(
            Style::plain(Format::Human),
            &transport(TransportKind::Usb),
            &r,
        );
        assert!(t.ends_with("no drift: 1 layer(s) match\n"), "{t}");
        r.diff.drift = true;
        r.diff.layers = vec![pair(1)];
        r.diff.differences = vec![Difference {
            layer: "NAV".into(),
            live_index: 1,
            position: 14,
            file: Some("&kp LG(I)".into()),
            live: Some("&kp A".into()),
        }];
        r.diff.removed = vec![LayerRef {
            index: 9,
            name: "MYGAME".into(),
        }];
        r.unsaved_changes = Some(true);
        let t = keymap_diff(
            Style::plain(Format::Human),
            &transport(TransportKind::Usb),
            &r,
        );
        assert!(t.contains("1 NAV: 1 key differs"), "{t}");
        assert!(t.contains("pos 14  file &kp LG(I)"), "{t}");
        assert!(t.contains("keyboard &kp A"), "{t}");
        assert!(t.contains("only in the file: MYGAME (index 9)"), "{t}");
        assert!(
            t.contains("drift: 1 key(s) differ on 1 layer(s), 1 layer change(s)"),
            "{t}"
        );
        assert!(t.contains("unsaved Studio changes"), "{t}");
        let v: Value = serde_json::from_str(&keymap_diff(
            Style::plain(Format::Json),
            &transport(TransportKind::Usb),
            &r,
        ))
        .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["drift"], true);
        assert_eq!(v["differences"][0]["file"], "&kp LG(I)");
        assert_eq!(v["removed"][0]["name"], "MYGAME");
        assert_eq!(r.exit_code(), 1);
    }
}
