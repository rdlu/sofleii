//! Render tests on ratatui's `TestBackend`, driven by scripted session
//! events folded into a `session::Snapshot`, plus key handling and the
//! event loop against a fake session.

use std::cell::RefCell;
use std::sync::atomic::AtomicBool;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};

use super::*;
use crate::commands::profiles::{Profile, Profiles};
use crate::commands::update;
use crate::session::{ConnectedVia, ErrorView, LinkState, Stamped};
use crate::transport::{TransportInfo, TransportKind};

/// 17:42:10 UTC; the tests draw in UTC.
fn t0() -> SystemTime {
    let ts: jiff::Timestamp = "2026-10-19T17:42:10Z".parse().unwrap();
    SystemTime::from(ts)
}

fn at(secs: u64) -> SystemTime {
    t0() + Duration::from_secs(secs)
}

fn app(snap: session::Snapshot, now: SystemTime) -> App {
    let mut a = App::new(snap, false, now);
    a.tz = TimeZone::UTC;
    a
}

fn demo() -> App {
    app(snapshot::demo(t0()), t0())
}

fn empty() -> App {
    app(session::Snapshot::new(&session::Config::default()), t0())
}

fn draw(app: &App, w: u16, h: u16) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| view::render(f, app)).unwrap();
    t.backend().buffer().clone()
}

