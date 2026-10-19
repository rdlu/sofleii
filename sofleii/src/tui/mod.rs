//! The TUI, opened by `sofleii` with no subcommand on a terminal. A front
//! end like the CLI: it reads the core, and `commands`, `client`,
//! `transport` and `session` never depend on it.
//!
//! **Live, read-only:** [`run`] spawns a [`Session`] with the CLI's
//! connection flags and folds its events into a [`session::Snapshot`]; the
//! dashboard renders a [`Snapshot`] derived from it. One loop reads one
//! channel, fed by an input thread (crossterm) and a forwarder (the
//! session's events), and redraws at least every [`TICK`] for ages and
//! staleness. `r` refreshes and `u` checks for an update; the write actions
//! (switch, output) come with the next PR. `SOFLEII_TUI_DEMO=1` shows a
//! fixture instead, with no session.
//!
//! The terminal is restored on every way out: a normal quit (`q`, `Esc`,
//! `Ctrl-C`), SIGTERM/SIGHUP/SIGINT (flags checked by the event loop), and a
//! panic: `ratatui::try_init` installs a panic hook that restores first, and
//! hooks run before the release profile's `panic = "abort"`. Quitting never
//! joins a thread: [`Request::Quit`] tells the session to stop, and the
//! process exit takes any thread still blocked in I/O.

pub mod log_pane;
pub mod snapshot;
pub mod view;

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime};

use jiff::tz::TimeZone;
use ratatui::crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event as TermEvent, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;

use crate::connect;
use crate::output::{Format, Style};
use crate::session::{self, Event, LogLevel, Request, Scope, Session, SessionHandle, Ticket};
pub use snapshot::{LogLine, Snapshot};

/// How often the loop wakes without input, for signals, ages and new log
/// lines.
pub const TICK: Duration = Duration::from_millis(250);

/// What the loop reads: the terminal's input and the session's events, on
/// one channel.
#[derive(Debug)]
pub enum Input {
    Term(TermEvent),
    /// Boxed: an event is much larger than a key.
    Session(Box<Event>),
}

/// Where the loop sends requests: a [`SessionHandle`], or a fake in tests
/// and the demo.
pub trait Requests {
    fn send(&self, req: Request) -> Ticket;
}

impl Requests for SessionHandle {
    fn send(&self, req: Request) -> Ticket {
        SessionHandle::send(self, req)
    }
}

/// The demo's session: takes every request and does nothing.
struct NoSession;

impl Requests for NoSession {
    fn send(&self, _: Request) -> Ticket {
        Ticket(0)
    }
}

/// Everything the view draws: the session's snapshot plus UI state.
#[derive(Debug, Clone)]
pub struct App {
    pub session: session::Snapshot,
    /// When the link state last changed, for the retry countdown.
    pub link_at: SystemTime,
    /// The `log` crate's lines (core diagnostics), from the pane logger.
    pub log: Vec<LogLine>,
    /// The TUI's own lines (a key's request that failed).
    pub notes: Vec<LogLine>,
    /// The time the view is drawn for; [`App::tick`] moves it.
    pub now: SystemTime,
    /// For the log pane's clock (local; UTC in tests).
    pub tz: TimeZone,
    /// Index into the profiles.
    pub selected: usize,
    /// The user moved the selection: it stops following the active slot.
    moved: bool,
    pub help: bool,
    /// The Log pane shows debug lines too (`d`); info and up otherwise.
    pub debug: bool,
    pub color: bool,
    pub quit: bool,
    /// Requests sent from a key, by ticket, to name a failure in the log.
    pending: HashMap<Ticket, &'static str>,
}

impl App {
    /// Starts with the active profile selected (and follows it until the
    /// user moves the selection).
    pub fn new(session: session::Snapshot, color: bool, now: SystemTime) -> Self {
        let mut app = Self {
            session,
            link_at: now,
            log: Vec::new(),
            notes: Vec::new(),
            now,
            tz: TimeZone::system(),
            selected: 0,
            moved: false,
            help: false,
            debug: false,
            color,
            quit: false,
            pending: HashMap::new(),
        };
        app.follow_active();
        app
    }

    /// The view at [`App::now`].
    pub fn view(&self) -> Snapshot {
        let extra: Vec<LogLine> = self.log.iter().chain(&self.notes).cloned().collect();
        Snapshot::derive(&self.session, self.link_at, self.now, &extra)
    }

    fn profiles(&self) -> &[crate::commands::profiles::Profile] {
        self.session
            .status
            .value
            .as_ref()
            .map_or(&[], |p| p.profiles.as_slice())
    }

