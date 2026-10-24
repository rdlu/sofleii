//! The dashboard, drawn from an [`App`] (the [`Snapshot`] it derives, plus
//! the selection and the help flag) and nothing else: no I/O here.
//!
//! Square panes in the docs site's `┌─ Title ───` style: one outer frame,
//! `├─ Profiles ─┤` / `├─ Log ─┤` separators across it, the link state in
//! the top-right corner and the keys along the bottom edge (or, around a
//! write, its spinner and then its result). A risky write's confirm modal
//! is a centred pane over the dashboard, like the help. Below [`WIDE`]
//! columns the summary collapses to one column. Stale values are dimmed,
//! and a field's latest error shows as a `! …` note next to its last good
//! value.
//!
//! Flash mode replaces the summary and profiles with its own body (setup,
//! the steps as they run, the outcome); the link corner and the Log pane
//! stay, and its questions are centred modals like the confirm.

use std::time::Duration;

use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::actions::Confirm;
use super::flash::{self, Ask, Mode, Row as FlashRow, SourceKind, Stage, StepState};
use super::log_pane;
use super::snapshot::{Health, Keyboard, Snapshot};
use super::{App, TICK};
use crate::commands::battery::{Level, Thresholds};
use crate::commands::profiles::Profile;
use crate::commands::update::Status as UpdateStatus;
use crate::session::{LinkState, LogLevel};
use crate::transport::TransportKind;

/// From this width on, the summary is two columns.
pub const WIDE: u16 = 70;
/// Label column width in the summary.
const LABEL: usize = 11;
/// Where the right-hand summary column starts, in the wide layout.
const RIGHT_COL: u16 = 45;
/// A truncated note keeps at least this many cells, or isn't drawn.
const MIN_NOTE: u16 = 8;

/// [`crate::output::USB_LEFT_ONLY`], short enough for the summary.
pub const USB_LEFT_ONLY: &str = "no USB on the left half (the right half's USB only charges)";
const USB_LEFT_ONLY_NARROW: &str = "no USB on the left half (right only charges)";

/// Styles; with colour off (`NO_COLOR`) only bold/dim/reverse remain.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub color: bool,
}

impl Theme {
    fn fg(self, c: Color) -> Style {
        if self.color {
            Style::new().fg(c)
        } else {
            Style::new()
        }
    }
    /// Frame lines and pane titles (the docs site's accent).
    fn border(self) -> Style {
        self.fg(Color::Magenta)
    }
    fn title(self) -> Style {
        self.border().add_modifier(Modifier::BOLD)
    }
    fn label(self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }
    fn good(self) -> Style {
        self.fg(Color::Green)
    }
    fn warn(self) -> Style {
        self.fg(Color::Yellow)
    }
    fn bad(self) -> Style {
        self.fg(Color::Red)
    }
    fn level(self, l: Level) -> Style {
        match l {
            Level::Ok => self.good(),
            Level::Warning => self.warn(),
            Level::Critical => self.bad(),
            Level::Unknown => self.label(),
        }
    }
}

/// Dims `spans` when the value is stale.
fn freshness(spans: Vec<Span<'static>>, stale: bool) -> Vec<Span<'static>> {
    if !stale {
        return spans;
    }
    spans
        .into_iter()
        .map(|s| {
            let style = s.style.add_modifier(Modifier::DIM);
            s.style(style)
        })
        .collect()
}

/// A field's right-hand note: its error, else `fresh` (an age, say).
fn note(h: &Health, fresh: Option<String>, t: Theme) -> Option<Span<'static>> {
    match &h.error {
        Some(e) => Some(Span::styled(format!("! {e}"), t.warn())),
        None => fresh.map(|f| Span::styled(f, t.label())),
    }
}