/// The screen as text rows.
fn screen(app: &App, w: u16, h: u16) -> Vec<String> {
    let buf = draw(app, w, h);
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn text(app: &App) -> String {
    screen(app, 80, 24).join("\n")
}

/// The row containing `needle`, and whether its first cell there is dim.
fn dim_at(app: &App, needle: &str) -> bool {
    let buf = draw(app, 80, 24);
    let rows = screen(app, 80, 24);
    for (y, row) in rows.iter().enumerate() {
        if let Some(byte) = row.find(needle) {
            let x = row[..byte].chars().count() as u16;
            return buf[(x, y as u16)].modifier.contains(Modifier::DIM);
        }
    }
    panic!("{needle:?} not on screen:\n{}", rows.join("\n"));
}

fn err(m: &str) -> ErrorView {
    ErrorView {
        kind: "bluez",
        message: m.into(),
        exit_code: 2,
    }
}

fn via(kind: TransportKind, device: &str) -> LinkState {
    LinkState::Connected {
        via: ConnectedVia {
            transport: TransportInfo {
                kind,
                device: device.into(),
            },
            auto: true,
        },
    }
}

/// One bonded slot, active, with `connected` and the output priority.
fn status(at: SystemTime, priority: &'static str, connected: bool) -> Event {
    Event::Status(Stamped {
        at,
        value: Ok(Profiles {
            max_profiles: 5,
            active_profile: Some(0),
            output_priority: Some(priority),
            profiles: vec![Profile {
                index: 0,
                active: true,
                connected,
                bonded: true,
                address: Some("00:11:22:33:44:55".into()),
                name: None,
            }],
        }),
    })
}

/// `demo()` with its update check replaced.
fn with_update(s: update::Status, latest: &str) -> App {
    let mut a = demo();
    let mut r = a.session.update.value.clone().unwrap();
    r.check.status = s;
    r.check.latest.tag = latest.into();
    a.apply(&Event::Update(Stamped::now(Ok(r))), t0());
    a
}

#[test]
fn dashboard_80x24() {
    let expected = [
        "┌─ sofleii · Eyelash Sofle ──────────────────────── ● usb /dev/ttyACM1 (auto) ─┐",
        "│ Transport  USB /dev/ttyACM1 (auto)           Output     USB  (priority usb)  │",
        "│ Profile    0 \"desk\"  connected               Studio     unlocked             │",
        "│ Battery    L ████████░░  82%   R ██████░░░░  61%                read 12s ago │",
        "│ Firmware   v3.1.0 (f9a665f) · up to date with v3.1.0          checked 2m ago │",
        "│ Split      connected                                                         │",
        "├─ Profiles ───────────────────────────────────────────────────────────────────┤",
        "│ ▶ 0  desk     00:11:22:33:44:55   active · connected · bonded                │",
        "│   1  laptop   00:11:22:33:44:66   bonded                                     │",
        "│   2  —                            empty                                      │",
        "│   3  —                            empty                                      │",
        "│   4  —                            empty                                      │",
        "├─ Log (info) ─────────────────────────────────────────────────────────────────┤",
        "│ 17:41:56 connected over usb /dev/ttyACM1 (auto)                              │",
    ];
    let s = screen(&demo(), 80, 24);
    assert_eq!(&s[..expected.len()], &expected[..]);
    for row in &s[expected.len()..23] {
        assert_eq!(row, &format!("│{}│", " ".repeat(78)));
    }
    assert_eq!(
        s[23],
        "└ q quit · r refresh · ⏎ switch · o output · u update · d debug · ? help ──────┘"
    );
}

#[test]
fn dashboard_60x20_is_one_column() {
    let expected = [
        "┌─ sofleii · Eyelash Sofle ──── ● usb /dev/ttyACM1 (auto) ─┐",
        "│ Transport  USB /dev/ttyACM1 (auto)                       │",
        "│ Output     USB  (priority usb)                           │",
        "│ Profile    0 \"desk\"  connected                           │",
        "│ Studio     unlocked                                      │",
        "│ Battery    L ████░  82%   R ███░░  61%      read 12s ago │",
        "│ Firmware   v3.1.0 (f9a665f) · up to date with v3.1.0     │",
        "│ Split      connected                                     │",
        "├─ Profiles ───────────────────────────────────────────────┤",
        "│ ▶ 0  desk     active · connected · bonded                │",
        "│   1  laptop   bonded                                     │",
        "│   2  —        empty                                      │",
        "│   3  —        empty                                      │",
        "│   4  —        empty                                      │",
        "├─ Log (info) ─────────────────────────────────────────────┤",
        "│ 17:41:56 connected over usb /dev/ttyACM1 (auto)          │",
        "│                                                          │",
        "│                                                          │",
        "│                                                          │",
        "└ q quit · r refresh · ⏎ switch · o output · ? help ───────┘",
    ];
    assert_eq!(screen(&demo(), 60, 20), expected);
}

/// `demo()` with the split read replaced (`None`: the empty reply of
/// firmware without the split-info fix), and the right battery at the 0%
/// the central's proxy keeps for a gone peripheral.
fn with_split(connected: Option<bool>) -> App {
    let mut a = demo();
    let mut b = a.session.battery.value.clone().unwrap();
    b.right.percent = Some(0);
    a.apply(&Event::Battery(Stamped::now(Ok(b))), t0());
    let split = crate::commands::split::Split {
        known: connected.is_some(),
        connected,
        is_split: connected.map(|_| true),
        is_central: connected.map(|_| true),
    };
    a.apply(&Event::Split(Stamped::now(Ok(split))), t0());
    a
}

/// The rows starting with `│ <label>`.
fn rows_of(s: &[String], label: &str) -> String {
    let start = format!("│ {label}");
    s.iter().find(|r| r.starts_with(&start)).cloned().unwrap()
}

#[test]
fn split_states_render() {
    let s = screen(&with_split(Some(false)), 80, 24);
    assert_eq!(
        rows_of(&s, "Split"),
        "│ Split      disconnected (right half off or out of range)                     │"
    );
    // The stale 0% is not shown as a level.
    let battery = rows_of(&s, "Battery");
    assert!(
        battery.starts_with("│ Battery    L ████████░░  82%   R disconnected "),
        "{battery}"
    );
    assert!(!battery.contains("0%"), "{battery}");

    let s = screen(&with_split(None), 80, 24);
    assert_eq!(
        rows_of(&s, "Split"),
        "│ Split      unknown (firmware without the split-info fix)                     │"
    );
    // Unknown keeps the reading as it came.
    assert!(rows_of(&s, "Battery").contains("R ░░░░░░░░░░   0%"));

    let s = screen(&with_split(Some(true)), 80, 24);
    assert!(rows_of(&s, "Split").starts_with("│ Split      connected  "));
    assert!(rows_of(&s, "Battery").contains("   0%"));

    // Narrow: the short forms.
    let s = screen(&with_split(Some(false)), 60, 20);
    assert!(
        rows_of(&s, "Split").starts_with("│ Split      disconnected (right half off)  "),
        "{s:#?}"
    );
    let s = screen(&with_split(None), 60, 20);
    assert!(rows_of(&s, "Split").starts_with("│ Split      unknown (old firmware)  "));
}

#[test]
fn right_zero_after_rejoining_reads_as_reading() {
    // Off, then on again, then the proxy's 0% before the real level.
    let mut a = with_split(Some(false));
    let split = |c| crate::commands::split::Split {
        known: true,
        connected: Some(c),
        is_split: Some(true),
        is_central: Some(true),
    };
    a.apply(
        &Event::Split(Stamped {
            at: t0(),
            value: Ok(split(true)),
        }),
        t0(),
    );
    let battery = |a: &mut App, now| {
        a.tick(now);
        rows_of(&screen(a, 80, 24), "Battery")
    };
    let row = battery(&mut a, t0());
    assert!(
        row.starts_with("│ Battery    L ████████░░  82%   R reading… "),
        "{row}"
    );
    // Past the 10 s window a persisting 0 is real.
    let row = battery(&mut a, at(11));
    assert!(row.contains("R ░░░░░░░░░░   0%"), "{row}");
    // A real level ends the window early.
    let mut b = a.session.battery.value.clone().unwrap();
    b.right.percent = Some(66);
    a.apply(
        &Event::Battery(Stamped {
            at: at(3),
            value: Ok(b),
        }),
        at(3),
    );
    let row = battery(&mut a, at(3));
    assert!(row.contains("R ███████░░░  66%"), "{row}");
}

#[test]
fn split_read_error_is_a_note() {
    let mut a = demo();
    a.apply(
        &Event::Split(Stamped::now(Err(err("split read failed")))),
        t0(),
    );
    let row = rows_of(&screen(&a, 80, 24), "Split");
    assert!(
        row.contains("connected") && row.contains("! split read failed"),
        "{row}"
    );
}

#[test]
fn searching_state() {
    let app = empty();
    let s = screen(&app, 80, 24);
    assert!(s[0].ends_with(" ○ searching ─┐"), "{}", s[0]);
    assert!(s[1].contains("Status     Searching…"), "{}", s[1]);
    assert!(
        s[2].contains("USB serial \"Eyelash Sofle\", then BLE"),
        "{}",
        s[2]
    );
    assert!(s[3].starts_with("├─ Profiles ─"), "{}", s[3]);
    assert!(s[4].starts_with("│ —  "), "{}", s[4]);
    assert!(s[5].starts_with("├─ Log (info) ─"), "{}", s[5]);
    assert!(s[6].starts_with("│ nothing yet  "), "{}", s[6]);
    // Nothing to select, and moving doesn't panic.
    let mut app = app;
    app.on_key(key(KeyCode::Down));
    assert_eq!(app.selected, 0);
}

#[test]
fn connecting_state() {
    let mut app = empty();
    app.apply(
        &Event::Link(LinkState::Connecting {
            kind: Some(TransportKind::Usb),
        }),
        t0(),
    );
    let s = screen(&app, 80, 24);
    assert!(s[0].ends_with(" ◌ usb connecting ─┐"), "{}", s[0]);
    assert!(s[1].contains("Status     Connecting over USB…"), "{}", s[1]);
}

#[test]
fn deaf_state_shows_the_hint() {
    let mut app = empty();
    app.apply(
        &Event::Link(LinkState::Deaf {
            kind: Some(TransportKind::Ble),
            hint: Some(crate::transport::ble::BLE_TIMEOUT_HINT.into()),
        }),
        t0(),
    );
    let s = screen(&app, 80, 24);
    assert!(s[0].ends_with(" ◐ ble not answering ─┐"), "{}", s[0]);
    assert!(
        s[1].contains("Status     BLE open, but Studio doesn't answer"),
        "{}",
        s[1]
    );
    assert!(
        s[2].contains("over BLE, ZMK Studio only answers on the selected output:"),
        "{}",
        s[2]
    );
    // Three wrapped lines at most; the log has the rest.
    assert!(s[5].starts_with("├─ Profiles ─"), "{}", s[5]);
}

#[test]
fn lost_counts_the_retry_down() {
    let mut app = empty();
    app.apply(
        &Event::Link(LinkState::Lost {
            reason: "serial: broken pipe".into(),
            retry_in: Duration::from_secs(4),
        }),
        t0(),
    );
    app.tick(t0() + Duration::from_millis(1500));
    let s = screen(&app, 80, 24);
    assert!(s[0].ends_with(" ✕ lost, retrying in 3s ─┐"), "{}", s[0]);
    assert!(s[1].contains("Status     Lost; retrying in 3s"), "{}", s[1]);
    assert!(s[2].contains("serial: broken pipe"), "{}", s[2]);
    app.tick(at(10));
    assert!(text(&app).contains("retrying in 0s"));
}

#[test]
fn lost_after_connected_keeps_the_last_values() {
    let mut app = demo();
    app.apply(
        &Event::Link(LinkState::Lost {
            reason: "serial: broken pipe".into(),
            retry_in: Duration::from_secs(2),
        }),
        t0(),
    );
    let s = screen(&app, 80, 24);
    assert!(s[0].ends_with(" ✕ lost, retrying in 2s ─┐"), "{}", s[0]);
    assert!(s[1].contains("Transport  lost, retry in 2s"), "{}", s[1]);
    // Over no link, the output is unknown; the profiles stay.
    assert!(s[1].contains("Output     —  (priority usb)"), "{}", s[1]);
    assert!(s[3].contains(" 82%"), "{}", s[3]);
    assert!(s[7].starts_with("│ ▶ 0  desk "), "{}", s[7]);
}

#[test]
fn stale_values_are_dimmed() {
    let mut app = demo();
    assert!(!dim_at(&app, "████████░░"));
    assert!(!dim_at(&app, "USB  (priority usb)"));
    // Battery every 60 s: stale after 3 minutes; the status (5 s) long before.
    app.tick(at(200));
    assert!(dim_at(&app, "████████░░"));
    assert!(dim_at(&app, "USB  (priority usb)"));
    let s = screen(&app, 80, 24);
    assert!(s[3].ends_with("read 3m ago │"), "{}", s[3]);
    // The status's age goes on the Profiles separator once it's stale.
    assert!(s[6].ends_with(" read 3m ago ─┤"), "{}", s[6]);
}

#[test]
fn a_field_error_keeps_the_last_good_value() {
    let mut app = demo();
    app.apply(
        &Event::Battery(Stamped {
            at: t0(),
            value: Err(err("BlueZ not running")),
        }),
        t0(),
    );
    let s = screen(&app, 80, 24);
    assert!(s[3].contains(" 82%"), "{}", s[3]);
    assert!(s[3].ends_with("! BlueZ not running │"), "{}", s[3]);
    // A long error is cut to fit.
    app.apply(
        &Event::Status(Stamped {
            at: t0(),
            value: Err(err(
                "timed out after 3000 ms waiting for a response; is Studio listening on this \
                 endpoint?",
            )),
        }),
        t0(),
    );
    let s = screen(&app, 80, 24);
    assert!(
        s[6].starts_with("├─ Profiles ── ! timed out after"),
        "{}",
        s[6]
    );
    assert!(s[6].ends_with("… ─┤"), "{}", s[6]);
    assert!(s[7].starts_with("│ ▶ 0  desk "), "{}", s[7]);
    // A good read clears it.
    app.apply(&status(t0(), "usb", true), t0());
    assert!(screen(&app, 80, 24)[6].starts_with("├─ Profiles ────"));
}

#[test]
fn update_states() {
    let row = |a: &App| screen(a, 80, 24)[4].clone();
    assert!(row(&demo()).contains("up to date with v3.1.0"));
    let r = row(&with_update(update::Status::Behind, "v3.2.0"));
    assert!(r.contains("· update: v3.2.0"), "{r}");
    assert!(r.contains("checked 2m ago"), "{r}");
    let r = row(&with_update(update::Status::Ahead, "v3.1.0"));
    assert!(r.contains("· ahead of v3.1.0"), "{r}");
    let r = row(&with_update(update::Status::Unknown, "v3.1.0"));
    assert!(r.contains("· unknown build (v3.1.0)"), "{r}");
    assert!(!r.contains("sofleii-v"), "{r}");

    // A newer sofleii release is named after the firmware verdict.
    let mut app = demo();
    let mut u = app.session.update.value.clone().unwrap();
    u.cli = update::cli_check(
        "0.1.0",
        Some(&update::Listed {
            tag: "sofleii-v0.2.0".into(),
            published_at: None,
        }),
    );
    app.apply(&Event::Update(Stamped::now(Ok(u))), t0());
    let r = screen(&app, 120, 24)[4].clone();
    assert!(
        r.contains("up to date with v3.1.0 · sofleii v0.2.0 available"),
        "{r}"
    );

    // Never checked: the firmware's own version.
    let mut app = demo();
    app.session.update = Default::default();
    let r = row(&app);
    assert!(r.contains("Firmware   f9a665f · not checked yet"), "{r}");
    // A failed check names its error, keeping the last result.
    let mut app = demo();
    app.apply(
        &Event::Update(Stamped {
            at: t0(),
            value: Err(err("GitHub rate limit")),
        }),
        t0(),
    );
    let r = row(&app);
    assert!(r.contains("up to date with v3.1.0"), "{r}");
    assert!(r.ends_with("! GitHub rate limit │"), "{r}");
    // Stale after 3 x 6 h, dimmed.
    app.tick(at(19 * 3600));
    assert!(dim_at(&app, "up to date with"));
}

#[test]
fn usb_on_the_right_half_only_charges() {
    let mut app = empty();
    app.apply(
        &Event::Link(via(TransportKind::Ble, "00:11:22:33:44:55")),
        t0(),
    );
    app.apply(&status(t0(), "usb", true), t0());
    let s = screen(&app, 80, 24);
    assert!(
        s[0].ends_with(" ● ble 00:11:22:33:44:55 (auto) ─┐"),
        "{}",
        s[0]
    );
    assert!(s[1].contains("Output     BLE  (priority usb)"), "{}", s[1]);
    assert!(s[3].contains(view::USB_LEFT_ONLY), "{}", s[3]);
    let narrow = screen(&app, 60, 20).join("\n");
    assert!(
        narrow.contains("no USB on the left half (right only charges)"),
        "{narrow}"
    );
    // Over USB, or with priority ble, there's nothing to explain.
    for (kind, priority) in [(TransportKind::Usb, "usb"), (TransportKind::Ble, "ble")] {
        let mut app = empty();
        app.apply(&Event::Link(via(kind, "x")), t0());
        app.apply(&status(t0(), priority, true), t0());
        assert!(!text(&app).contains("left half"), "{kind:?} {priority}");
    }
}

#[test]
fn the_log_merges_session_and_core_lines() {
    let mut app = demo();
    app.log.push(LogLine {
        at: t0() - Duration::from_secs(20),
        level: LogLevel::Warn,
        text: "warning: two devices".into(),
    });
    app.apply(
        &Event::Log {
            at: t0(),
            level: LogLevel::Warn,
            msg: "link lost: broken pipe".into(),
        },
        t0(),
    );
    let s = screen(&app, 80, 24);
    assert!(
        s[13].starts_with("│ 17:41:50 warning: two devices"),
        "{}",
        s[13]
    );
    assert!(
        s[14].starts_with("│ 17:41:56 connected over usb"),
        "{}",
        s[14]
    );
    assert!(
        s[15].starts_with("│ 17:42:10 warning: link lost: broken pipe"),
        "{}",
        s[15]
    );
    // Debug lines are hidden by default (`Log (info)`).
    for i in 1..=3 {
        app.log.push(LogLine {
            at: at(i),
            level: LogLevel::Debug,
            text: "RPC lease released after 1.5s idle".into(),
        });
    }
    let s = screen(&app, 80, 24);
    assert!(s[12].starts_with("├─ Log (info) ─"), "{}", s[12]);
    assert!(!s.join("\n").contains("lease released"));
    // `d` shows them, a run folded into its latest, counted; `d` again hides.
    app.on_key(key(KeyCode::Char('d')));
    let s = screen(&app, 80, 24);
    assert!(s[12].starts_with("├─ Log (debug) ─"), "{}", s[12]);
    assert!(
        s[16].starts_with("│ 17:42:13 RPC lease released after 1.5s idle (×3) "),
        "{}",
        s[16]
    );
    assert!(s[17].starts_with("│     "), "{}", s[17]);
    app.on_key(key(KeyCode::Char('d')));
    assert!(!app.debug);
    assert!(screen(&app, 80, 24)[12].starts_with("├─ Log (info) ─"));
}

#[test]
fn only_debug_lines_means_nothing_yet_at_info() {
    let mut app = empty();
    app.log.push(LogLine {
        at: t0(),
        level: LogLevel::Debug,
        text: "presence changed".into(),
    });
    assert!(screen(&app, 80, 24)[6].starts_with("│ nothing yet  "));
    app.debug = true;
    assert!(screen(&app, 80, 24)[6].contains("presence changed"));
}

#[test]
fn help_overlay() {
    let mut app = demo();
    app.on_key(key(KeyCode::Char('?')));
    assert!(app.help);
    let s = text(&app);
    assert!(s.contains("┌─ Help ─"), "{s}");
    for k in [
        "q  Ctrl-C",
        "↑ ↓  k j",
        "refresh everything now",
        "debug lines in the log",
        "switch to the selected profile",
        "toggle the output priority usb / ble",
        "risky switches ask first; only y confirms",
        "? or Esc closes",
    ] {
        assert!(s.contains(k), "{k}:\n{s}");
    }
    // Esc closes the overlay first, then quits.
    app.on_key(key(KeyCode::Esc));
    assert!(!app.help && !app.quit);
    app.on_key(key(KeyCode::Esc));
    assert!(app.quit);
}

#[test]
fn no_color_means_no_colors() {
    let colored = |app: &App| {
        let buf = draw(app, 80, 24);
        buf.content()
            .iter()
            .any(|c| c.fg != Color::Reset || c.bg != Color::Reset)
    };
    assert!(!colored(&demo()));
    let mut c = demo();
    c.color = true;
    assert!(colored(&c));
}

#[test]
fn battery_bars_and_ages() {
    assert_eq!(view::bar(82, 10), "████████░░");
    assert_eq!(view::bar(0, 5), "░░░░░");
    assert_eq!(view::bar(100, 5), "█████");
    assert_eq!(view::ago(Duration::from_secs(12)), "12s ago");
    assert_eq!(view::ago(Duration::from_secs(150)), "2m ago");
    assert_eq!(view::ago(Duration::from_secs(7200)), "2h ago");
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn keys_select_and_quit() {
    let mut app = demo();
    assert_eq!(app.selected, 0, "starts on the active profile");
    app.on_key(key(KeyCode::Char('j')));
    app.on_key(key(KeyCode::Down));
    assert_eq!(app.selected, 2);
    for _ in 0..10 {
        app.on_key(key(KeyCode::Down));
    }
    assert_eq!(app.selected, 4, "stops at the last slot");
    app.on_key(key(KeyCode::Char('k')));
    app.on_key(key(KeyCode::Up));
    assert_eq!(app.selected, 2);
    let s = screen(&app, 80, 24);
    assert!(s[9].starts_with("│ ▶ 2  "), "{}", s[9]);

    for quit in [
        key(KeyCode::Char('q')),
        key(KeyCode::Esc),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let mut app = demo();
        app.on_key(quit);
        assert!(app.quit, "{quit:?}");
    }
    // Plain `c` is not Ctrl-C.
    let mut app = demo();
    app.on_key(key(KeyCode::Char('c')));
    assert!(!app.quit);
}

#[test]
fn the_selection_follows_the_active_slot_until_moved() {
    let mut app = empty();
    let two = |active: u32| {
        let mut e = status(t0(), "usb", true);
        if let Event::Status(Stamped { value: Ok(p), .. }) = &mut e {
            p.active_profile = Some(active);
            p.profiles = (0..2)
                .map(|i| Profile {
                    index: i,
                    active: i == active,
                    ..p.profiles[0].clone()
                })
                .collect();
        }
        e
    };
    app.apply(&two(1), t0());
    assert_eq!(app.selected, 1);
    app.apply(&two(0), t0());
    assert_eq!(app.selected, 0);
    app.on_key(key(KeyCode::Down));
    app.apply(&two(0), t0());
    assert_eq!(app.selected, 1, "moved by hand: stays");
}

/// Records what the loop sends.
#[derive(Default)]
struct Rec(RefCell<Vec<Request>>);

impl Requests for Rec {
    fn send(&self, req: Request) -> Ticket {
        let mut v = self.0.borrow_mut();
        v.push(req);
        Ticket(v.len() as u64)
    }
}

#[test]
fn the_loop_sends_requests_and_quits() {
    let (tx, rx) = mpsc::channel();
    let rec = Rec::default();
    let stop = AtomicBool::new(false);
    let log = log_pane::Buffer::default();
    let mut app = empty();
    for input in [
        Input::Term(TermEvent::Key(key(KeyCode::Char('r')))),
        Input::Term(TermEvent::Key(key(KeyCode::Char('u')))),
        Input::Term(TermEvent::FocusLost),
        Input::Term(TermEvent::FocusGained),
        Input::Session(Box::new(Event::Link(via(
            TransportKind::Usb,
            "/dev/ttyACM1",
        )))),
        // The update check (ticket 2) failed.
        Input::Session(Box::new(Event::Done {
            ticket: Ticket(2),
            result: Err(err("GitHub rate limit")),
        })),
        Input::Term(TermEvent::Key(key(KeyCode::Char('q')))),
        // Never read: `q` came first.
        Input::Term(TermEvent::Key(key(KeyCode::Char('r')))),
    ] {
        tx.send(input).unwrap();
    }
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut frames = 0;
    event_loop(&mut app, &rx, &rec, &log, &stop, |a| {
        frames += 1;
        term.draw(|f| view::render(f, a))
            .map(drop)
            .map_err(io::Error::other)
    })
    .unwrap();
    assert_eq!(
        *rec.0.borrow(),
        [
            Request::Refresh(Scope::All),
            Request::CheckUpdate,
            Request::Focus(false),
            Request::Focus(true),
            Request::Quit,
        ]
    );
    assert!(frames >= 1);
    assert!(app.quit && app.session.focused);
    assert_eq!(app.session.link.kind(), Some(TransportKind::Usb));
    let s = text(&app);
    assert!(
        s.contains("warning: update check: GitHub rate limit"),
        "{s}"
    );
}

#[test]
fn a_signal_stops_the_loop_with_quit() {
    let (_tx, rx) = mpsc::channel();
    let rec = Rec::default();
    let stop = AtomicBool::new(true);
    let mut app = empty();
    event_loop(
        &mut app,
        &rx,
        &rec,
        &log_pane::Buffer::default(),
        &stop,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(*rec.0.borrow(), [Request::Quit]);
}

// Write actions: `⏎`/`s` and `o`, the confirm modal, pending and results.

use crate::commands::{output, profile};
use crate::session::{Outcome, Presence, Risk};

/// `demo()` (USB link, priority usb, 0 active, 1 bonded, 2-4 empty) with
/// both transports present on this host.
fn actions_app() -> App {
    let mut a = demo();
    a.apply(
        &Event::Presence(Presence {
            usb: true,
            ble: true,
        }),
        t0(),
    );
    a
}

fn press(a: &mut App, rec: &Rec, code: KeyCode) {
    let now = a.now;
    a.handle(Input::Term(TermEvent::Key(key(code))), rec, now);
}

fn sent(rec: &Rec) -> Vec<Request> {
    rec.0.borrow().clone()
}

fn output_done(target: &'static str, over: TransportKind) -> Outcome {
    Outcome::Output(output::Outcome {
        action: "output",
        output_priority: target,
        previous_output_priority: Some("usb"),
        ok: true,
        reply: false,
        verified: true,
        verified_over: Some(over),
        studio_transport: Some(over),
        note: Some("no reply over usb (expected); read back over ble".into()),
    })
}

fn profile_done(n: u32, over: TransportKind) -> Outcome {
    Outcome::Profile(profile::Outcome {
        action: "profile_switch",
        profile: n,
        name: None,
        ok: true,
        sent: true,
        reply: true,
        verified: true,
        verified_over: Some(over),
        previous_active_profile: Some(0),
        active_profile: Some(n),
        slot: None,
        studio_transport: Some(over),
        note: None,
    })
}

#[test]
fn enter_on_a_safe_target_sends_without_a_modal() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    // Over USB with priority usb, a bonded slot keeps the link.
    assert_eq!(sent(&rec), [Request::SwitchProfile(1)]);
    assert!(a.confirm.is_none());
    assert_eq!(a.action.as_ref().unwrap().ticket, Ticket(1));
    // `s` is the same key.
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Char('s'));
    assert_eq!(sent(&rec), [Request::SwitchProfile(1)]);
}

#[test]
fn enter_on_the_active_profile_does_nothing() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Enter);
    assert!(sent(&rec).is_empty() && a.confirm.is_none() && a.action.is_none());
    let s = screen(&a, 80, 24);
    assert!(
        s[23].starts_with("└ ✓ profile 0 is already active ─"),
        "{}",
        s[23]
    );
    assert!(
        s.join("\n")
            .contains("17:42:10 profile 0 is already active")
    );
}

