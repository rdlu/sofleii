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
                address: Some("DD:1F:23:76:6E:71".into()),
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
        "│ Split      unavailable (get_split_info is broken upstream)                   │",
        "├─ Profiles ───────────────────────────────────────────────────────────────────┤",
        "│ ▶ 0  desk     DD:1F:23:76:6E:71   active · connected · bonded                │",
        "│   1  laptop   C8:3A:35:10:22:9E   bonded                                     │",
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
        "└ q quit · r refresh · u update check · ↑↓/jk select · d debug · ? help ───────┘"
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
        "│ Split      unavailable (broken upstream)                 │",
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
        "└ q quit · r refresh · ? help ─────────────────────────────┘",
    ];
    assert_eq!(screen(&demo(), 60, 20), expected);
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
        &Event::Link(via(TransportKind::Ble, "DD:1F:23:76:6E:71")),
        t0(),
    );
    app.apply(&status(t0(), "usb", true), t0());
    let s = screen(&app, 80, 24);
    assert!(
        s[0].ends_with(" ● ble DD:1F:23:76:6E:71 (auto) ─┐"),
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
        "(coming soon)",
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