/// One summary row: two `label value` cells, or one cell with an optional
/// right-aligned note (`read 12s ago`, or an error, truncated to fit).
enum Row<'a> {
    Pair(Line<'a>, Line<'a>),
    Full(Line<'a>, Option<Span<'a>>),
}

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let t = Theme { color: app.color };
    let wide = area.width >= WIDE;
    let snap = app.view();

    let outer = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(t.border())
        .title_top(header(&snap, t))
        .title_top(corner(&snap, t).right_aligned())
        .title_bottom(match &app.flash_mode {
            Some(m) => flash_keys(m, t),
            None => bottom(app, area.width, wide, t),
        });
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if let Some(mode) = &app.flash_mode {
        flash_screen(f, area, inner, app, &snap, mode, t);
        return;
    }

    let rows = summary(&snap, wide, t);
    let selected = app.selected.min(
        snap.keyboard
            .as_ref()
            .map_or(0, |k| k.profiles.len().saturating_sub(1)),
    );
    let profiles = profile_lines(&snap, selected, wide, t);
    let [sum, sep1, prof, sep2, log] = Layout::vertical([
        Constraint::Length(rows.len() as u16),
        Constraint::Length(1),
        Constraint::Length(profiles.len() as u16),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);

    for (row, y) in rows.into_iter().zip(sum.y..sum.bottom()) {
        let r = Rect {
            y,
            height: 1,
            ..sum
        };
        draw_row(f.buffer_mut(), r, row, wide);
    }
    f.render_widget(Paragraph::new(profiles), prof);
    let status_note = snap.keyboard.as_ref().and_then(|k| {
        note(
            &k.status,
            k.status
                .stale
                .then(|| k.status.age.map(|a| format!("read {}", ago(a))))
                .flatten(),
            t,
        )
    });
    separator(f.buffer_mut(), area, sep1, "Profiles", status_note, t);
    let filter = if app.debug {
        "Log (debug)"
    } else {
        "Log (info)"
    };
    separator(f.buffer_mut(), area, sep2, filter, None, t);
    f.render_widget(
        Paragraph::new(log_lines(&snap, log.height, app.debug, &app.tz, t)),
        log,
    );

    if app.help {
        help(f, area, t);
    }
    if let Some(c) = &app.confirm {
        confirm(f, area, c, t);
    }
}

fn header(snap: &Snapshot, t: Theme) -> Line<'static> {
    let name = snap
        .keyboard
        .as_ref()
        .map_or("Eyelash Sofle", |k| k.name.as_str())
        .to_string();
    Line::from(vec![
        Span::styled("─ ", t.border()),
        Span::styled(format!("sofleii · {name}"), t.title()),
        Span::raw(" "),
    ])
}

/// `usb` / `ble`, as the CLI writes them.
fn lower(k: Option<TransportKind>) -> String {
    k.map_or_else(String::new, |k| format!("{k} "))
}

/// Whole seconds, rounded up: `retrying in 1s` until it retries.
fn secs_up(d: Duration) -> u64 {
    d.as_millis().div_ceil(1000) as u64
}

/// Top-right: the link state.
fn corner(snap: &Snapshot, t: Theme) -> Line<'static> {
    let (dot, text, style) = match &snap.link {
        LinkState::Searching => ("○", "searching".to_string(), t.warn()),
        LinkState::Connecting { kind } => ("◌", format!("{}connecting", lower(*kind)), t.warn()),
        LinkState::Connected { via } => (
            "●",
            format!(
                "{} {}{}",
                via.transport.kind,
                via.transport.device,
                if via.auto { " (auto)" } else { "" }
            ),
            t.good(),
        ),
        LinkState::Deaf { kind, .. } => ("◐", format!("{}not answering", lower(*kind)), t.warn()),
        LinkState::Suspended => ("◇", "paused for flashing".into(), t.warn()),
        LinkState::Lost { retry_in, .. } => (
            "✕",
            format!("lost, retrying in {}s", secs_up(*retry_in)),
            t.bad(),
        ),
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(dot, style),
        Span::raw(format!(" {text} ")),
        Span::styled("─", t.border()),
    ])
}

/// Braille spinner frames, one per [`TICK`].
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The bottom edge: a write's spinner while it's in flight, then its
/// result for a moment, else the keys.
fn bottom(app: &App, width: u16, wide: bool, t: Theme) -> Line<'static> {
    let (mark, text, style) = if let Some(p) = &app.action {
        let n = app
            .now
            .duration_since(p.since)
            .unwrap_or(Duration::ZERO)
            .as_millis()
            / TICK.as_millis();
        let frame = SPINNER[n as usize % SPINNER.len()];
        (frame, super::actions::doing(&p.req), t.warn())
    } else if let Some(fl) = &app.flash {
        match fl.level {
            LogLevel::Error => ("✕", fl.text.clone(), t.bad()),
            LogLevel::Warn => ("!", fl.text.clone(), t.warn()),
            LogLevel::Info | LogLevel::Debug => ("✓", fl.text.clone(), t.good()),
        }
    } else {
        return footer(wide, t);
    };
    // `└ ` and ` ─┘` around it.
    let room = width.saturating_sub(6);
    let span = fit(Span::styled(format!("{mark} {text}"), style), room)
        .unwrap_or_else(|| Span::styled(mark, style));
    Line::from(vec![Span::raw(" "), span, Span::raw(" ")])
}

fn footer(wide: bool, t: Theme) -> Line<'static> {
    let keys: &[(&str, &str)] = if wide {
        &[
            ("q", "quit"),
            ("r", "refresh"),
            ("⏎", "switch"),
            ("o", "output"),
            ("u", "update"),
            ("F", "flash"),
            ("?", "help"),
        ]
    } else {
        &[
            ("q", "quit"),
            ("r", "refresh"),
            ("⏎", "switch"),
            ("o", "output"),
            ("?", "help"),
        ]
    };
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", t.label()));
        }
        spans.push(Span::styled(*k, Style::new().add_modifier(Modifier::BOLD)));
        spans.push(Span::raw(format!(" {what}")));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// `label` padded to the label column, then the value.