#[test]
fn a_risky_target_opens_the_modal_and_y_sends() {
    for yes in [KeyCode::Char('y'), KeyCode::Char('Y')] {
        let rec = Rec::default();
        let mut a = actions_app();
        press(&mut a, &rec, KeyCode::Down);
        press(&mut a, &rec, KeyCode::Down);
        press(&mut a, &rec, KeyCode::Enter);
        // Slot 2 is empty: it starts pairing, so it asks.
        assert!(sent(&rec).is_empty());
        let c = a.confirm.clone().expect("a modal");
        assert_eq!(c.risk, Risk::DropsCurrentLink);
        assert_eq!(c.req, Request::SwitchProfile(2));
        press(&mut a, &rec, yes);
        assert_eq!(sent(&rec), [Request::SwitchProfile(2)], "{yes:?}");
        assert!(a.confirm.is_none() && a.action.is_some());
    }
}

#[test]
fn a_double_enter_on_a_risky_profile_sends_nothing() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    assert!(a.confirm.is_some());
    press(&mut a, &rec, KeyCode::Enter);
    assert!(sent(&rec).is_empty());
    assert!(a.confirm.is_none() && a.action.is_none());
}

#[test]
fn n_esc_or_any_other_key_cancels_the_modal() {
    // `⏎` too: a double Enter on a profile must never confirm.
    for no in [
        KeyCode::Char('n'),
        KeyCode::Esc,
        KeyCode::Enter,
        KeyCode::Char('j'),
        KeyCode::Char('q'),
    ] {
        let rec = Rec::default();
        let mut a = actions_app();
        press(&mut a, &rec, KeyCode::Char('o'));
        assert!(a.confirm.is_some(), "USB link to BLE asks");
        press(&mut a, &rec, no);
        assert!(sent(&rec).is_empty(), "{no:?}");
        assert!(
            a.confirm.is_none() && a.action.is_none() && !a.quit,
            "{no:?}"
        );
        let s = screen(&a, 80, 24);
        assert!(
            s[23].starts_with("└ ✓ cancelled: switching output to BLE ─"),
            "{}",
            s[23]
        );
    }
}

