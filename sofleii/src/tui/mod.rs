//! The TUI, opened by `sofleii` with no subcommand on a terminal. A front
//! end like the CLI: it reads the core, and `commands`, `client`,
//! `transport` and `session` never depend on it.
//!
//! **Live:** [`run`] spawns a [`Session`] with the CLI's
//! connection flags and folds its events into a [`session::Snapshot`]; the
//! dashboard renders a [`Snapshot`] derived from it. One loop reads one
//! channel, fed by an input thread (crossterm) and a forwarder (the
//! session's events), and redraws at least every [`TICK`] for ages and
//! staleness. `r` refreshes and `u` checks for an update.
//!
//! **Writes:** `⏎`/`s` switches to the selected profile and `o` toggles the
//! output priority. A request [`Request::risk`] calls risky (it drops the
//! current link) opens a confirm modal first, which only `y` confirms; the
//! core never prompts. One
//! write at a time: its spinner sits on the bottom edge until its
//! [`Event::Done`], and action keys are ignored meanwhile. The result
//! stays there briefly (an error until the next key) and goes to the log.
//! `SOFLEII_TUI_DEMO=1` shows a fixture instead, with no session.
//!
//! **Flash mode** (`F`, [`flash`]): a full-screen wizard that flashes the
//! halves through the same job as `sofleii flash`, on a worker thread
//! whose messages arrive on the loop's channel too.
//!
//! The terminal is restored on every way out: a normal quit (`q`, `Esc`,
//! `Ctrl-C`), SIGTERM/SIGHUP/SIGINT (flags checked by the event loop), and a
//! panic: `ratatui::try_init` installs a panic hook that restores first, and
//! hooks run before the release profile's `panic = "abort"`. Quitting never
//! joins a thread: [`Request::Quit`] tells the session to stop, and the
//! process exit takes any thread still blocked in I/O.

pub mod actions;
pub mod flash;
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
use crate::transport::TransportKind;
pub use actions::{Confirm, Flash, Pending};
pub use snapshot::{LogLine, Snapshot};

/// How often the loop wakes without input, for signals, ages and new log
/// lines.
pub const TICK: Duration = Duration::from_millis(250);
/// How long a write's success message stays on the bottom edge.
pub const FLASH_FOR: Duration = Duration::from_secs(5);

/// What the loop reads: the terminal's input and the session's events, on
/// one channel.
#[derive(Debug)]
pub enum Input {
    Term(TermEvent),
    /// Boxed: an event is much larger than a key.
    Session(Box<Event>),
    /// The Flash mode's worker.
    Flash(Box<flash::Msg>),
}

/// Where the loop sends requests: a [`SessionHandle`], or a fake in tests
/// and the demo.
pub trait Requests {
    fn send(&self, req: Request) -> Ticket;
    /// Starts a flash worker; `None` where flashing isn't available (the
    /// demo).
    fn flash(&self, _spec: flash::Spec) -> Option<flash::Worker> {
        None
    }
}

impl Requests for SessionHandle {
    fn send(&self, req: Request) -> Ticket {
        SessionHandle::send(self, req)
    }
}

/// The live session, plus the loop's channel for a flash worker.
struct Live {
    handle: SessionHandle,
    tx: Sender<Input>,
}