fn kv<'a>(label: &'a str, value: Vec<Span<'a>>, t: Theme) -> Line<'a> {
    let mut spans = vec![Span::styled(format!("{label:<LABEL$}"), t.label())];
    spans.extend(value);
    Line::from(spans)
}

/// `text` cut into lines of at most `width` characters, at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The rows before anything is known: what the link is doing, and why.
fn link_rows(link: &LinkState, wide: bool, t: Theme) -> Vec<Row<'static>> {
    let (status, detail) = match link {
        LinkState::Searching => (
            Span::styled("Searching…", t.warn()),
            Some("USB serial \"Eyelash Sofle\", then BLE through BlueZ".to_string()),
        ),
        LinkState::Connecting { kind } => (
            Span::styled(
                match kind {
                    Some(k) => format!("Connecting over {}…", kind_name(*k)),
                    None => "Connecting…".into(),
                },
                t.warn(),
            ),
            None,
        ),
        LinkState::Connected { .. } => (Span::styled("Connected", t.good()), None),
        LinkState::Deaf { kind, hint } => (
            Span::styled(
                match kind {
                    Some(k) => format!("{} open, but Studio doesn't answer", kind_name(*k)),
                    None => "Studio doesn't answer".into(),
                },
                t.warn(),
            ),
            hint.clone(),
        ),
        LinkState::Suspended => (Span::styled("Paused while flashing", t.warn()), None),
        LinkState::Lost { reason, retry_in } => (
            Span::styled(
                format!("Lost; retrying in {}s", secs_up(*retry_in)),
                t.bad(),
            ),
            Some(reason.clone()),
        ),
    };
    let mut rows = vec![Row::Full(kv("Status", vec![status], t), None)];
    // The hint gets a few lines; the log has it in full.
    let width = if wide { 64 } else { 44 };
    for l in detail
        .map(|d| wrap(&d, width))
        .unwrap_or_default()
        .into_iter()
        .take(3)
    {
        rows.push(Row::Full(kv("", vec![Span::styled(l, t.label())], t), None));
    }
    rows
}

fn summary(snap: &Snapshot, wide: bool, t: Theme) -> Vec<Row<'static>> {
    let Some(k) = &snap.keyboard else {
        return link_rows(&snap.link, wide, t);
    };
    let stale = k.status.stale;
    let pairs = [
        (
            kv("Transport", transport(k, &snap.link, t), t),
            kv("Output", freshness(output(k, t), stale), t),
        ),
        (
            kv("Profile", freshness(active_profile(k, t), stale), t),
            kv("Studio", studio(k, t), t),
        ),
    ];
    let mut rows = Vec::new();
    for (l, r) in pairs {
        if wide {
            rows.push(Row::Pair(l, r));
        } else {
            rows.push(Row::Full(l, None));
            rows.push(Row::Full(r, None));
        }
    }
    if k.usb_left_only {
        let why = if wide {
            USB_LEFT_ONLY
        } else {
            USB_LEFT_ONLY_NARROW
        };
        rows.push(Row::Full(
            kv("", vec![Span::styled(why, t.warn())], t),
            None,
        ));
    }
    let cells = if wide { 10 } else { 5 };
    let b = &k.battery.health;
    rows.push(Row::Full(
        kv("Battery", freshness(battery(k, cells, t), b.stale), t),
        note(b, b.age.map(|a| format!("read {}", ago(a))), t),
    ));
    let (fw, checked) = firmware(k, t);
    rows.push(Row::Full(kv("Firmware", fw, t), checked));
    let split = match &k.split {
        Some(s) => match (s.connected, s.known) {
            (Some(true), _) => Span::styled("connected", t.good()),
            (Some(false), _) if wide => {
                Span::styled("disconnected (right half off or out of range)", t.bad())
            }
            (Some(false), _) => Span::styled("disconnected (right half off)", t.bad()),
            (None, true) => Span::styled("unknown (not a split central)", t.label()),
            (None, false) if wide => {
                Span::styled("unknown (firmware without the split-info fix)", t.label())
            }
            (None, false) => Span::styled("unknown (old firmware)", t.label()),
        },
        None => Span::styled("—", t.label()),
    };
    let h = &k.split_health;
    rows.push(Row::Full(
        kv("Split", freshness(vec![split], h.stale), t),
        note(h, None, t),
    ));
    rows
}

