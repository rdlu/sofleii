//! Whether to flash a half onto the chip in bootloader mode (#100).
//!
//! The chip's serial (see [`super::chips`]) outranks the firmware on it: a
//! chip sofleii knows as the left half is the left half, whatever it runs.
//! For a chip it doesn't know, the firmware's identity ([`super::identify`])
//! decides, and when that can't vouch for it either the user **adopts** the
//! chip by typing the half. There is no blind override.
//!
//! | chip serial      | firmware says   | result                               |
//! |------------------|-----------------|--------------------------------------|
//! | known, target    | anything        | flash (a note when firmware differs)  |
//! | known, other     | anything        | refuse; adoption re-learns it        |
//! | unknown          | target          | flash, learn the serial              |
//! | unknown          | other / foreign | refuse; adoption overrides           |
//! | unknown          | unknown         | needs adoption                       |

use serde::Serialize;

use super::chips::Chip;
use super::identify::{Firmware, Half, Identity, KEYBOARD};

/// What the decision rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The chip's serial is known as this half.
    Chip,
    /// The firmware on the chip identifies it as this half.
    Firmware,
    /// The user typed the half.
    Adopted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum Decision {
    Flash {
        basis: Basis,
        notes: Vec<String>,
    },
    /// Nothing vouches for the chip: ask the user to type the half.
    Adopt {
        reason: String,
    },
    /// Evidence says it is not the target; adoption can still override.
    Refuse {
        reason: String,
    },
}

fn firmware_says(id: &Identity) -> String {
    let what = match id.firmware {
        Firmware::Sofleii => "sofleii firmware",
        Firmware::Legacy => "older sofleii firmware",
        Firmware::SettingsReset => "settings_reset, after firmware",
        Firmware::Unknown => "unrecognised firmware",
    };
    match (id.keyboard.as_deref(), id.half) {
        (Some(KEYBOARD), Some(h)) => format!("{what} for the {} half", h.as_str()),
        (Some(k), _) if k != KEYBOARD => format!("firmware for another keyboard ({k})"),
        _ => what.to_string(),
    }
}