impl Requests for Live {
    fn send(&self, req: Request) -> Ticket {
        self.handle.send(req)
    }
    fn flash(&self, spec: flash::Spec) -> Option<flash::Worker> {
        Some(flash::spawn(spec, self.tx.clone()))
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
    /// A risky request waiting for `y` / `n`.
    pub confirm: Option<Confirm>,
    /// The write in flight.
    pub action: Option<Pending>,
    /// The last write's result (or why a key did nothing).
    pub flash: Option<Flash>,
    /// Flash mode, while it's open.
    pub flash_mode: Option<flash::Mode>,
    /// The chip memory to learn Studio's serial into; `None` (the demo,
    /// tests) learns nothing.
    pub chips_path: Option<std::path::PathBuf>,
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
            confirm: None,
            action: None,
            flash: None,
            flash_mode: None,
            chips_path: None,
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

    /// A line of the TUI's own in the Log pane.
    fn note(&mut self, level: LogLevel, at: SystemTime, msg: &str) {
        if self.notes.len() >= log_pane::CAPACITY {
            self.notes.remove(0);
        }
        self.notes.push(LogLine {
            at,
            level,
            text: log_pane::text(level, &format_args!("{msg}")),
        });
    }

    /// Shows `text` on the bottom edge (and in the log).
    fn tell(&mut self, level: LogLevel, text: String, sticky: bool) {
        let now = self.now;
        self.note(level, now, &text);
        self.flash = Some(Flash {
            at: now,
            level,
            text,
            sticky,
        });
    }

    /// The write's `Done`: its result on the bottom edge and in the log.
    fn finish(
        &mut self,
        p: Pending,
        result: &Result<session::Outcome, session::ErrorView>,
        now: SystemTime,
    ) {
        let (level, lines) = actions::finished(&p.req, result);
        let mut lines = lines.into_iter();
        if let Some(first) = lines.next() {
            self.flash = Some(Flash {
                at: now,
                level,
                text: first.clone(),
                sticky: level == LogLevel::Error,
            });
            self.note(level, now, &first);
        }
        for l in lines {
            self.note(LogLevel::Info, now, &l);
        }
    }

    /// Folds one session event in, at `now`.
    pub fn apply(&mut self, e: &Event, now: SystemTime) {
        match e {
            Event::Link(l) if *l != self.session.link => self.link_at = now,
            Event::Done { ticket, result }
                if self.action.as_ref().is_some_and(|p| p.ticket == *ticket) =>
            {
                if let Some(p) = self.action.take() {
                    self.finish(p, result, now);
                }
            }
            Event::Done {
                ticket,
                result: Err(err),
            } => {
                if let Some(what) = self.pending.remove(ticket) {
                    self.note(LogLevel::Warn, now, &format!("{what}: {}", err.message));
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

    /// Moves the clock: ages, staleness, the retry countdown, and a
    /// success message's fade.
    pub fn tick(&mut self, now: SystemTime) {
        self.now = now;
        self.session.tick(now);
        if self.flash.as_ref().is_some_and(|f| {
            !f.sticky && now.duration_since(f.at).unwrap_or(Duration::ZERO) >= FLASH_FOR
        }) {
            self.flash = None;
        }
    }

    /// The selected slot.
    fn selected_profile(&self) -> Option<&crate::commands::profiles::Profile> {
        self.profiles().get(self.selected)
    }

    /// `⏎` / `s`: the selected profile, unless it's already active.
    fn switch_request(&mut self) -> Option<Request> {
        let p = self.selected_profile()?;
        let (index, active) = (p.index, p.active);
        if active {
            self.tell(
                LogLevel::Info,
                format!("profile {index} is already active"),
                false,
            );
            return None;
        }
        Some(Request::SwitchProfile(index))
    }

    /// `o`: the other output priority.
    fn output_request(&mut self) -> Option<Request> {
        let priority = self
            .session
            .status
            .value
            .as_ref()
            .and_then(|p| p.output_priority);
        let target = match priority {
            Some("usb") => TransportKind::Ble,
            Some("ble") => TransportKind::Usb,
            _ => {
                self.tell(LogLevel::Info, "output priority not read yet".into(), false);
                return None;
            }
        };
        Some(Request::SetOutput(target))
    }

    /// A write from a key: held for the modal when risky, else sent.
    fn ask(&mut self, req: Option<Request>) -> Option<Request> {
        let req = req?;
        match actions::confirm(&req, &self.session) {
            Some(c) => {
                self.help = false;
                self.confirm = Some(c);
                None
            }
            None => Some(req),
        }
    }

    /// A key while the modal is up: only `y` sends; anything else cancels,
    /// `⏎` included, so a double Enter on a profile never confirms.
    fn on_confirm_key(&mut self, code: KeyCode) -> Option<Request> {
        let c = self.confirm.take()?;
        match code {
            KeyCode::Char('y' | 'Y') => Some(c.req),
            _ => {
                let text = format!("cancelled: {}", actions::what(&c.req));
                self.tell(LogLevel::Info, text, false);
                None
            }
        }
    }

    /// `F`: opens Flash mode (not while a write is in flight). Learns the
    /// left chip from Studio's serial number on the way, if it's new.
    fn open_flash(&mut self) {
        if self.action.is_some() {
            return;
        }
        let token = ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|t| !t.is_empty()));
        let local = std::env::current_dir()
            .map(|d| flash::find_local(&d))
            .unwrap_or_default();
        if let Some(path) = self.chips_path.clone()
            && let Some(serial) = self
                .session
                .info
                .value
                .as_ref()
                .and_then(|i| i.device.serial_number.clone())
        {
            use crate::commands::flash::chips;
            let mut known = chips::load(&path);
            if flash::learn_from_studio(&mut known, &serial, &flash::now_iso()) {
                let now = self.now;
                match chips::save(&path, &known) {
                    Ok(()) => self.note(
                        LogLevel::Info,
                        now,
                        &format!("flash: chip {serial} is your left half (from Studio)"),
                    ),
                    Err(e) => self.note(LogLevel::Warn, now, &format!("flash: {e}")),
                }
            }
        }
        self.help = false;
        self.flash = None;
        self.flash_mode = Some(flash::Mode::new(flash::Setup::new(token, local)));
    }

    /// What Flash mode asks for.
    fn flash_effect(&mut self, fx: flash::Effect, session: &dyn Requests, now: SystemTime) {
        match fx {
            flash::Effect::None => {}
            flash::Effect::Close => self.flash_mode = None,
            flash::Effect::Log(level, text) => self.note(level, now, &text),
            flash::Effect::Send(req) => {
                session.send(req);
            }
            flash::Effect::Start(spec) => {
                let Some(mode) = self.flash_mode.as_mut() else {
                    return;
                };
                match session.flash(spec.clone()) {
                    Some(w) => {
                        if mode.started(&spec, w) {
                            session.send(Request::Suspend(true));
                        }
                    }
                    None => {
                        if let flash::Stage::Setup(s) = &mut mode.stage {
                            s.problem = Some("flashing isn't available here (demo)".into());
                        }
                    }
                }
            }
        }
    }

    /// A key while Flash mode is open. `Ctrl-C` quits, except mid-write.
    fn flash_key(&mut self, key: KeyEvent, session: &dyn Requests, now: SystemTime) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let writing = self
            .flash_mode
            .as_ref()
            .is_some_and(|m| matches!(&m.stage, flash::Stage::Running(r) if r.writing()));
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if writing {
                self.note(
                    LogLevel::Warn,
                    now,
                    "writing to the half: wait for the copy to finish",
                );
            } else {
                self.quit = true;
            }
            return;
        }
        let Some(mode) = self.flash_mode.as_mut() else {
            return;
        };
        let fx = mode.on_key(key.code);
        self.flash_effect(fx, session, now);
    }

    /// Handles a key; returns the request it asks for, if any.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Request> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        if ctrl_c {
            self.quit = true;
            return None;
        }
        if self.confirm.is_some() {
            return self.on_confirm_key(key.code);
        }
        // Any key dismisses the last result, sticky errors included.
        self.flash = None;
        let busy = self.action.is_some();
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc | KeyCode::Char('?') if self.help => self.help = false,
            KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => return Some(Request::Refresh(Scope::All)),
            KeyCode::Char('u') => return Some(Request::CheckUpdate),
            KeyCode::Char('d') => self.debug = !self.debug,
            KeyCode::Char('F' | 'f') if !busy => self.open_flash(),
            // One write at a time: ignored while one is in flight.
            KeyCode::Enter | KeyCode::Char('s') if !busy => {
                let req = self.switch_request();
                return self.ask(req);
            }
            KeyCode::Char('o') if !busy => {
                let req = self.output_request();
                return self.ask(req);
            }
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
            Input::Term(TermEvent::Key(key)) if self.flash_mode.is_some() => {
                self.flash_key(key, session, now);
            }
            Input::Flash(msg) => {
                if let Some(mode) = self.flash_mode.as_mut() {
                    let fx = mode.apply(*msg, now);
                    for f in fx {
                        self.flash_effect(f, session, now);
                    }
                }
            }
            Input::Term(TermEvent::Key(key)) => {
                if let Some(req) = self.on_key(key) {
                    let write = actions::is_action(&req);
                    let what = match req {
                        Request::CheckUpdate => "update check",
                        _ => "refresh",
                    };
                    let t = session.send(req.clone());
                    if write {
                        self.action = Some(Pending {
                            ticket: t,
                            req,
                            since: now,
                        });
                    } else {
                        self.pending.insert(t, what);
                    }
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
        let live = Live {
            handle,
            tx: tx.clone(),
        };
        let mut app = App::new(snap, color, now);
        app.chips_path = Some(crate::commands::flash::chips::default_path());
        (app, Box::new(live))
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
