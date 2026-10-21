//! HID usages ↔ ZMK key names, from the generated [`keys_table`](super::keys_table)
//! (every `keys.h` name: keyboard page 0x07, consumer page 0x0C and the few
//! generic-desktop ones).
//!
//! A `&kp` parameter is ZMK's encoded usage: `page << 16 | id`, with
//! implicit modifiers in the high byte (`LS(N1)` = `0x02_07_001e`, which is
//! also `EXCL`). Rendering looks the whole value up first (so shifted
//! symbols keep their names: `LBRC`, `PIPE`), then wraps the base key in
//! the modifier functions (`LG(LS(H))`). Unknown values stay numeric.

use super::keys_table::{NAMES, PREFERRED};

/// ZMK's modifier functions and their bits in the high byte, from the
/// outermost (`LG(…)` wraps `LS(…)`) in.
const MODS: [(&str, u32); 8] = [
    ("RG", 0x80),
    ("RA", 0x40),
    ("RS", 0x20),
    ("RC", 0x10),
    ("LG", 0x08),
    ("LA", 0x04),
    ("LS", 0x02),
    ("LC", 0x01),
];

/// The value of a ZMK key name (any alias: `LSHIFT`, `LSHFT`, `LEFT_SHIFT`).
pub fn value(name: &str) -> Option<u32> {
    NAMES.iter().find(|(n, _)| *n == name).map(|&(_, v)| v)
}

/// The modifier function's bit (`LS` → `0x02`), for the `.keymap` parser.
pub fn modifier_bit(func: &str) -> Option<u32> {
    MODS.iter().find(|(n, _)| *n == func).map(|&(_, b)| b)
}

fn preferred(v: u32) -> Option<&'static str> {
    PREFERRED
        .binary_search_by_key(&v, |&(k, _)| k)
        .ok()
        .map(|i| PREFERRED[i].1)
}

/// The ZMK name for an encoded usage: `A`, `LSHFT`, `C_VOL_UP`, `LBRC`,
/// `LG(LS(H))`; `0x…` hex when the usage is unknown.
pub fn name(v: u32) -> String {
    if let Some(n) = preferred(v) {
        return n.to_string();
    }
    let mods = v >> 24;
    let base = v & 0x00ff_ffff;
    let Some(base_name) = preferred(base) else {
        return format!("0x{v:x}");
    };
    if mods == 0 {
        return base_name.to_string();
    }
    let mut out = base_name.to_string();
    // Innermost first, so the outermost function ends up outside.
    for &(f, bit) in MODS.iter().rev() {
        if mods & bit != 0 {
            out = format!("{f}({out})");
        }
    }
    out
}

/// A short legend for a key name (at most a few characters where there's a
/// symbol for it): `N1` → `1`, `LBRC` → `{`, `C_VOL_UP` → `VOL+`, and
/// modifier functions as letters, `LG(LS(H))` → `GS+H`.
pub fn legend(v: u32) -> String {
    if let Some(n) = preferred(v) {
        return short(n);
    }
    let mods = v >> 24;
    let base = v & 0x00ff_ffff;
    match preferred(base) {
        Some(n) if mods != 0 => {
            let letters: String = [(0x11, 'C'), (0x22, 'S'), (0x44, 'A'), (0x88, 'G')]
                .iter()
                .filter(|(bits, _)| mods & bits != 0)
                .map(|&(_, c)| c)
                .collect();
            format!("{letters}+{}", short(n))
        }
        Some(n) => short(n),
        None => format!("0x{v:x}"),
    }
}