/// The decision for flashing `target` onto a chip whose serial maps to
/// `known` (if any) and whose flash identifies as `id`. `adopt` is the half
/// the user typed, if they did; a typed half other than `target` adopts
/// nothing.
pub fn decide(target: Half, known: Option<&Chip>, id: &Identity, adopt: Option<Half>) -> Decision {
    let adopted = adopt == Some(target);
    let ours = id.keyboard.as_deref() == Some(KEYBOARD);
    let fw_half = if ours { id.half } else { None };
    if let Some(chip) = known.filter(|c| c.keyboard == KEYBOARD) {
        if chip.half == target {
            let mut notes = Vec::new();
            if fw_half != Some(target) {
                notes.push(format!(
                    "this chip is your {} half; it runs {}",
                    target.as_str(),
                    firmware_says(id)
                ));
            }
            return Decision::Flash {
                basis: Basis::Chip,
                notes,
            };
        }
        if adopted {
            return Decision::Flash {
                basis: Basis::Adopted,
                notes: vec![format!(
                    "this chip was your {} half; it is re-adopted as the {} half",
                    chip.half.as_str(),
                    target.as_str()
                )],
            };
        }
        return Decision::Refuse {
            reason: format!(
                "this chip is your {} half, not the {} one",
                chip.half.as_str(),
                target.as_str()
            ),
        };
    }
    match fw_half {
        Some(h) if h == target => Decision::Flash {
            basis: Basis::Firmware,
            notes: Vec::new(),
        },
        _ if adopted => Decision::Flash {
            basis: Basis::Adopted,
            notes: Vec::new(),
        },
        Some(h) => Decision::Refuse {
            reason: format!(
                "the chip runs {}, and sofleii doesn't know it as the {} half",
                firmware_says(id),
                h.other().as_str()
            ),
        },
        None if id.keyboard.as_deref().is_some_and(|k| k != KEYBOARD) => Decision::Refuse {
            reason: format!("the chip runs {}", firmware_says(id)),
        },
        None => Decision::Adopt {
            reason: format!(
                "sofleii has never seen this chip and it runs {}{}",
                firmware_says(id),
                id.notes
                    .first()
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default()
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::chips::Learned;
    use Half::*;

    fn id(firmware: Firmware, keyboard: Option<&str>, half: Option<Half>) -> Identity {
        Identity {
            firmware,
            keyboard: keyboard.map(String::from),
            half,
            notes: vec![],
        }
    }

    fn ours(half: Option<Half>) -> Identity {
        id(Firmware::Sofleii, Some(KEYBOARD), half)
    }

    fn unknown() -> Identity {
        id(Firmware::Unknown, None, None)
    }

    fn chip(half: Half) -> Chip {
        Chip {
            keyboard: KEYBOARD.into(),
            half,
            learned: Learned::Verified,
            first_seen: "t".into(),
            last_flashed: None,
        }
    }

    fn basis(d: &Decision) -> Option<Basis> {
        match d {
            Decision::Flash { basis, .. } => Some(*basis),
            _ => None,
        }
    }

    #[test]
    fn a_known_chip_decides() {
        let left = chip(Left);
        let d = decide(Left, Some(&left), &ours(Some(Left)), None);
        assert_eq!(
            d,
            Decision::Flash {
                basis: Basis::Chip,
                notes: vec![]
            }
        );
        // Wrong firmware on the left chip: still the left half (recovery).
        let d = decide(Left, Some(&left), &ours(Some(Right)), None);
        assert_eq!(basis(&d), Some(Basis::Chip));
        let Decision::Flash { notes, .. } = d else {
            unreachable!()
        };
        assert_eq!(
            notes,
            ["this chip is your left half; it runs sofleii firmware for the right half"]
        );
        let d = decide(Left, Some(&left), &unknown(), None);
        assert_eq!(basis(&d), Some(Basis::Chip));
        // The left chip as the right half: refused unless re-adopted.
        let d = decide(Right, Some(&left), &ours(Some(Left)), None);
        assert!(
            matches!(&d, Decision::Refuse { reason } if reason.contains("your left half")),
            "{d:?}"
        );
        let d = decide(Right, Some(&left), &ours(Some(Left)), Some(Right));
        assert_eq!(basis(&d), Some(Basis::Adopted));
        // A typed half that isn't the target adopts nothing.
        let d = decide(Right, Some(&left), &ours(Some(Left)), Some(Left));
        assert!(matches!(d, Decision::Refuse { .. }));
    }

    #[test]
    fn an_unknown_chip_goes_by_its_firmware() {
        assert_eq!(
            basis(&decide(Left, None, &ours(Some(Left)), None)),
            Some(Basis::Firmware)
        );
        let legacy = id(Firmware::Legacy, Some(KEYBOARD), Some(Right));
        assert_eq!(
            basis(&decide(Right, None, &legacy, None)),
            Some(Basis::Firmware)
        );
        let d = decide(Left, None, &ours(Some(Right)), None);
        assert!(
            matches!(&d, Decision::Refuse { reason } if reason.contains("for the right half")),
            "{d:?}"
        );
        assert_eq!(
            basis(&decide(Left, None, &ours(Some(Right)), Some(Left))),
            Some(Basis::Adopted)
        );
        let corne = id(Firmware::Sofleii, Some("corne"), Some(Left));
        let d = decide(Left, None, &corne, None);
        assert!(
            matches!(&d, Decision::Refuse { reason } if reason.contains("corne")),
            "{d:?}"
        );
        assert_eq!(
            basis(&decide(Left, None, &corne, Some(Left))),
            Some(Basis::Adopted)
        );
    }

    #[test]
    fn nothing_vouches_so_adopt() {
        let mut erased = unknown();
        erased
            .notes
            .push("the flash holds remains of more than one firmware".into());
        let d = decide(Left, None, &erased, None);
        assert!(
            matches!(&d, Decision::Adopt { reason } if reason.contains("never seen") && reason.contains("remains")),
            "{d:?}"
        );
        assert_eq!(
            basis(&decide(Left, None, &erased, Some(Left))),
            Some(Basis::Adopted)
        );
        assert!(matches!(
            decide(Left, None, &erased, Some(Right)),
            Decision::Adopt { .. }
        ));
        // settings_reset with nothing left of what it replaced.
        let reset = id(Firmware::SettingsReset, None, None);
        assert!(matches!(
            decide(Right, None, &reset, None),
            Decision::Adopt { .. }
        ));
    }

    #[test]
    fn json() {
        let d = decide(Left, None, &unknown(), None);
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["decision"], "adopt");
        let v = serde_json::to_value(decide(Left, None, &ours(Some(Left)), None)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"decision": "flash", "basis": "firmware", "notes": []})
        );
    }
}
