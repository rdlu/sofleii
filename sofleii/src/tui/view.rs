//! The dashboard, drawn from an [`App`] (its [`Snapshot`] plus the
//! selection and the help flag) and nothing else: no I/O here.
//!
//! Square panes in the docs site's `┌─ Title ───` style: one outer frame,
//! `├─ Profiles ─┤` / `├─ Log ─┤` separators across it, the transport in the
//! top-right corner and the keys along the bottom edge. Below [`WIDE`]
//! columns the summary collapses to one column.

use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::App;
use super::snapshot::{Keyboard, Snapshot};
use crate::commands::battery::{Level, Thresholds};
use crate::commands::profiles::Profile;
use crate::commands::update::Status as UpdateStatus;
use crate::transport::TransportKind;

/// From this width on, the summary is two columns.
pub const WIDE: u16 = 70;
/// Label column width in the summary.
const LABEL: usize = 11;
/// Where the right-hand summary column starts, in the wide layout.
const RIGHT_COL: u16 = 45;

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

/// One summary row: two `label value` cells, or one cell with an optional
/// right-aligned note (`read 12s ago`).
enum Row<'a> {
    Pair(Line<'a>, Line<'a>),
    Full(Line<'a>, Option<Span<'a>>),
}

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let t = Theme { color: app.color };
    let wide = area.width >= WIDE;
    let snap = &app.snapshot;

    let outer = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(t.border())
        .title_top(header(snap, t))
        .title_top(corner(snap, t).right_aligned())
        .title_bottom(footer(wide, t));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = summary(snap, wide, t);
    let profiles = profile_lines(snap, app.selected, wide, t);
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
    separator(f.buffer_mut(), area, sep1, "Profiles", t);
    separator(f.buffer_mut(), area, sep2, "Log", t);
    f.render_widget(Paragraph::new(log_lines(snap, log.height, t)), log);

    if app.help {
        help(f, area, t);
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

/// Top-right: the link, or the search.
fn corner(snap: &Snapshot, t: Theme) -> Line<'static> {
    let (dot, text, style) = match &snap.keyboard {
        Some(k) => (
            "●",
            format!("{} {}", k.transport.kind, k.transport.device),
            t.good(),
        ),
        None => ("○", "searching".to_string(), t.warn()),
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(dot, style),
        Span::raw(format!(" {text} ")),
        Span::styled("─", t.border()),
    ])
}

fn footer(wide: bool, t: Theme) -> Line<'static> {
    let keys: &[(&str, &str)] = if wide {
        &[
            ("q", "quit"),
            ("r", "refresh"),
            ("↑↓/jk", "select"),
            ("⏎", "switch"),
            ("o", "output"),
            ("u", "update"),
            ("?", "help"),
        ]
    } else {
        &[("q", "quit"), ("↑↓/jk", "select"), ("?", "help")]
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

fn summary(snap: &Snapshot, wide: bool, t: Theme) -> Vec<Row<'_>> {
    let Some(k) = &snap.keyboard else {
        return vec![
            Row::Full(
                kv("Status", vec![Span::styled("Searching…", t.warn())], t),
                None,
            ),
            Row::Full(
                kv(
                    "",
                    vec![Span::styled(
                        "USB serial \"Eyelash Sofle\", then BLE through BlueZ",
                        t.label(),
                    )],
                    t,
                ),
                None,
            ),
            Row::Full(
                kv(
                    "",
                    vec![Span::styled("(no live data in this build yet)", t.label())],
                    t,
                ),
                None,
            ),
        ];
    };
    let pairs = [
        (kv("Transport", transport(k), t), kv("Output", output(k), t)),
        (
            kv("Profile", active_profile(k, t), t),
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
    let cells = if wide { 10 } else { 5 };
    rows.push(Row::Full(
        kv("Battery", battery(k, cells, t), t),
        k.battery
            .age
            .map(|a| Span::styled(format!("read {}", ago(a)), t.label())),
    ));
    let (fw, checked) = firmware(k, t);
    rows.push(Row::Full(kv("Firmware", fw, t), checked));
    let split = match &k.split {
        Some(s) if s.peripheral_connected => Span::styled("connected", t.good()),
        Some(_) => Span::styled("disconnected", t.bad()),
        None if wide => Span::styled("unavailable (get_split_info is broken upstream)", t.label()),
        None => Span::styled("unavailable (broken upstream)", t.label()),
    };
    rows.push(Row::Full(kv("Split", vec![split], t), None));
    rows
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
            if let Some(note) = note {
                let w = note.width() as u16;
                // Only when it fits after the value with a gap.
                if used + 2 + w <= r.width {
                    buf.set_span(r.x + r.width - w, r.y, &note, w);
                }
            }
        }
    }
}