#[test]
fn o_toggles_the_output_priority() {
    // Over USB, BLE is the other one: confirmed, then sent.
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Char('o'));
    press(&mut a, &rec, KeyCode::Char('y'));
    assert_eq!(sent(&rec), [Request::SetOutput(TransportKind::Ble)]);
    // Over BLE with priority usb (no USB on the left half), `o` asks for
    // BLE, which keeps the link: no modal.
    let ble = || {
        let mut a = empty();
        a.apply(
            &Event::Link(via(TransportKind::Ble, "DD:1F:23:76:6E:71")),
            t0(),
        );
        a
    };
    let rec = Rec::default();
    let mut a = ble();
    a.apply(&status(t0(), "usb", true), t0());
    press(&mut a, &rec, KeyCode::Char('o'));
    assert!(a.confirm.is_none());
    assert_eq!(sent(&rec), [Request::SetOutput(TransportKind::Ble)]);
    // And back: priority ble over BLE, `o` asks for USB, which moves it.
    let rec = Rec::default();
    let mut a = ble();
    a.apply(&status(t0(), "ble", true), t0());
    press(&mut a, &rec, KeyCode::Char('o'));
    assert_eq!(
        a.confirm.as_ref().unwrap().req,
        Request::SetOutput(TransportKind::Usb)
    );
    // Not read yet: nothing to toggle.
    let rec = Rec::default();
    let mut a = empty();
    press(&mut a, &rec, KeyCode::Char('o'));
    assert!(sent(&rec).is_empty() && a.confirm.is_none());
}

