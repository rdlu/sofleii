//! The TUI's logger: core diagnostics (`log::warn!` and friends) go into
//! the log pane's buffer, since stderr is the screen while the TUI runs.
//! The pane merges them with the session's own log by time.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use jiff::tz::TimeZone;
use log::{Level, LevelFilter, Log, Metadata, Record};

use super::snapshot::LogLine;
use crate::session::LogLevel;

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

/// The session's level for a `log` record.
pub fn level(l: Level) -> LogLevel {
    match l {
        Level::Error => LogLevel::Error,
        Level::Warn => LogLevel::Warn,
        Level::Info => LogLevel::Info,
        Level::Debug | Level::Trace => LogLevel::Debug,
    }
}

/// One line as the pane shows it (no `sofleii:` prefix: the pane is
/// sofleii's own).
pub fn text(level: LogLevel, args: &std::fmt::Arguments<'_>) -> String {
    match level {
        LogLevel::Error => format!("error: {args}"),
        LogLevel::Warn => format!("warning: {args}"),
        LogLevel::Info | LogLevel::Debug => args.to_string(),
    }
}

/// `at` as wall-clock time in `tz`, `HH:MM:SS`.
pub fn clock(at: SystemTime, tz: &TimeZone) -> String {
    jiff::Timestamp::try_from(at).map_or_else(
        |_| "--:--:--".into(),
        |t| t.to_zoned(tz.clone()).strftime("%H:%M:%S").to_string(),
    )
}

struct Pane(Buffer);

impl Log for Pane {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= LevelFilter::Debug && crate::logger::ours(m)
    }

    fn log(&self, r: &Record<'_>) {
        if self.enabled(r.metadata()) {
            let level = level(r.level());
            self.0.push(LogLine {
                at: SystemTime::now(),
                level,
                text: text(level, r.args()),
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
            text(level(Level::Warn), &format_args!("two devices")),
            "warning: two devices"
        );
        assert_eq!(
            text(level(Level::Debug), &format_args!("stale id 3")),
            "stale id 3"
        );
        let b = Buffer::default();
        for i in 0..CAPACITY + 5 {
            b.push(LogLine {
                at: SystemTime::UNIX_EPOCH,
                level: LogLevel::Info,
                text: i.to_string(),
            });
        }
        let lines = b.lines();
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines[0].text, "5");
        let t = clock(SystemTime::now(), &TimeZone::system());
        assert_eq!(t.len(), 8, "{t}");
        assert_eq!(clock(SystemTime::UNIX_EPOCH, &TimeZone::UTC), "00:00:00");
    }
}
