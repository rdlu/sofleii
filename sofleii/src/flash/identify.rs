//! Which keyboard and half a flash image (a `CURRENT.UF2`) belongs to.
//!
//! The evidence, strongest first:
//!
//! 1. **The identity record** (`zmk-sofleii-id`, every build since #98): a
//!    `SOFLEII-ID/1` magic, the image's reset handler, the keyboard id and
//!    the half. It only counts when that reset handler is the reset entry
//!    of the application's vector table (see [`app_start`]), i.e. the record
//!    belongs to the image that boots.
//! 2. **Remains of an older image.** The bootloader only rewrites the pages
//!    a new image covers, so the tail of a longer, earlier image survives
//!    behind a shorter one: records whose reset handler doesn't match, and
//!    the strings below. They say what this chip ran before, which is still
//!    the physical half unless someone flashed the wrong one then.
//! 3. **Strings of builds without the record** (≤ v3.2.0): the keyboard
//!    name `Eyelash Sofle`, plus `ble_central` on the left (central) half or
//!    `ble_peripheral` on the right. Both together are the remains of two
//!    images, so they decide nothing.
//!
//! `settings_reset` (ZMK's, recognised by `settings_reset_kscan`) is a
//! small image, so it always has such a tail: keyboard and half then come
//! from the firmware it replaced, which is what makes "reset, then flash"
//! checkable. Conflicting evidence is never resolved by guessing: the
//! answer is then unknown.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Image;

/// What the image that boots is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Firmware {
    /// Our firmware with a valid identity record.
    Sofleii,
    /// Our firmware from before the record (≤ v3.2.0), or another build of
    /// an Eyelash Sofle, identified by its strings.
    Legacy,
    /// ZMK's `settings_reset`; keyboard and half are from what it replaced.
    SettingsReset,
    /// Anything else: an erased chip, another keyboard, conflicting
    /// evidence.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Half {
    Left,
    Right,
}

impl Half {
    pub fn as_str(self) -> &'static str {
        match self {
            Half::Left => "left",
            Half::Right => "right",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Half::Left => Half::Right,
            Half::Right => Half::Left,
        }
    }

    /// `left` or `right`, exactly (the record's spelling and what adoption
    /// asks the user to type).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Half::Left),
            "right" => Some(Half::Right),
            _ => None,
        }
    }
}

/// The keyboard id this repo's firmware writes into its record.
pub const KEYBOARD: &str = "eyelash_sofle";

/// The answer: what boots, and whose chip this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    pub firmware: Firmware,
    /// The record's keyboard id; [`KEYBOARD`] for a legacy image.
    pub keyboard: Option<String>,
    pub half: Option<Half>,
    /// Why the answer is weaker than a record, or unknown.
    pub notes: Vec<String>,
}

const MAGIC_PREFIX: &[u8] = b"SOFLEII-ID/";
const MAGIC: &[u8; 16] = b"SOFLEII-ID/1\0\0\0\0";
const RECORD_LEN: usize = 16 + 4 + 16 + 8;

/// One identity record found in the image.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    reset: u32,
    keyboard: String,
    half: Option<Half>,
}