#[test]
fn action_keys_are_ignored_while_pending() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    for k in [KeyCode::Enter, KeyCode::Char('s'), KeyCode::Char('o')] {
        press(&mut a, &rec, k);
    }
    assert!(a.confirm.is_none());
    assert_eq!(sent(&rec), [Request::SwitchProfile(1)]);
    // Refresh and navigation still work, and the spinner stays.
    press(&mut a, &rec, KeyCode::Char('r'));
    press(&mut a, &rec, KeyCode::Up);
    assert_eq!(sent(&rec)[1], Request::Refresh(Scope::All));
    assert_eq!(a.selected, 0);
    assert!(a.action.is_some());
}

#[test]
fn done_clears_pending_and_shows_the_result() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    // Another request's Done leaves it pending.
    a.apply(
        &Event::Done {
            ticket: Ticket(9),
            result: Ok(Outcome::Ok),
        },
        t0(),
    );
    assert!(a.action.is_some());
    a.apply(
        &Event::Done {
            ticket: Ticket(1),
            result: Ok(profile_done(1, TransportKind::Usb)),
        },
        t0(),
    );
    assert!(a.action.is_none());
    let s = screen(&a, 80, 24);
    assert!(
        s[23].starts_with("└ ✓ profile 1 active (verified over usb) ─"),
        "{}",
        s[23]
    );
    assert!(
        s.join("\n")
            .contains("17:42:10 profile 1 active (verified over usb)")
    );
    // A success fades on its own, and action keys work again.
    a.tick(at(6));
    assert!(a.flash.is_none());
    assert!(screen(&a, 80, 24)[23].starts_with("└ q quit"));
    press(&mut a, &rec, KeyCode::Char('o'));
    assert!(a.confirm.is_some());
}