/// `note` cut to `room` cells with `…`, or `None` if that leaves too little.
fn fit(note: Span<'_>, room: u16) -> Option<Span<'_>> {
    let w = note.width() as u16;
    if w <= room {
        return Some(note);
    }
    if room < MIN_NOTE {
        return None;
    }
    let cut: String = note.content.chars().take(usize::from(room) - 1).collect();
    Some(Span::styled(format!("{cut}…"), note.style))
}

fn draw_row(buf: &mut Buffer, r: Rect, row: Row<'_>, wide: bool) {
    // One cell of padding on each side, as in the panes below.
    let r = Rect {
        x: r.x + 1,
        width: r.width.saturating_sub(2),
        ..r
    };
    match row {
        Row::Pair(left, right) => {
            let split = if wide {
                RIGHT_COL.min(r.width)
            } else {
                r.width
            };
            buf.set_line(r.x, r.y, &left, split.saturating_sub(1));
            buf.set_line(r.x + split, r.y, &right, r.width - split);
        }
        Row::Full(line, note) => {
            let used = line.width() as u16;
            buf.set_line(r.x, r.y, &line, r.width);
            // Only after the value with a gap of two.
            if let Some(note) = note.and_then(|n| fit(n, r.width.saturating_sub(used + 2))) {
                let w = note.width() as u16;
                buf.set_span(r.x + r.width - w, r.y, &note, w);
            }
        }
    }
}

fn kind_name(k: TransportKind) -> &'static str {
    match k {
        TransportKind::Usb => "USB",
        TransportKind::Ble => "BLE",
    }
}

/// The open link, or what the link is doing while the last values stay.
fn transport(k: &Keyboard, link: &LinkState, t: Theme) -> Vec<Span<'static>> {
    if let Some(tr) = &k.transport {
        let mut s = format!("{} {}", kind_name(tr.kind), tr.device);
        if k.auto {
            s.push_str(" (auto)");
        }
        return vec![Span::raw(s)];
    }
    vec![match link {
        LinkState::Lost { retry_in, .. } => {
            Span::styled(format!("lost, retry in {}s", secs_up(*retry_in)), t.bad())
        }
        LinkState::Deaf { .. } => Span::styled("not answering", t.warn()),
        LinkState::Suspended => Span::styled("paused", t.warn()),
        LinkState::Connecting { .. } => Span::styled("connecting…", t.warn()),
        LinkState::Searching | LinkState::Connected { .. } => Span::styled("searching…", t.warn()),
    }]
}

fn output(k: &Keyboard, t: Theme) -> Vec<Span<'static>> {
    let now = k.output.map_or("—", kind_name);
    let style = if k.usb_left_only {
        t.warn()
    } else {
        Style::new()
    };
    match k.output_priority {
        Some(p) => vec![Span::styled(format!("{now}  (priority {p})"), style)],
        None => vec![Span::styled(now, style)],
    }
}

fn active_profile(k: &Keyboard, t: Theme) -> Vec<Span<'static>> {
    let Some(p) = k.active() else {
        return vec![Span::styled("—", t.label())];
    };
    let mut s = vec![Span::raw(p.index.to_string())];
    if let Some(n) = &p.name {
        s.push(Span::raw(format!(" \"{n}\"")));
    }
    s.push(Span::raw("  "));
    s.push(if p.connected {
        Span::styled("connected", t.good())
    } else {
        Span::styled("not connected", t.warn())
    });
    s
}

fn studio(k: &Keyboard, t: Theme) -> Vec<Span<'static>> {
    vec![match k.studio_locked {
        Some(true) => Span::styled("locked", t.warn()),
        Some(false) => Span::raw("unlocked"),
        None => Span::styled("—", t.label()),
    }]
}

/// `████████░░` for `pct`, in `cells` cells.
pub fn bar(pct: u8, cells: usize) -> String {
    let full = (usize::from(pct.min(100)) * cells + 50) / 100;
    format!("{}{}", "█".repeat(full), "░".repeat(cells - full))
}

fn battery(k: &Keyboard, cells: usize, t: Theme) -> Vec<Span<'static>> {
    let th = Thresholds::default();
    let half = |name: &str, p: Option<u8>| -> Vec<Span<'static>> {
        let style = t.level(th.level(p));
        match p {
            Some(p) => vec![
                Span::raw(format!("{name} ")),
                Span::styled(bar(p, cells), style),
                Span::styled(format!(" {p:>3}%"), style),
            ],
            None => vec![Span::raw(format!("{name} ")), Span::styled("—", t.label())],
        }
    };
    let mut s = half("L", k.battery.left);
    s.push(Span::raw("   "));
    if k.battery.right_disconnected {
        s.extend([Span::raw("R "), Span::styled("disconnected", t.bad())]);
    } else if k.battery.right_settling {
        s.extend([Span::raw("R "), Span::styled("reading…", t.label())]);
    } else {
        s.extend(half("R", k.battery.right));
    }
    s
}