fn kind(k: TransportKind) -> &'static str {
    match k {
        TransportKind::Usb => "USB",
        TransportKind::Ble => "BLE",
    }
}

fn transport(k: &Keyboard) -> Vec<Span<'static>> {
    let mut s = format!("{} {}", kind(k.transport.kind), k.transport.device);
    if k.auto {
        s.push_str(" (auto)");
    }
    vec![Span::raw(s)]
}

fn output(k: &Keyboard) -> Vec<Span<'static>> {
    let now = k.output.map_or("—", kind);
    match k.output_priority {
        Some(p) => vec![Span::raw(format!("{now}  (priority {p})"))],
        None => vec![Span::raw(now)],
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
    vec![if k.studio_locked {
        Span::styled("locked", t.warn())
    } else {
        Span::raw("unlocked")
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
    s.extend(half("R", k.battery.right));
    s
}

fn firmware(k: &Keyboard, t: Theme) -> (Vec<Span<'static>>, Option<Span<'static>>) {
    let running = k.firmware.running.clone().unwrap_or_else(|| "—".into());
    let mut s = vec![Span::raw(running), Span::styled(" · ", t.label())];
    let Some(u) = &k.firmware.update else {
        s.push(Span::styled("not checked", t.label()));
        return (s, None);
    };
    s.push(match u.status {
        UpdateStatus::UpToDate => Span::styled(format!("up to date with {}", u.latest), t.good()),
        UpdateStatus::Ahead => Span::styled(format!("ahead of {}", u.latest), t.good()),
        UpdateStatus::Behind => Span::styled(format!("update: {}", u.latest), t.warn()),
        UpdateStatus::Unknown => Span::styled(format!("unknown build ({})", u.latest), t.warn()),
    });
    (
        s,
        Some(Span::styled(format!("checked {}", ago(u.age)), t.label())),
    )
}

/// `12s ago`, `2m ago`, `3h ago`, `2d ago`.
pub fn ago(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s ago"),
        60..3600 => format!("{}m ago", s / 60),
        3600..86_400 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86_400),
    }
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
        return vec![Line::styled(" no BLE profiles reported", t.label())];
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
            let style = if sel {
                t.title()
            } else if p.bonded {
                Style::new()
            } else {
                t.label()
            };
            Line::styled(s, style)
        })
        .collect()
}

/// The newest lines that fit in `height`.
fn log_lines(snap: &Snapshot, height: u16, t: Theme) -> Vec<Line<'static>> {
    if snap.log.is_empty() {
        return vec![Line::styled(" nothing yet", t.label())];
    }
    let skip = snap.log.len().saturating_sub(usize::from(height));
    snap.log[skip..]
        .iter()
        .map(|l| {
            Line::from(vec![
                Span::styled(format!(" {} ", l.at), t.label()),
                Span::raw(l.text.clone()),
            ])
        })
        .collect()
}

/// `├─ Title ───┤` across the outer frame, on `at`'s row.
fn separator(buf: &mut Buffer, outer: Rect, at: Rect, title: &str, t: Theme) {
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
}

/// Every key; the ones whose actions come in later PRs say so.
const HELP: &[(&str, &str)] = &[
    ("q  Ctrl-C", "quit"),
    ("Esc", "close this help; quit from the dashboard"),
    ("↑ ↓  k j", "select a profile"),
    ("?", "this help"),
    ("r", "refresh             (not yet)"),
    ("⏎", "switch to profile   (not yet)"),
    ("o", "output usb / ble    (not yet)"),
    ("u", "update check        (not yet)"),
];

fn help(f: &mut Frame, area: Rect, t: Theme) {
    let w = 56.min(area.width.saturating_sub(4));
    let h = (HELP.len() as u16 + 2).min(area.height.saturating_sub(2));
    let r = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
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
        .title_top(Line::from(vec![
            Span::styled("─ ", t.border()),
            Span::styled("Help", t.title()),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(" ? or Esc closes "));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines).block(block), r);
}