#[test]
fn a_failure_stays_until_the_next_key() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Char('o'));
    press(&mut a, &rec, KeyCode::Char('y'));
    a.apply(
        &Event::Done {
            ticket: Ticket(1),
            result: Err(ErrorView {
                kind: "protocol",
                message: "the priority reads back as usb".into(),
                exit_code: 4,
            }),
        },
        t0(),
    );
    assert!(a.action.is_none());
    a.tick(at(60));
    let s = screen(&a, 80, 24);
    assert_eq!(
        s[23],
        "└ ✕ switching output to BLE failed: the priority reads back as usb ────────────┘"
    );
    assert!(
        s.join("\n")
            .contains("error: switching output to BLE failed: the priority reads back")
    );
    press(&mut a, &rec, KeyCode::Down);
    assert!(screen(&a, 80, 24)[23].starts_with("└ q quit"));
}

#[test]
fn an_output_success_logs_its_note() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Char('o'));
    press(&mut a, &rec, KeyCode::Char('y'));
    a.apply(
        &Event::Done {
            ticket: Ticket(1),
            result: Ok(output_done("ble", TransportKind::Ble)),
        },
        t0(),
    );
    let s = screen(&a, 80, 24).join("\n");
    assert!(
        s.contains("✓ output priority ble (verified over ble)"),
        "{s}"
    );
    assert!(
        s.contains("17:42:10 no reply over usb (expected); read back over ble"),
        "{s}"
    );
}