fn firmware(k: &Keyboard, t: Theme) -> (Vec<Span<'static>>, Option<Span<'static>>) {
    let fw = &k.firmware;
    let running = fw.running.clone().unwrap_or_else(|| "—".into());
    let mut s = vec![Span::raw(running), Span::styled(" · ", t.label())];
    let h = &fw.update_health;
    // The update check's error first, then the info read's.
    let err = if h.error.is_some() { h } else { &fw.info };
    let Some(u) = &fw.update else {
        s.push(Span::styled("not checked yet", t.label()));
        return (s, note(err, None, t));
    };
    s.push(match u.status {
        UpdateStatus::UpToDate => Span::styled(format!("up to date with {}", u.latest), t.good()),
        UpdateStatus::Ahead => Span::styled(format!("ahead of {}", u.latest), t.good()),
        UpdateStatus::Behind => Span::styled(format!("update: {}", u.latest), t.warn()),
        UpdateStatus::Unknown => Span::styled(format!("unknown build ({})", u.latest), t.warn()),
    });
    if let Some(v) = &u.cli_available {
        s.push(Span::styled(" · ", t.label()));
        s.push(Span::styled(format!("sofleii v{v} available"), t.warn()));
    }
    let checked = format!("checked {}", ago(u.age));
    (freshness(s, h.stale), note(err, Some(checked), t))
}

/// `12s ago`, `2m ago`, `3h ago`, `2d ago`.
pub fn ago(d: Duration) -> String {
    crate::output::ago(d)
}

fn profile_state(p: &Profile) -> String {
    let mut parts = Vec::new();
    if p.active {
        parts.push("active");
    }
    if p.connected {
        parts.push("connected");
    }
    if p.bonded {
        parts.push("bonded");
    }
    if parts.is_empty() {
        "empty".into()
    } else {
        parts.join(" · ")
    }
}

fn profile_lines(snap: &Snapshot, selected: usize, wide: bool, t: Theme) -> Vec<Line<'static>> {
    let Some(k) = &snap.keyboard else {
        return vec![Line::styled(" —", t.label())];
    };
    if k.profiles.is_empty() {
        // Read (an age) and empty, or not read yet.
        let why = if k.status.age.is_some() {
            " no BLE profiles reported"
        } else {
            " —"
        };
        return vec![Line::styled(why, t.label())];
    }
    k.profiles
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let sel = i == selected;
            let name = p.name.as_deref().unwrap_or("—");
            let mut s = format!(" {} {:<3}{:<9}", if sel { "▶" } else { " " }, p.index, name);
            if wide {
                s.push_str(&format!("{:<20}", p.address.as_deref().unwrap_or("")));
            }
            s.push_str(&profile_state(p));
            let mut style = if sel {
                t.title()
            } else if p.bonded {
                Style::new()
            } else {
                t.label()
            };
            if k.status.stale {
                style = style.add_modifier(Modifier::DIM);
            }
            Line::styled(s, style)
        })
        .collect()
}

/// The newest lines that fit in `height`, with the time in `tz`. A run of
/// the same line (the lease released on every poll, say) shows once, at
/// its latest time, with `(×N)`. Debug lines only with `debug` (`d`).
fn log_lines(
    snap: &Snapshot,
    height: u16,
    debug: bool,
    tz: &TimeZone,
    t: Theme,
) -> Vec<Line<'static>> {
    let mut runs: Vec<(&super::LogLine, usize)> = Vec::new();
    for l in snap
        .log
        .iter()
        .filter(|l| debug || l.level != LogLevel::Debug)
    {
        match runs.last_mut() {
            Some((last, n)) if last.text == l.text && last.level == l.level => {
                *last = l;
                *n += 1;
            }
            _ => runs.push((l, 1)),
        }
    }
    if runs.is_empty() {
        return vec![Line::styled(" nothing yet", t.label())];
    }
    let skip = runs.len().saturating_sub(usize::from(height));
    runs[skip..]
        .iter()
        .map(|(l, n)| {
            let style = match l.level {
                LogLevel::Error => t.bad(),
                LogLevel::Warn => t.warn(),
                LogLevel::Info => Style::new(),
                LogLevel::Debug => t.label(),
            };
            let mut spans = vec![
                Span::styled(format!(" {} ", log_pane::clock(l.at, tz)), t.label()),
                Span::styled(l.text.clone(), style),
            ];
            if *n > 1 {
                spans.push(Span::styled(format!(" (×{n})"), t.label()));
            }
            Line::from(spans)
        })
        .collect()
}

