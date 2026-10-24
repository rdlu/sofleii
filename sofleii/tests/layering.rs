//! Source checks for the layering rules in `lib.rs`: core diagnostics go
//! through `log` (only `main.rs` prints to stderr directly), and the core
//! (`session` included) never depends on the TUI.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn only_main_uses_eprintln() {
    let mut files = Vec::new();
    rust_files(&src(), &mut files);
    let offenders: Vec<_> = files
        .iter()
        .filter(|p| !p.ends_with("src/main.rs"))
        .filter(|p| fs::read_to_string(p).unwrap().contains("eprintln!"))
        .collect();
    assert!(
        offenders.is_empty(),
        "use log::warn!/debug! in {offenders:?}"
    );
}

/// A lowercase `tui` word (`crate::tui`, `mod tui`) or `ratatui`; prose
/// saying "TUI" is fine.
fn mentions_tui(text: &str) -> bool {
    // `sofleii-tui-v*` is the crate's release tag, not the front end.
    let text = text.replace("sofleii-tui", "");
    text.contains("ratatui")
        || text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|w| w == "tui")
}

#[test]
fn core_never_depends_on_the_tui() {
    assert!(mentions_tui("use crate::tui::App;") && mentions_tui("ratatui::Frame"));
    assert!(!mentions_tui("front ends (CLI and TUI); intuitive"));
    assert!(!mentions_tui("the newest `sofleii-tui-v*` release"));
    for dir in ["commands", "client", "transport", "session"] {
        let mut files = Vec::new();
        rust_files(&src().join(dir), &mut files);
        assert!(!files.is_empty(), "{dir}");
        for p in files {
            let text = fs::read_to_string(&p).unwrap();
            assert!(!mentions_tui(&text), "{} mentions the TUI", p.display());
        }
    }
}
