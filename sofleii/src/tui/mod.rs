//! The TUI, opened by `sofleii` with no subcommand on a terminal. A front
//! end like the CLI: it reads the core, and `commands`, `client` and
//! `transport` never depend on it.
//!
//! **Skeleton:** the dashboard renders a [`Snapshot`] but nothing fills it
//! from the keyboard yet, so it shows the "Searching…" state
//! (`SOFLEII_TUI_DEMO=1` shows a fixture instead, for screenshots), and
//! only quit, selection and help are wired.
//!
//! The terminal is restored on every way out: a normal quit (`q`, `Esc`,
//! `Ctrl-C`), SIGTERM/SIGHUP/SIGINT (flags checked by the event loop), and a
//! panic: `ratatui::try_init` installs a panic hook that restores first, and
//! hooks run before the release profile's `panic = "abort"`.

pub mod log_pane;
pub mod snapshot;
pub mod view;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::output::{Format, Style};
pub use snapshot::Snapshot;

/// How often the loop wakes without input, to see signals and new log lines.
const TICK: Duration = Duration::from_millis(250);

/// Everything the view draws: the snapshot plus UI state.
#[derive(Debug, Clone)]
pub struct App {
    pub snapshot: Snapshot,
    /// Index into the snapshot's profiles.
    pub selected: usize,
    pub help: bool,
    pub color: bool,
    pub quit: bool,
}

impl App {
    /// Starts with the active profile selected.
    pub fn new(snapshot: Snapshot, color: bool) -> Self {
        let selected = snapshot
            .keyboard
            .as_ref()
            .and_then(|k| k.profiles.iter().position(|p| p.active))
            .unwrap_or(0);
        Self {
            snapshot,
            selected,
            help: false,
            color,
            quit: false,
        }
    }

    fn profile_count(&self) -> usize {
        self.snapshot
            .keyboard
            .as_ref()
            .map_or(0, |k| k.profiles.len())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        match key.code {
            _ if ctrl_c => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc | KeyCode::Char('?') if self.help => self.help = false,
            KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.profile_count().saturating_sub(1));
            }
            _ => {}
        }
    }
}

/// Runs the TUI until the user quits or a signal arrives.
pub fn run() -> io::Result<()> {
    let log = log_pane::install();
    let stop = Arc::new(AtomicBool::new(false));
    for sig in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(sig, Arc::clone(&stop))?;
    }

    let demo = std::env::var_os("SOFLEII_TUI_DEMO").is_some_and(|v| v == "1");
    let mut snapshot = if demo {
        Snapshot::demo()
    } else {
        Snapshot::searching()
    };
    // The demo's log goes through the buffer like any other line.
    for line in std::mem::take(&mut snapshot.log) {
        log.push(line);
    }
    if !demo {
        log::info!("TUI skeleton: no live data yet (use `sofleii status` for now)");
    }
    let mut app = App::new(snapshot, Style::detect(Format::Human).color);

    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal, &mut app, &log, &stop);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    log: &log_pane::Buffer,
    stop: &AtomicBool,
) -> io::Result<()> {
    while !app.quit && !stop.load(Ordering::SeqCst) {
        app.snapshot.log = log.lines();
        terminal.draw(|f| view::render(f, app))?;
        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
        {
            app.on_key(key);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