/// `├─ Title ───┤` across the outer frame, on `at`'s row, with an optional
/// note at the right (`├─ Profiles ── ! timed out ─┤`).
fn separator(
    buf: &mut Buffer,
    outer: Rect,
    at: Rect,
    title: &str,
    note: Option<Span<'_>>,
    t: Theme,
) {
    if at.height == 0 || outer.width < 2 {
        return;
    }
    let y = at.y;
    let line = format!(
        "├─ {title} {}┤",
        "─".repeat(usize::from(outer.width).saturating_sub(title.chars().count() + 5))
    );
    buf.set_stringn(outer.x, y, &line, usize::from(outer.width), t.border());
    buf.set_stringn(outer.x + 3, y, title, title.len(), t.title());
    // `├─ Title ─ ` before, ` ─┤` after.
    let before = title.chars().count() as u16 + 6;
    let room = outer.width.saturating_sub(before + 4);
    if let Some(n) = note.and_then(|n| fit(n, room)) {
        let w = n.width() as u16;
        let x = outer.x + outer.width - 3 - w;
        buf.set_stringn(x - 1, y, " ", 1, Style::new());
        buf.set_span(x, y, &n, w);
        buf.set_stringn(x + w, y, " ", 1, Style::new());
    }
}

/// Every key.
const HELP: &[(&str, &str)] = &[
    ("q  Ctrl-C", "quit"),
    ("Esc", "close this help; quit from the dashboard"),
    ("↑ ↓  k j", "select a profile"),
    ("⏎  s", "switch to the selected profile"),
    ("o", "toggle the output priority usb / ble"),
    ("r", "refresh everything now"),
    ("u", "check for a firmware update now"),
    ("d", "debug lines in the log on / off"),
    ("F", "flash firmware onto the halves"),
    ("?", "this help"),
    ("", "risky switches ask first; only y confirms"),
];

/// A pane's `─ Title ` for the top edge.
fn pane_title(title: &str, style: Style, t: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("─ ", t.border()),
        Span::styled(title.to_string(), style),
        Span::raw(" "),
    ])
}

