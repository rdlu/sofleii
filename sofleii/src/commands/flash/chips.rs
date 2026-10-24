//! The chip memory on disk: `$XDG_STATE_HOME/sofleii/chips.json`
//! (`~/.local/state/sofleii/chips.json` without it). State, not cache:
//! losing it means adopting chips again, so it doesn't go under
//! `$XDG_CACHE_HOME` with the update check.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::flash::Chips;
use crate::flash::chips::SCHEMA;

pub fn default_path() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("."));
    state.join("sofleii").join("chips.json")
}

/// The chips in `path`. A missing file is no chips; an unreadable,
/// malformed or newer-schema file is no chips too, with a warning: the
/// worst case is adopting a chip again, never trusting a misread one.
pub fn load(path: &Path) -> Chips {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Chips::default(),
        Err(e) => {
            log::warn!("ignoring {}: {e}", path.display());
            return Chips::default();
        }
    };
    match serde_json::from_str::<Chips>(&text) {
        Ok(c) if c.schema == SCHEMA => c,
        Ok(c) => {
            log::warn!(
                "ignoring {}: schema {} (this sofleii reads {SCHEMA})",
                path.display(),
                c.schema
            );
            Chips::default()
        }
        Err(e) => {
            log::warn!("ignoring {}: {e}", path.display());
            Chips::default()
        }
    }
}

/// Writes `chips` to `path` atomically (a temporary file beside it, then a
/// rename), creating the directory.
pub fn save(path: &Path, chips: &Chips) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(chips).expect("serialising plain data cannot fail");
    fs::write(&tmp, text + "\n")
        .and_then(|()| fs::rename(&tmp, path))
        .map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::{Half, Learned};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sofleii-chips-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trip_and_tolerance() {
        let d = dir("rt");
        let p = d.join("sofleii").join("chips.json");
        assert_eq!(load(&p), Chips::default());
        let mut c = Chips::default();
        c.learn(
            "93B0CC1C158978F8",
            "eyelash_sofle",
            Half::Right,
            Learned::Adopted,
            "t",
            true,
        );
        save(&p, &c).unwrap();
        assert_eq!(load(&p), c);
        assert!(!p.with_extension("json.tmp").exists());
        fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), Chips::default());
        fs::write(&p, r#"{"schema":2,"chips":{}}"#).unwrap();
        assert_eq!(load(&p), Chips::default());
        let _ = fs::remove_dir_all(&d);
    }
}
