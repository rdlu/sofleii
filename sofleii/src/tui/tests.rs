//! Render tests on ratatui's `TestBackend`, plus key handling.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

use super::*;

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

fn demo() -> App {
    App::new(Snapshot::demo(), false)
}

#[test]
fn dashboard_80x24() {
    let expected = [
        "┌─ sofleii · Eyelash Sofle ─────────────────────────────── ● usb /dev/ttyACM1 ─┐",
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
        "├─ Log ────────────────────────────────────────────────────────────────────────┤",
        "│ 17:41:58 connected over USB /dev/ttyACM1                                     │",
        "│ 17:42:10 USB went quiet (output BLE?), switched to BLE DD:1F:23:76:6E:71     │",
    ];
    let s = screen(&demo(), 80, 24);
    assert_eq!(&s[..expected.len()], &expected[..]);
    for row in &s[expected.len()..23] {
        assert_eq!(row, &format!("│{}│", " ".repeat(78)));
    }
    assert_eq!(
        s[23],
        "└ q quit · r refresh · ↑↓/jk select · ⏎ switch · o output · u update · ? help ─┘"
    );
}

#[test]
fn dashboard_60x20_is_one_column() {
    let expected = [
        "┌─ sofleii · Eyelash Sofle ─────────── ● usb /dev/ttyACM1 ─┐",
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
        "├─ Log ────────────────────────────────────────────────────┤",
        "│ 17:41:58 connected over USB /dev/ttyACM1                 │",
        "│ 17:42:10 USB went quiet (output BLE?), switched to BLE DD│",
        "│                                                          │",
        "│                                                          │",
        "└ q quit · ↑↓/jk select · ? help ──────────────────────────┘",
    ];
    assert_eq!(screen(&demo(), 60, 20), expected);
}

#[test]
fn searching_state() {
    let app = App::new(Snapshot::searching(), false);
    let s = screen(&app, 80, 24);
    assert!(s[0].ends_with(" ○ searching ─┐"), "{}", s[0]);
    assert!(s[1].contains("Status     Searching…"), "{}", s[1]);
    assert!(s[4].starts_with("├─ Profiles ─"), "{}", s[4]);
    assert!(s[5].starts_with("│ —  "), "{}", s[5]);
    assert!(s[6].starts_with("├─ Log ─"), "{}", s[6]);
    assert!(s[7].starts_with("│ nothing yet  "), "{}", s[7]);
    // Nothing to select, and moving doesn't panic.
    let mut app = app;
    app.on_key(key(KeyCode::Down));
    assert_eq!(app.selected, 0);
}

#[test]
fn help_overlay() {
    let mut app = demo();
    app.on_key(key(KeyCode::Char('?')));
    assert!(app.help);
    let s = screen(&app, 80, 24).join("\n");
    assert!(s.contains("┌─ Help ─"), "{s}");
    for k in ["q  Ctrl-C", "↑ ↓  k j", "(not yet)", "? or Esc closes"] {
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
    assert!(colored(&App::new(Snapshot::demo(), true)));
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