/// `w` x `h` centred in `area`, clamped to it.
fn centred(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// The confirm modal: what will happen, wrapped, then `Continue?`; the
/// keys on its bottom edge. Anything but `y` cancels, `⏎` included.
fn confirm(f: &mut Frame, area: Rect, c: &Confirm, t: Theme) {
    let w = 60.min(area.width.saturating_sub(4));
    let text_w = usize::from(w.saturating_sub(4)).max(10);
    let mut lines = vec![Line::raw("")];
    lines.extend(
        wrap(&c.text, text_w)
            .into_iter()
            .map(|l| Line::raw(format!(" {l}"))),
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " Continue?",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let h = lines.len() as u16 + 2;
    let r = centred(area, w, h);
    let title_style = match c.risk {
        crate::session::Risk::Destructive => t.bad().add_modifier(Modifier::BOLD),
        _ => t.title(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(t.border())
        .title_top(pane_title(&c.title, title_style, t))
        .title_bottom(Line::from(vec![
            Span::raw(" "),
            Span::styled("y", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(" confirm · "),
            Span::styled("n", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(" / "),
            Span::styled("Esc", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(" cancel "),
        ]));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

fn help(f: &mut Frame, area: Rect, t: Theme) {
    let w = 60.min(area.width.saturating_sub(4));
    let h = (HELP.len() as u16 + 2).min(area.height.saturating_sub(2));
    let r = centred(area, w, h);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, what)| {
            Line::from(vec![
                Span::styled(
                    format!(" {k:<11}"),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Span::raw(*what),
            ])
        })
        .collect();
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(t.border())
        .title_top(pane_title("Help", t.title(), t))
        .title_bottom(Line::from(" ? or Esc closes "));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

/// Flash mode's keys for the bottom edge.
fn flash_keys(m: &Mode, t: Theme) -> Line<'static> {
    let keys: &[(&str, &str)] = match &m.stage {
        Stage::Setup(s) if s.editing => &[("⏎", "done"), ("⌫", "erase")],
        Stage::Setup(_) => &[
            ("⏎", "start"),
            ("s", "source"),
            ("space", "toggle"),
            ("↑↓", "move"),
            ("Esc", "back"),
        ],
        Stage::Running(r) if r.ask.is_some() => &[],
        Stage::Running(r) if r.writing() => &[("", "writing: don't unplug the half")],
        Stage::Running(r) if r.cancelling => &[("", "cancelling…")],
        Stage::Running(_) => &[("Esc", "cancel (before anything is written)")],
        Stage::Done(_) => &[("⏎", "back to the dashboard")],
    };
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", t.label()));
        }
        if !k.is_empty() {
            spans.push(Span::styled(*k, Style::new().add_modifier(Modifier::BOLD)));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::raw(*what));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

fn mark(on: bool) -> &'static str {
    if on { "[x]" } else { "[ ]" }
}

/// Setup: source, branch, plan.
fn flash_setup(s: &flash::Setup, width: u16, t: Theme) -> Vec<Line<'static>> {
    let text_w = usize::from(width.saturating_sub(3)).max(20);
    let source = match s.source {
        SourceKind::Release => "newest release".to_string(),
        SourceKind::Ci if s.token => "newest green CI build".to_string(),
        SourceKind::Ci => "newest green CI build (needs GH_TOKEN)".to_string(),
        SourceKind::Local if s.local.is_empty() => "local build (none found)".to_string(),
        SourceKind::Local => format!(
            "local build: {}",
            s.local
                .iter()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let mut lines = vec![Line::raw("")];
    for row in s.rows() {
        let (label, value) = match row {
            FlashRow::Source => ("Firmware", format!("‹ {source} ›")),
            FlashRow::Branch if s.editing => ("Branch", format!("{}▏", s.branch)),
            FlashRow::Branch => ("Branch", s.branch.clone()),
            FlashRow::Reset => ("Plan", format!("{} settings reset first", mark(s.reset))),
            FlashRow::Left => ("", format!("{} left half", mark(s.left))),
            FlashRow::Right => ("", format!("{} right half", mark(s.right))),
        };
        let style = if row == s.cursor {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {label:<LABEL$}"), t.label()),
            Span::styled(format!(" {value} "), style),
        ]));
    }
    lines.push(Line::raw(""));
    let hint = if s.reset {
        "settings reset clears the half's settings and Bluetooth bonds: two double-taps per half"
    } else {
        "each half: double-tap reset when asked; sofleii checks the chip before writing"
    };
    for l in wrap(hint, text_w) {
        lines.push(Line::styled(format!(" {l}"), t.label()));
    }
    if let Some(p) = &s.problem {
        for (i, l) in wrap(p, text_w.saturating_sub(2)).into_iter().enumerate() {
            let mark = if i == 0 { "!" } else { " " };
            lines.push(Line::styled(format!(" {mark} {l}"), t.warn()));
        }
    }
    lines
}

fn step_mark(state: &StepState, t: Theme) -> Span<'static> {
    match state {
        StepState::Pending => Span::styled("○", t.label()),
        StepState::Waiting(_) | StepState::Active | StepState::Writing { .. } => {
            Span::styled("●", t.warn())
        }
        StepState::Flashed => Span::styled("✓", t.good()),
        StepState::Stopped => Span::styled("✕", t.bad()),
    }
}

/// The steps, the current one with its commentary.
fn flash_steps(
    steps: &[flash::StepView],
    current: Option<usize>,
    now: std::time::SystemTime,
    width: u16,
    t: Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            step_mark(&s.state, t),
            Span::styled(
                format!(" {} {}", i + 1, flash::step_title(&s.step)),
                Style::new().add_modifier(Modifier::BOLD),
            ),
        ]));
        if current != Some(i) && !matches!(s.state, StepState::Stopped) {
            continue;
        }
        if let StepState::Waiting(since) = s.state {
            let secs = now
                .duration_since(since)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "     Double-tap RESET on the {} half…",
                        s.step.half.as_str().to_ascii_uppercase()
                    ),
                    t.warn().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  waiting {}:{:02}", secs / 60, secs % 60),
                    t.label(),
                ),
            ]));
        }
        for l in &s.lines {
            let style = if l.contains("REFUSED") {
                t.bad()
            } else if l.starts_with("decision") && l.contains("flash (") {
                t.good()
            } else {
                Style::new()
            };
            lines.push(Line::styled(format!("     {l}"), style));
        }
        if let StepState::Writing { written, total } = s.state {
            let cells = usize::from(width.saturating_sub(30)).clamp(10, 40);
            let filled = (written * cells).checked_div(total).unwrap_or(cells);
            lines.push(Line::from(vec![
                Span::styled("     copying    ", t.label()),
                Span::styled("█".repeat(filled), t.good()),
                Span::styled("░".repeat(cells - filled), t.label()),
                Span::raw(format!(" {} / {} KB", written / 1024, total / 1024)),
            ]));
        }
    }
    lines
}