#[test]
fn pending_renders_a_spinner() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    let s = screen(&a, 80, 24);
    assert_eq!(
        s[23],
        "└ ⠋ switching to profile 1… ───────────────────────────────────────────────────┘"
    );
    // One frame per tick.
    a.tick(t0() + TICK * 2);
    assert!(screen(&a, 80, 24)[23].starts_with("└ ⠹ switching"));
}

/// The modal's rows, cut out of the 80x24 screen (60 wide at column 10).
fn modal(a: &App) -> Vec<String> {
    screen(a, 80, 24)
        .into_iter()
        .map(|r| r.chars().skip(10).take(60).collect::<String>())
        .filter(|r| r.starts_with(['┌', '│', '└']))
        .collect()
}

#[test]
fn modal_renders_for_a_link_drop() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Char('o'));
    assert_eq!(
        modal(&a),
        [
            "┌─ Switch output? ─────────────────────────────────────────┐",
            "│                                                          │",
            "│ Switching output to BLE: this USB link goes quiet;       │",
            "│ sofleii continues over BLE (verified by read-back).      │",
            "│                                                          │",
            "│ Continue?                                                │",
            "└ y confirm · n / Esc cancel ──────────────────────────────┘",
        ]
    );
}

#[test]
fn modal_renders_for_an_empty_slot() {
    let rec = Rec::default();
    let mut a = actions_app();
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    assert_eq!(
        modal(&a),
        [
            "┌─ Switch profile? ────────────────────────────────────────┐",
            "│                                                          │",
            "│ Profile 2 is empty: switching starts pairing on it and   │",
            "│ disconnects this host's BLE link; sofleii stays on USB.  │",
            "│                                                          │",
            "│ Continue?                                                │",
            "└ y confirm · n / Esc cancel ──────────────────────────────┘",
        ]
    );
}

