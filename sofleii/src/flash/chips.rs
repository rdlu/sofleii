//! The chips sofleii has seen, by USB serial number (#100).
//!
//! A nice!nano's USB serial is the nRF52840's own device ID: the bootloader
//! and ZMK report the same one (left `94708422E5F2FD3E` in both, seen
//! 2026-10-24), and no flash changes it. So once a chip is known as a half,
//! it stays that half whatever firmware it runs: that is what lets sofleii
//! fix a half that got the wrong firmware, and flash a chip whose firmware
//! can't vouch for it (vendor stock, another ZMK, erased) after the user
//! adopted it once. Pure data; [`crate::commands::flash`] loads and saves
//! it as `$XDG_STATE_HOME/sofleii/chips.json`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Half;

/// Bumped when the file's layout changes; an unknown schema is ignored
/// (with a warning) rather than misread.
pub const SCHEMA: u32 = 1;

/// How sofleii came to know a chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Learned {
    /// Flashed after its firmware identified it as this half.
    Verified,
    /// The user typed the half (adoption).
    Adopted,
    /// Studio's `serial_number` while connected (the left half only: the
    /// right half has no Studio).
    Studio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chip {
    pub keyboard: String,
    pub half: Half,
    pub learned: Learned,
    /// ISO 8601 UTC.
    pub first_seen: String,
    /// ISO 8601 UTC; `null` until sofleii flashes it.
    pub last_flashed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chips {
    pub schema: u32,
    /// By serial, upper-case hex as the USB descriptor spells it.
    pub chips: BTreeMap<String, Chip>,
}

impl Default for Chips {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            chips: BTreeMap::new(),
        }
    }
}

/// Serials compare case-insensitively; stored upper-case.
fn key(serial: &str) -> String {
    serial.trim().to_ascii_uppercase()
}

impl Chips {
    pub fn get(&self, serial: &str) -> Option<&Chip> {
        self.chips.get(&key(serial))
    }

    /// Records `serial` as `half` of `keyboard`. A chip already known as
    /// that half keeps its `first_seen` and how it was first learned; one
    /// known as something else (re-adopted, a swapped controller) is
    /// replaced. `flashed` also stamps `last_flashed`. Returns whether
    /// anything changed.
    pub fn learn(
        &mut self,
        serial: &str,
        keyboard: &str,
        half: Half,
        learned: Learned,
        now: &str,
        flashed: bool,
    ) -> bool {
        let k = key(serial);
        let last = flashed.then(|| now.to_string());
        match self.chips.get_mut(&k) {
            Some(c) if c.keyboard == keyboard && c.half == half => {
                if flashed && c.last_flashed.as_deref() != Some(now) {
                    c.last_flashed = last;
                    return true;
                }
                false
            }
            _ => {
                self.chips.insert(
                    k,
                    Chip {
                        keyboard: keyboard.to_string(),
                        half,
                        learned,
                        first_seen: now.to_string(),
                        last_flashed: last,
                    },
                );
                true
            }
        }
    }

    /// Drops a chip; whether it was known.
    pub fn forget(&mut self, serial: &str) -> bool {
        self.chips.remove(&key(serial)).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: &str = "94708422E5F2FD3E";

    #[test]
    fn learns_keeps_history_and_forgets() {
        let mut c = Chips::default();
        assert!(c.learn(
            LEFT,
            "eyelash_sofle",
            Half::Left,
            Learned::Studio,
            "t1",
            false
        ));
        assert!(!c.learn(
            LEFT,
            "eyelash_sofle",
            Half::Left,
            Learned::Verified,
            "t2",
            false
        ));
        assert!(c.learn(
            &LEFT.to_lowercase(),
            "eyelash_sofle",
            Half::Left,
            Learned::Verified,
            "t3",
            true
        ));
        let chip = c.get(LEFT).unwrap();
        assert_eq!(
            (
                chip.learned,
                chip.first_seen.as_str(),
                chip.last_flashed.as_deref()
            ),
            (Learned::Studio, "t1", Some("t3"))
        );
        // Re-adopted as the other half: replaced.
        assert!(c.learn(
            LEFT,
            "eyelash_sofle",
            Half::Right,
            Learned::Adopted,
            "t4",
            true
        ));
        let chip = c.get(LEFT).unwrap();
        assert_eq!(
            (chip.half, chip.learned, chip.first_seen.as_str()),
            (Half::Right, Learned::Adopted, "t4")
        );
        assert!(c.forget(" 94708422e5f2fd3e "));
        assert!(!c.forget(LEFT));
        assert!(c.get(LEFT).is_none());
    }

    #[test]
    fn json_shape() {
        let mut c = Chips::default();
        c.learn(
            LEFT,
            "eyelash_sofle",
            Half::Left,
            Learned::Verified,
            "2026-10-24T22:00:00Z",
            true,
        );
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "schema": 1,
                "chips": { LEFT: {
                    "keyboard": "eyelash_sofle", "half": "left", "learned": "verified",
                    "first_seen": "2026-10-24T22:00:00Z",
                    "last_flashed": "2026-10-24T22:00:00Z"
                }}
            })
        );
        assert_eq!(serde_json::from_value::<Chips>(v).unwrap(), c);
    }
}
