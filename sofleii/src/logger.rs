//! Diagnostics from the core go through the `log` facade; each front end
//! installs its own logger. The CLI's writes them to stderr in the
//! `sofleii: warning: …` form it always printed; the TUI's
//! ([`crate::tui`]) puts them in its log pane instead.

use std::io::Write;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// The CLI shows everything down to `debug` (a stale response id is one).
pub const CLI_LEVEL: LevelFilter = LevelFilter::Debug;

/// Only sofleii's own records: dependencies log too (ureq and rustls trace
/// every HTTPS request at `debug`), and none of that is for the user.
pub fn ours(m: &Metadata<'_>) -> bool {
    let t = m.target();
    t == "sofleii" || t.starts_with("sofleii::")
}

/// One diagnostic as the CLI prints it: `warning:`/`error:` keep their
/// prefix, the rest is plain.
pub fn line(level: Level, args: &std::fmt::Arguments<'_>) -> String {
    match level {
        Level::Error => format!("sofleii: error: {args}"),
        Level::Warn => format!("sofleii: warning: {args}"),
        Level::Info | Level::Debug | Level::Trace => format!("sofleii: {args}"),
    }
}

struct Stderr;

impl Log for Stderr {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= CLI_LEVEL && ours(m)
    }

    fn log(&self, r: &Record<'_>) {
        if self.enabled(r.metadata()) {
            let _ = writeln!(std::io::stderr(), "{}", line(r.level(), r.args()));
        }
    }

    fn flush(&self) {}
}

/// Installs the stderr logger (once per process; later calls do nothing).
pub fn install_stderr() {
    if log::set_logger(&Stderr).is_ok() {
        log::set_max_level(CLI_LEVEL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_lines_keep_the_old_text() {
        let w = "two BlueZ devices match";
        assert_eq!(
            line(Level::Warn, &format_args!("{w}")),
            "sofleii: warning: two BlueZ devices match"
        );
        assert_eq!(
            line(
                Level::Debug,
                &format_args!("ignoring stale response id {}", 7)
            ),
            "sofleii: ignoring stale response id 7"
        );
        assert_eq!(
            line(Level::Error, &format_args!("boom")),
            "sofleii: error: boom"
        );
    }

    #[test]
    fn dependencies_are_filtered_out() {
        let m = |target| {
            Metadata::builder()
                .level(Level::Debug)
                .target(target)
                .build()
        };
        assert!(Stderr.enabled(&m("sofleii::client")));
        assert!(Stderr.enabled(&m("sofleii")));
        assert!(!Stderr.enabled(&m("ureq::unversioned::transport")));
        assert!(!Stderr.enabled(&m("rustls::client::hs")));
        assert!(!Stderr.enabled(&m("sofleiiish")));
    }
}
