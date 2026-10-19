//! The TUI's logger: core diagnostics (`log::warn!` and friends) go into
//! the log pane's buffer, since stderr is the screen while the TUI runs.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use log::{Level, LevelFilter, Log, Metadata, Record};

use super::snapshot::LogLine;

/// The pane keeps this many lines.
pub const CAPACITY: usize = 200;

/// Shared between the logger and the event loop.
#[derive(Debug, Clone, Default)]
pub struct Buffer(Arc<Mutex<VecDeque<LogLine>>>);

impl Buffer {
    pub fn push(&self, line: LogLine) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() == CAPACITY {
            q.pop_front();
        }
        q.push_back(line);
    }

    /// The lines, oldest first.
    pub fn lines(&self) -> Vec<LogLine> {
        let q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        q.iter().cloned().collect()
    }
}

/// One record as the pane shows it (no `sofleii:` prefix: the pane is
/// sofleii's own).
pub fn text(level: Level, args: &std::fmt::Arguments<'_>) -> String {
    match level {
        Level::Error => format!("error: {args}"),
        Level::Warn => format!("warning: {args}"),
        Level::Info | Level::Debug | Level::Trace => args.to_string(),
    }
}

/// The local wall-clock time, `HH:MM:SS`.
pub fn now() -> String {
    jiff::Zoned::now().strftime("%H:%M:%S").to_string()
}

struct Pane(Buffer);

impl Log for Pane {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= LevelFilter::Debug && crate::logger::ours(m)
    }

    fn log(&self, r: &Record<'_>) {
        if self.enabled(r.metadata()) {
            self.0.push(LogLine {
                at: now(),
                text: text(r.level(), r.args()),
            });
        }
    }

    fn flush(&self) {}
}

/// Installs the pane logger and returns its buffer. If a logger is already
/// installed (only in tests), the buffer just stays empty.
pub fn install() -> Buffer {
    let buf = Buffer::default();
    // Leaked once per process: a logger lives as long as the program.
    if log::set_logger(Box::leak(Box::new(Pane(buf.clone())))).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_text_and_capacity() {
        assert_eq!(
            text(Level::Warn, &format_args!("two devices")),
            "warning: two devices"
        );
        assert_eq!(
            text(Level::Debug, &format_args!("stale id 3")),
            "stale id 3"
        );
        let b = Buffer::default();
        for i in 0..CAPACITY + 5 {
            b.push(LogLine {
                at: "00:00:00".into(),
                text: i.to_string(),
            });
        }
        let lines = b.lines();
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines[0].text, "5");
        let t = now();
        assert_eq!(t.len(), 8, "{t}");
    }
}