/// A NUL-padded C string field.
fn field(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Every `SOFLEII-ID/1` record in the image (4-byte aligned, as the
/// firmware lays it out), plus the count of records in a layout this
/// sofleii doesn't know.
fn records(img: &Image) -> (Vec<Record>, usize) {
    let mut found = Vec::new();
    let mut unknown = 0;
    let b = &img.bytes;
    let mut at = 0;
    while let Some(i) = find(&b[at..], MAGIC_PREFIX).map(|i| i + at) {
        at = i + 1;
        if i % 4 != 0 {
            continue;
        }
        let Some(r) = b.get(i..i + RECORD_LEN) else {
            continue;
        };
        if &r[..16] != MAGIC {
            unknown += 1;
            continue;
        }
        found.push(Record {
            reset: u32::from_le_bytes(r[16..20].try_into().expect("4 bytes")),
            keyboard: field(&r[20..36]),
            half: Half::parse(&field(&r[36..44])),
        });
    }
    (found, unknown)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A whole NUL-terminated string (NUL on both sides), as the firmware's
/// string tables store it.
fn has_cstr(img: &Image, s: &str) -> bool {
    let mut needle = Vec::with_capacity(s.len() + 2);
    needle.push(0);
    needle.extend_from_slice(s.as_bytes());
    needle.push(0);
    find(&img.bytes, &needle).is_some()
}

/// Nordic's SoftDevice info struct sits at SoftDevice base + 0x2000: its
/// size, then this magic, then the SoftDevice's end, which is where the
/// application starts (`0x26000` for S140 6.1.1).
const SD_INFO: u32 = 0x3000;
const SD_MAGIC: u32 = 0x51B1_E5DB;

/// The address of the application's vector table. The Adafruit bootloader's
/// `CURRENT.UF2` starts at the SoftDevice (`0x1000`), not at the
/// application, so the app start comes from the SoftDevice info struct when
/// the image has one; an image without it (a `.uf2` build) starts at the
/// application.
pub fn app_start(img: &Image) -> u32 {
    let at = |addr: u32| {
        addr.checked_sub(img.base)
            .and_then(|o| img.word(o as usize))
    };
    match (at(SD_INFO + 4), at(SD_INFO + 8)) {
        (Some(SD_MAGIC), Some(app)) if app > SD_INFO => app,
        _ => img.base,
    }
}

/// Identifies the image; see the module docs for the rules.
pub fn identify(img: &Image) -> Identity {
    let (records, unknown_layouts) = records(img);
    let mut notes = Vec::new();
    if unknown_layouts > 0 {
        notes.push("found an identity record in a newer layout; update sofleii".into());
    }

    // The reset entry of the application's vector table, without the
    // Thumb bit.
    let boot = (app_start(img) - img.base)
        .checked_add(4)
        .and_then(|o| img.word(o as usize))
        .map(|w| w & !1);
    let (current, stale): (Vec<_>, Vec<_>) = records
        .into_iter()
        .partition(|r| Some(r.reset & !1) == boot);

    // 1. A record of the image that boots.
    let ids: BTreeSet<_> = current.iter().map(|r| (&r.keyboard, r.half)).collect();
    match ids.len() {
        0 => {}
        1 => {
            let (keyboard, half) = ids.into_iter().next().expect("one");
            if half.is_none() {
                notes.push("the identity record names no known half".into());
            }
            return Identity {
                firmware: Firmware::Sofleii,
                keyboard: Some(keyboard.clone()),
                half,
                notes,
            };
        }
        _ => {
            notes.push("the image carries conflicting identity records".into());
            return unknown(notes);
        }
    }

    // 2./3. Everything else: older records and the legacy strings.
    let mut evidence: BTreeSet<(String, Option<Half>)> =
        stale.iter().map(|r| (r.keyboard.clone(), r.half)).collect();
    let eyelash = has_cstr(img, "Eyelash Sofle");
    if eyelash {
        if has_cstr(img, "ble_central") {
            evidence.insert((KEYBOARD.into(), Some(Half::Left)));
        }
        if has_cstr(img, "ble_peripheral") {
            evidence.insert((KEYBOARD.into(), Some(Half::Right)));
        }
    }
    let reset = has_cstr(img, "settings_reset_kscan");
    let single = (evidence.len() == 1).then(|| evidence.iter().next().expect("one").clone());
    if evidence.len() > 1 {
        notes.push(
            "the flash holds remains of more than one firmware that disagree on the half".into(),
        );
    }

    if reset {
        match &single {
            Some(_) => notes.push(
                "the chip holds settings_reset; keyboard and half come from the remains of the \
                 firmware it replaced"
                    .into(),
            ),
            None if evidence.is_empty() => notes.push(
                "the chip holds settings_reset and nothing is left of the firmware it replaced"
                    .into(),
            ),
            None => {}
        }
        let (keyboard, half) = single.unzip();
        return Identity {
            firmware: Firmware::SettingsReset,
            keyboard,
            half: half.flatten(),
            notes,
        };
    }
    match single {
        // Legacy needs its own strings, not just an older record's remains.
        Some((keyboard, half)) if eyelash => {
            if !stale.is_empty() {
                notes.push(
                    "an older identity record agrees, but the image that boots has none".into(),
                );
            }
            Identity {
                firmware: Firmware::Legacy,
                keyboard: Some(keyboard),
                half,
                notes,
            }
        }
        Some(_) => {
            notes.push(
                "only the remains of an earlier sofleii firmware were found; the image that \
                 boots is something else"
                    .into(),
            );
            unknown(notes)
        }
        None => unknown(notes),
    }
}

fn unknown(notes: Vec<String>) -> Identity {
    Identity {
        firmware: Firmware::Unknown,
        keyboard: None,
        half: None,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 0x26000;

    /// An image of `len` bytes whose vector table boots `reset`.
    fn image(len: usize, reset: u32) -> Vec<u8> {
        let mut b = vec![0x5A; len];
        b[0..4].copy_from_slice(&0x2000_8000u32.to_le_bytes());
        b[4..8].copy_from_slice(&reset.to_le_bytes());
        b
    }

    fn record(b: &mut [u8], at: usize, reset: u32, keyboard: &str, half: &str) {
        let mut r = [0u8; RECORD_LEN];
        r[..16].copy_from_slice(MAGIC);
        r[16..20].copy_from_slice(&reset.to_le_bytes());
        r[20..20 + keyboard.len()].copy_from_slice(keyboard.as_bytes());
        r[36..36 + half.len()].copy_from_slice(half.as_bytes());
        b[at..at + RECORD_LEN].copy_from_slice(&r);
    }

    fn cstr(b: &mut [u8], at: usize, s: &str) {
        b[at] = 0;
        b[at + 1..at + 1 + s.len()].copy_from_slice(s.as_bytes());
        b[at + 1 + s.len()] = 0;
    }

    /// Writes `newer` over `older` the way the bootloader does: only the
    /// pages it covers change.
    fn flash_over(older: &[u8], newer: &[u8]) -> Vec<u8> {
        let mut b = older.to_vec();
        if b.len() < newer.len() {
            b.resize(newer.len(), 0xFF);
        }
        b[..newer.len()].copy_from_slice(newer);
        b
    }

    fn id(bytes: Vec<u8>) -> Identity {
        identify(&Image { base: BASE, bytes })
    }

    fn left_v4() -> Vec<u8> {
        let mut b = image(4096, 0x35605);
        record(&mut b, 0x800, 0x35605, KEYBOARD, "left");
        b
    }

    fn right_v4() -> Vec<u8> {
        let mut b = image(3072, 0x2be31);
        record(&mut b, 0x600, 0x2be31, KEYBOARD, "right");
        b
    }

    /// A build without the record; its strings sit near the end, as in
    /// the real images (`.rodata` after `.text`).
    fn legacy(len: usize, role: &str) -> Vec<u8> {
        let mut b = image(len, 0x3501);
        cstr(&mut b, len - 0x200, "Eyelash Sofle");
        cstr(&mut b, len - 0x100, role);
        b
    }

    fn settings_reset() -> Vec<u8> {
        let mut b = image(1024, 0x2001);
        cstr(&mut b, 0x100, "settings_reset_kscan");
        b
    }

    #[test]
    fn a_record_of_the_booting_image_wins() {
        let i = id(left_v4());
        assert_eq!(
            (i.firmware, i.keyboard.as_deref(), i.half),
            (Firmware::Sofleii, Some(KEYBOARD), Some(Half::Left))
        );
        assert!(i.notes.is_empty(), "{:?}", i.notes);
        // The right half's image over the left's: the left record survives
        // in the tail but doesn't boot.
        let i = id(flash_over(&left_v4(), &right_v4()));
        assert_eq!((i.firmware, i.half), (Firmware::Sofleii, Some(Half::Right)));
    }

    /// What the Adafruit bootloader's `CURRENT.UF2` holds: the SoftDevice
    /// from 0x1000 (its own vector table first, its info struct at 0x3000)
    /// and the application from 0x26000.
    fn with_softdevice(app: &[u8]) -> Image {
        let mut b = vec![0xFF; (BASE - 0x1000) as usize];
        b[4..8].copy_from_slice(&0x257B1u32.to_le_bytes());
        b[0x2004..0x2008].copy_from_slice(&SD_MAGIC.to_le_bytes());
        b[0x2008..0x200C].copy_from_slice(&BASE.to_le_bytes());
        b.extend_from_slice(app);
        Image {
            base: 0x1000,
            bytes: b,
        }
    }

    #[test]
    fn current_uf2_starts_at_the_softdevice() {
        let img = with_softdevice(&left_v4());
        assert_eq!(app_start(&img), BASE);
        let i = identify(&img);
        assert_eq!((i.firmware, i.half), (Firmware::Sofleii, Some(Half::Left)));
        assert!(i.notes.is_empty(), "{:?}", i.notes);
        // A record whose reset is the SoftDevice's is not the app's.
        let mut app = image(2048, 0x35605);
        record(&mut app, 0x400, 0x257B1, KEYBOARD, "left");
        assert_eq!(identify(&with_softdevice(&app)).firmware, Firmware::Unknown);
        // No info struct (a plain build): the image starts at the app.
        let plain = Image {
            base: BASE,
            bytes: left_v4(),
        };
        assert_eq!(app_start(&plain), BASE);
    }

    #[test]
    fn the_thumb_bit_does_not_matter() {
        let mut b = image(2048, 0x35605);
        record(&mut b, 0x400, 0x35604, KEYBOARD, "left");
        assert_eq!(id(b).firmware, Firmware::Sofleii);
    }

    #[test]
    fn records_must_be_aligned_and_whole() {
        let mut b = image(2048, 0x35605);
        record(&mut b, 0x402, 0x35605, KEYBOARD, "left");
        assert_eq!(id(b).firmware, Firmware::Unknown);
        let mut b = image(2048, 0x35605);
        b[2040..2048].copy_from_slice(&MAGIC[..8]);
        assert_eq!(id(b).firmware, Firmware::Unknown);
    }

    #[test]
    fn legacy_builds_by_their_strings() {
        let i = id(legacy(4096, "ble_central"));
        assert_eq!(
            (i.firmware, i.keyboard.as_deref(), i.half),
            (Firmware::Legacy, Some(KEYBOARD), Some(Half::Left))
        );
        let i = id(legacy(4096, "ble_peripheral"));
        assert_eq!((i.firmware, i.half), (Firmware::Legacy, Some(Half::Right)));
        // A role string alone (another ZMK split keyboard) is not ours.
        let mut b = image(4096, 0x3501);
        cstr(&mut b, 0x200, "ble_central");
        assert_eq!(id(b).firmware, Firmware::Unknown);
        // Substrings of longer strings don't count.
        let mut b = image(4096, 0x3501);
        cstr(&mut b, 0x100, "Eyelash Sofle 2");
        cstr(&mut b, 0x200, "ble_central");
        assert_eq!(id(b).firmware, Firmware::Unknown);
    }

    #[test]
    fn legacy_right_over_legacy_left_is_ambiguous() {
        let i = id(flash_over(
            &legacy(4096, "ble_central"),
            &legacy(2048, "ble_peripheral"),
        ));
        assert_eq!((i.firmware, i.half), (Firmware::Unknown, None));
        assert!(i.notes[0].contains("disagree"), "{:?}", i.notes);
    }

    #[test]
    fn a_downgrade_keeps_the_half_when_the_remains_agree() {
        let i = id(flash_over(&left_v4(), &legacy(2048, "ble_central")));
        assert_eq!((i.firmware, i.half), (Firmware::Legacy, Some(Half::Left)));
        assert!(
            i.notes[0].contains("older identity record"),
            "{:?}",
            i.notes
        );
        let i = id(flash_over(&left_v4(), &legacy(2048, "ble_peripheral")));
        assert_eq!(i.firmware, Firmware::Unknown);
    }

    #[test]
    fn settings_reset_takes_the_half_from_what_it_replaced() {
        for (older, half) in [
            (left_v4(), Half::Left),
            (right_v4(), Half::Right),
            (legacy(4096, "ble_peripheral"), Half::Right),
        ] {
            let i = id(flash_over(&older, &settings_reset()));
            assert_eq!(
                (i.firmware, i.keyboard.as_deref(), i.half),
                (Firmware::SettingsReset, Some(KEYBOARD), Some(half))
            );
            assert!(i.notes[0].contains("remains"), "{:?}", i.notes);
        }
        let i = id(settings_reset());
        assert_eq!((i.firmware, i.half), (Firmware::SettingsReset, None));
        assert!(i.notes[0].contains("nothing is left"), "{:?}", i.notes);
    }

    #[test]
    fn only_old_remains_under_foreign_firmware_is_unknown() {
        let foreign = image(1024, 0x7001);
        let i = id(flash_over(&left_v4(), &foreign));
        assert_eq!((i.firmware, i.half), (Firmware::Unknown, None));
        assert!(i.notes[0].contains("something else"), "{:?}", i.notes);
        assert_eq!(id(vec![0xFF; 4096]).firmware, Firmware::Unknown);
    }

    #[test]
    fn conflicting_and_newer_records() {
        let mut b = left_v4();
        record(&mut b, 0xA00, 0x35605, KEYBOARD, "right");
        let i = id(b);
        assert_eq!(i.firmware, Firmware::Unknown);
        assert!(i.notes[0].contains("conflicting"), "{:?}", i.notes);

        let mut b = image(2048, 0x35605);
        b[0x400..0x40C].copy_from_slice(b"SOFLEII-ID/2");
        let i = id(b);
        assert_eq!(i.firmware, Firmware::Unknown);
        assert!(i.notes[0].contains("newer layout"), "{:?}", i.notes);
    }

    #[test]
    fn another_keyboard_with_a_record() {
        let mut b = image(2048, 0x35605);
        record(&mut b, 0x400, 0x35605, "corne", "left");
        let i = id(b);
        assert_eq!(
            (i.firmware, i.keyboard.as_deref(), i.half),
            (Firmware::Sofleii, Some("corne"), Some(Half::Left))
        );
    }
}