#[test]
fn modal_renders_for_a_switch_over_ble() {
    let rec = Rec::default();
    let mut a = actions_app();
    a.apply(
        &Event::Link(via(TransportKind::Ble, "DD:1F:23:76:6E:71")),
        t0(),
    );
    press(&mut a, &rec, KeyCode::Down);
    press(&mut a, &rec, KeyCode::Enter);
    assert_eq!(
        modal(&a),
        [
            "┌─ Switch profile? ────────────────────────────────────────┐",
            "│                                                          │",
            "│ Switching to profile 1 drops this host's BLE connection; │",
            "│ sofleii continues over USB (verified by read-back).      │",
            "│                                                          │",
            "│ Continue?                                                │",
            "└ y confirm · n / Esc cancel ──────────────────────────────┘",
        ]
    );
}

#[test]
fn modal_renders_destructive() {
    let mut a = actions_app();
    a.confirm = actions::confirm(&Request::UnpairProfile(1), &a.session);
    assert_eq!(a.confirm.as_ref().unwrap().risk, Risk::Destructive);
    assert_eq!(
        modal(&a),
        [
            "┌─ Unpair profile? ────────────────────────────────────────┐",
            "│                                                          │",
            "│ Unpairing profile 1 deletes its bond: that host has to   │",
            "│ pair again. This can't be undone.                        │",
            "│                                                          │",
            "│ Continue?                                                │",
            "└ y confirm · n / Esc cancel ──────────────────────────────┘",
        ]
    );
}

/// `actions_app()` with profile 1 "laptop" active (another host's),
/// `connected` or not, and profile 0 "desk" this host's (by adapter
/// address).
fn laptop_active(connected: bool) -> App {
    let mut a = actions_app();
    let p = a.session.status.value.as_mut().unwrap();
    p.active_profile = Some(1);
    for s in &mut p.profiles {
        s.active = s.index == 1;
    }
    p.profiles[1].connected = connected;
    a.apply(&Event::Adapters(vec!["DD:1F:23:76:6E:71".into()]), t0());
    a
}

#[test]
fn modal_renders_for_output_ble_with_the_active_profile_unconnected() {
    let rec = Rec::default();
    let mut a = laptop_active(false);
    press(&mut a, &rec, KeyCode::Char('o'));
    assert_eq!(a.confirm.as_ref().unwrap().risk, Risk::DropsCurrentLink);
    assert_eq!(
        modal(&a),
        [
            "┌─ Switch output? ─────────────────────────────────────────┐",
            "│                                                          │",
            "│ Output priority becomes BLE, but the active profile 1    │",
            "│ \"laptop\" isn't connected, so the keyboard keeps using    │",
            "│ USB until a BLE host connects; sofleii stays on USB.     │",
            "│ Once profile 1 \"laptop\"'s host connects, keystrokes and  │",
            "│ Studio move to it.                                       │",
            "│                                                          │",
            "│ Continue?                                                │",
            "└ y confirm · n / Esc cancel ──────────────────────────────┘",
        ]
    );
    press(&mut a, &rec, KeyCode::Char('n'));
    assert!(sent(&rec).is_empty());
}

#[test]
fn modal_renders_for_output_ble_to_another_hosts_profile() {
    let rec = Rec::default();
    let mut a = laptop_active(true);
    press(&mut a, &rec, KeyCode::Char('o'));
    assert_eq!(a.confirm.as_ref().unwrap().risk, Risk::Destructive);
    assert_eq!(
        modal(&a),
        [
            "┌─ Switch output? ─────────────────────────────────────────┐",
            "│                                                          │",
            "│ The active profile 1 \"laptop\" is connected to another    │",
            "│ host: keystrokes and Studio move there. This USB link    │",
            "│ goes quiet and this host isn't the active BLE profile,   │",
            "│ so sofleii loses the keyboard until the output is        │",
            "│ switched back on the keyboard or from that host.         │",
            "│                                                          │",
            "│ Continue?                                                │",
            "└ y confirm · n / Esc cancel ──────────────────────────────┘",
        ]
    );
    press(&mut a, &rec, KeyCode::Char('y'));
    assert_eq!(sent(&rec), [Request::SetOutput(TransportKind::Ble)]);
}

#[test]
fn output_ble_that_stays_on_usb_says_so() {
    let rec = Rec::default();
    let mut a = laptop_active(false);
    press(&mut a, &rec, KeyCode::Char('o'));
    press(&mut a, &rec, KeyCode::Char('y'));
    let Outcome::Output(mut o) = output_done("ble", TransportKind::Usb) else {
        unreachable!()
    };
    o.reply = true;
    o.note = Some(
        "priority is ble, but the active BLE profile isn't connected, so keystrokes and \
         Studio stay on usb"
            .into(),
    );
    a.apply(
        &Event::Done {
            ticket: Ticket(1),
            result: Ok(Outcome::Output(o)),
        },
        t0(),
    );
    let s = screen(&a, 80, 24);
    assert!(
        s[23].starts_with("└ ✓ output priority ble; still on USB until a BLE host connects ─"),
        "{}",
        s[23]
    );
    assert!(
        s.join("\n")
            .contains("17:42:10 priority is ble, but the active BLE profile")
    );
}