    fn follow_active(&mut self) {
        let count = self.profiles().len();
        if !self.moved
            && let Some(i) = self.profiles().iter().position(|p| p.active)
        {
            self.selected = i;
        }
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    /// Folds one session event in, at `now`.
    pub fn apply(&mut self, e: &Event, now: SystemTime) {
        match e {
            Event::Link(l) if *l != self.session.link => self.link_at = now,
            Event::Done {
                ticket,
                result: Err(err),
            } => {
                if let Some(what) = self.pending.remove(ticket) {
                    if self.notes.len() >= log_pane::CAPACITY {
                        self.notes.remove(0);
                    }
                    self.notes.push(LogLine {
                        at: now,
                        level: LogLevel::Warn,
                        text: format!("warning: {what}: {}", err.message),
                    });
                }
            }
            Event::Done { ticket, .. } => {
                self.pending.remove(ticket);
            }
            _ => {}
        }
        self.session.apply(e);
        if matches!(e, Event::Status(_)) {
            self.follow_active();
        }
    }

    /// Moves the clock: ages, staleness and the retry countdown.
    pub fn tick(&mut self, now: SystemTime) {
        self.now = now;
        self.session.tick(now);
    }

    /// Handles a key; returns the request it asks for, if any.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Request> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        match key.code {
            _ if ctrl_c => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc | KeyCode::Char('?') if self.help => self.help = false,
            KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => return Some(Request::Refresh(Scope::All)),
            KeyCode::Char('u') => return Some(Request::CheckUpdate),
            KeyCode::Char('d') => self.debug = !self.debug,
            KeyCode::Up | KeyCode::Char('k') => {
                self.moved = true;
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.moved = true;
                self.selected = (self.selected + 1).min(self.profiles().len().saturating_sub(1));
            }
            _ => {}
        }
        None
    }

    /// One input: a key (whose request goes to `session`), a focus change,
    /// or a session event.
    pub fn handle(&mut self, input: Input, session: &dyn Requests, now: SystemTime) {
        match input {
            Input::Term(TermEvent::Key(key)) => {
                if let Some(req) = self.on_key(key) {
                    let what = match req {
                        Request::CheckUpdate => "update check",
                        _ => "refresh",
                    };
                    let t = session.send(req);
                    self.pending.insert(t, what);
                }
            }
            Input::Term(TermEvent::FocusGained) => self.focus(true, session),
            Input::Term(TermEvent::FocusLost) => self.focus(false, session),
            Input::Term(_) => {}
            Input::Session(e) => self.apply(&e, now),
        }
    }

    fn focus(&mut self, focused: bool, session: &dyn Requests) {
        // The snapshot mirrors it: the status staleness depends on it.
        self.session.focused = focused;
        session.send(Request::Focus(focused));
    }
}

/// Runs the TUI until the user quits or a signal arrives; `link` is the
/// CLI's `--transport`/`--device`/`--bt-address`/`--timeout` (already
/// checked).
pub fn run(link: connect::Config) -> io::Result<()> {
    let log = log_pane::install();
    let stop = Arc::new(AtomicBool::new(false));
    for sig in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(sig, Arc::clone(&stop))?;
    }

    let (tx, rx) = mpsc::channel();
    let now = SystemTime::now();
    let demo = std::env::var_os("SOFLEII_TUI_DEMO").is_some_and(|v| v == "1");
    let color = Style::detect(Format::Human).color;
    let (mut app, requests): (App, Box<dyn Requests>) = if demo {
        (
            App::new(snapshot::demo(now), color, now),
            Box::new(NoSession),
        )
    } else {
        let config = session::Config::default();
        let snap = session::Snapshot::new(&config);
        let (handle, events) = Session::spawn(config, link);
        forward(events, tx.clone());
        (App::new(snap, color, now), Box::new(handle))
    };

    let mut terminal = ratatui::try_init()?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableFocusChange);
        hook(info);
    }));
    // Not fatal: without it the session just never slows its polling.
    let _ = execute!(io::stdout(), EnableFocusChange);
    read_input(tx);
    let result = event_loop(&mut app, &rx, requests.as_ref(), &log, &stop, |app| {
        terminal.draw(|f| view::render(f, app)).map(drop)
    });
    let _ = execute!(io::stdout(), DisableFocusChange);
    ratatui::restore();
    result
}

/// The session's events onto the loop's channel, until either side goes.
fn forward(events: Receiver<Event>, tx: Sender<Input>) {
    std::thread::Builder::new()
        .name("sofleii-tui-events".into())
        .spawn(move || {
            for e in events {
                if tx.send(Input::Session(Box::new(e))).is_err() {
                    break;
                }
            }
        })
        .expect("spawning the event forwarder");
}

/// The terminal's input onto the loop's channel. Never joined: on quit it
/// is left blocked in `read`, and the process exit ends it.
fn read_input(tx: Sender<Input>) {
    std::thread::Builder::new()
        .name("sofleii-tui-input".into())
        .spawn(move || {
            while let Ok(e) = event::read() {
                if tx.send(Input::Term(e)).is_err() {
                    break;
                }
            }
        })
        .expect("spawning the input thread");
}

/// Draws, then waits up to [`TICK`] for input and takes everything queued
/// before drawing again. Sends [`Request::Quit`] on the way out.
pub fn event_loop(
    app: &mut App,
    rx: &Receiver<Input>,
    session: &dyn Requests,
    log: &log_pane::Buffer,
    stop: &AtomicBool,
    mut draw: impl FnMut(&App) -> io::Result<()>,
) -> io::Result<()> {
    let result = loop {
        if app.quit || stop.load(Ordering::SeqCst) {
            break Ok(());
        }
        app.log = log.lines();
        app.tick(SystemTime::now());
        if let Err(e) = draw(app) {
            break Err(e);
        }
        match rx.recv_timeout(TICK) {
            Ok(input) => {
                app.handle(input, session, SystemTime::now());
                while !app.quit
                    && let Ok(input) = rx.try_recv()
                {
                    app.handle(input, session, SystemTime::now());
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // No input thread and no session: nothing can arrive any more.
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
        }
    };
    session.send(Request::Quit);
    result
}

#[cfg(test)]
mod tests;