fn short(name: &str) -> String {
    let s = match name {
        "GRAVE" => "`",
        "MINUS" => "-",
        "EQUAL" => "=",
        "LBKT" => "[",
        "RBKT" => "]",
        "BSLH" => "\\",
        "SEMI" => ";",
        "APOS" => "'",
        "COMMA" => ",",
        "DOT" => ".",
        "FSLH" => "/",
        "EXCL" => "!",
        "AT" => "@",
        "HASH" => "#",
        "DLLR" => "$",
        "PRCNT" => "%",
        "CARET" => "^",
        "AMPS" => "&",
        "STAR" => "*",
        "LPAR" => "(",
        "RPAR" => ")",
        "UNDER" => "_",
        "PLUS" => "+",
        "LBRC" => "{",
        "RBRC" => "}",
        "PIPE" => "|",
        "COLON" => ":",
        "DQT" => "\"",
        "TILDE" => "~",
        "LT" => "<",
        "GT" => ">",
        "QMARK" => "?",
        "LEFT" => "←",
        "RIGHT" => "→",
        "UP" => "↑",
        "DOWN" => "↓",
        "LSHFT" | "RSHFT" => "SHIFT",
        "LCTRL" | "RCTRL" => "CTRL",
        "LALT" => "ALT",
        "RALT" => "ALTGR",
        "LGUI" | "RGUI" => "GUI",
        "PG_UP" => "PGUP",
        "PG_DN" => "PGDN",
        "PSCRN" => "PRTSC",
        "PAUSE_BREAK" => "PAUSE",
        "K_APP" => "MENU",
        "C_VOL_UP" => "VOL+",
        "C_VOL_DN" => "VOL-",
        "C_PP" => "PLAY",
        "C_MUTE" => "MUTE",
        "C_NEXT" => "NEXT",
        "C_PREV" => "PREV",
        "C_STOP" => "STOP",
        _ => {
            if let Some(d) = name.strip_prefix('N').filter(|d| d.len() == 1) {
                return d.to_string();
            }
            if let Some(rest) = name.strip_prefix("KP_") {
                let rest = rest
                    .strip_prefix('N')
                    .filter(|d| d.len() == 1)
                    .unwrap_or(rest);
                let sym = match rest {
                    "SLASH" => "/",
                    "MULTIPLY" | "ASTERISK" => "*",
                    "MINUS" => "-",
                    "PLUS" => "+",
                    "DOT" => ".",
                    "EQUAL" => "=",
                    "ENTER" => "ENT",
                    other => other,
                };
                return format!("KP{sym}");
            }
            return name
                .strip_prefix("C_")
                .or_else(|| name.strip_prefix("K_"))
                .unwrap_or(name)
                .to_string();
        }
    };
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_aliases() {
        assert_eq!(value("A"), Some(0x0007_0004));
        assert_eq!(value("LSHIFT"), value("LSHFT"));
        assert_eq!(value("LEFT_SHIFT"), Some(0x0007_00e1));
        assert_eq!(value("C_VOL_UP"), Some(0x000c_00e9));
        assert_eq!(value("EXCL"), Some(0x0207_001e));
        assert_eq!(value("NOPE"), None);
        assert_eq!(name(0x0007_0004), "A");
        assert_eq!(name(0x0007_00e1), "LSHFT");
        assert_eq!(name(0x0007_00e0), "LCTRL");
        assert_eq!(name(0x0007_0028), "RET");
        assert_eq!(name(0x0007_0035), "GRAVE");
        assert_eq!(name(0x0007_0034), "APOS");
        assert_eq!(name(0x000c_00e9), "C_VOL_UP");
        assert_eq!(name(0x000c_00cd), "C_PP");
        assert_eq!(name(0x0007_002c), "SPACE");
        assert_eq!(name(0x0007_0065), "K_APP");
    }

    #[test]
    fn modifiers_in_the_high_byte() {
        // Shifted symbols keep their own names.
        assert_eq!(name(value("LBRC").unwrap()), "LBRC");
        assert_eq!(name(0x0207_002f), "LBRC");
        assert_eq!(name(0x0207_001e), "EXCL");
        // Otherwise the modifier functions, outermost = highest bit.
        assert_eq!(name(0x0807_0018), "LG(U)");
        assert_eq!(name(0x0a07_000b), "LG(LS(H))");
        assert_eq!(name(0x0b07_000b), "LG(LS(LC(H)))");
        assert_eq!(name(0x0207_0007), "LS(D)");
        assert_eq!(name(0x4007_0004), "RA(A)");
        assert_eq!(legend(0x0a07_000b), "SG+H");
    }

    #[test]
    fn unknown_values_stay_numeric() {
        assert_eq!(name(0x0007_7777), "0x77777");
        assert_eq!(name(0x0a07_7777), "0xa077777");
        assert_eq!(name(0), "0x0");
        assert_eq!(name(u32::MAX), "0xffffffff");
        assert_eq!(legend(0x0007_7777), "0x77777");
    }

    #[test]
    fn legends() {
        for (n, l) in [
            ("N1", "1"),
            ("LBRC", "{"),
            ("C_VOL_UP", "VOL+"),
            ("LSHFT", "SHIFT"),
            ("KP_N7", "KP7"),
            ("KP_MULTIPLY", "KP*"),
            ("F11", "F11"),
            ("LEFT", "←"),
            ("C_AL_CALC", "AL_CALC"),
        ] {
            assert_eq!(legend(value(n).unwrap()), l, "{n}");
        }
    }

    #[test]
    fn the_table_is_sorted_and_round_trips() {
        assert!(PREFERRED.windows(2).all(|w| w[0].0 < w[1].0));
        for &(v, n) in PREFERRED {
            assert_eq!(value(n), Some(v), "{n}");
        }
    }
}