/// The outcome, and the left half's version once the resumed link reads
/// it.
fn flash_done(
    d: &flash::Done,
    session: &crate::session::Snapshot,
    now: std::time::SystemTime,
    t: Theme,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw("")];
    lines.extend(flash_steps(&d.steps, None, now, 60, t));
    lines.push(Line::raw(""));
    match &d.result {
        Ok(r) if r.complete() => {
            lines.push(Line::styled(
                " ✓ flashed",
                t.good().add_modifier(Modifier::BOLD),
            ));
            let left = r.steps.iter().any(|s| {
                s.step.role == crate::flash::Role::Left
                    && matches!(
                        s.outcome,
                        crate::commands::flash::job::Outcome::Flashed { .. }
                    )
            });
            if left {
                let info = &session.info;
                let fresh = info
                    .value
                    .as_ref()
                    .filter(|_| info.at.is_some_and(|at| at > d.at))
                    .and_then(|i| i.build.zmk_config_version.clone());
                lines.push(Line::raw(match fresh {
                    Some(v) => format!(" left half now runs {v}"),
                    None => " reading the left half's version once it's back…".into(),
                }));
            }
        }
        Ok(r) => {
            lines.push(Line::styled(
                " ✕ stopped",
                t.bad().add_modifier(Modifier::BOLD),
            ));
            if let Some(s) = r.steps.last() {
                let half = s.step.half.as_str();
                lines.push(Line::styled(
                    format!(
                        " if that chip really is your {half} half: sofleii flash {half} --adopt {half}"
                    ),
                    t.label(),
                ));
            }
        }
        Err(e) => {
            lines.push(Line::styled(format!(" ✕ {}", e.message), t.bad()));
        }
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn flash_screen(
    f: &mut Frame,
    area: Rect,
    inner: Rect,
    app: &App,
    snap: &Snapshot,
    mode: &Mode,
    t: Theme,
) {
    let body = match &mode.stage {
        Stage::Setup(s) => flash_setup(s, inner.width, t),
        Stage::Running(r) => {
            let mut l = vec![Line::raw("")];
            if r.fetched.is_empty() {
                l.push(Line::styled(" fetching the firmware…", t.warn()));
            } else {
                l.extend(r.fetched.iter().map(|x| Line::raw(format!(" {x}"))));
            }
            l.push(Line::raw(""));
            l.extend(flash_steps(&r.steps, r.current, app.now, inner.width, t));
            l
        }
        Stage::Done(d) => flash_done(d, &app.session, app.now, t),
    };
    let log_h = 6.min(inner.height / 3);
    let body_h = inner.height.saturating_sub(log_h + 1);
    let [top, sep, log] = Layout::vertical([
        Constraint::Length(body_h),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    // The newest lines when the steps overflow.
    let skip = body.len().saturating_sub(usize::from(top.height));
    f.render_widget(
        Paragraph::new(body.into_iter().skip(skip).collect::<Vec<_>>()),
        top,
    );
    separator(f.buffer_mut(), area, sep, "Log", None, t);
    f.render_widget(
        Paragraph::new(log_lines(snap, log.height, app.debug, &app.tz, t)),
        log,
    );
    if let Stage::Running(r) = &mode.stage
        && let Some(a) = &r.ask
    {
        flash_ask(f, area, a, &r.typed, t);
    }
}

/// The job's question as a modal.
fn flash_ask(f: &mut Frame, area: Rect, a: &Ask, typed: &str, t: Theme) {
    let w = 64.min(area.width.saturating_sub(4));
    let text_w = usize::from(w.saturating_sub(4)).max(10);
    let (title, text, prompt, keys): (String, String, Line, Vec<Span>) = match a {
        Ask::Confirm { step, file } => (
            format!("Flash the {} half?", step.half.as_str()),
            format!(
                "Write {file} to this chip ({})? The half reboots into it when the copy is done.",
                flash::step_title(step)
            ),
            Line::styled(" Flash it?", Style::new().add_modifier(Modifier::BOLD)),
            vec![
                Span::raw(" "),
                Span::styled("y", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(" flash · any other key stops "),
            ],
        ),
        Ask::Adopt {
            step,
            serial,
            reason,
        } => (
            "Unknown chip".to_string(),
            format!(
                "{reason}. Nothing vouches for this chip{}. If it is your {h} half, type {h}: \
                 sofleii remembers it by serial from then on.",
                serial
                    .as_deref()
                    .map(|s| format!(" ({s})"))
                    .unwrap_or_default(),
                h = step.half.as_str()
            ),
            Line::from(vec![
                Span::styled(" Half: ", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(format!("{typed}▏")),
            ]),
            vec![
                Span::raw(" "),
                Span::styled("⏎", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(" adopt · "),
                Span::styled("Esc", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(" stop "),
            ],
        ),
    };
    let mut lines = vec![Line::raw("")];
    lines.extend(
        wrap(&text, text_w)
            .into_iter()
            .map(|l| Line::raw(format!(" {l}"))),
    );
    lines.push(Line::raw(""));
    lines.push(prompt);
    let h = lines.len() as u16 + 2;
    let r = centred(area, w, h);
    let style = match a {
        Ask::Adopt { .. } => t.warn().add_modifier(Modifier::BOLD),
        Ask::Confirm { .. } => t.title(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(t.border())
        .title_top(pane_title(&title, style, t))
        .title_bottom(Line::from(keys));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}
