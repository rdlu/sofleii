//! A small C integer-expression evaluator for `.keymap` binding cells:
//! what `cpp` would compute for `LG(LS(H))`, `MOVE_Y(-1200)`, `BT_SEL_CMD`
//! or `(1 << 3)`. Identifiers resolve through the `#define`s (the file's,
//! then [`BUILTIN_DEFINES`] from ZMK's dt-bindings headers), then ZMK's key
//! names; the function-like macros ZMK's headers define are built in.
//! Anything else is an error message, never a panic.

use std::collections::HashMap;

use super::keys;

/// Object-like macros from `dt-bindings/zmk/{bt,rgb,outputs,backlight,
/// ext_power,pointing,mouse,modifiers}.h` that bindings use. Several expand
/// to two cells (`BT_CLR` = `BT_CLR_CMD 0`), which the cell splitter
/// handles; here only their numeric parts matter.
pub const BUILTIN_DEFINES: &[(&str, &str)] = &[
    ("BT_CLR_CMD", "0"),
    ("BT_NXT_CMD", "1"),
    ("BT_PRV_CMD", "2"),
    ("BT_SEL_CMD", "3"),
    ("BT_CLR_ALL_CMD", "4"),
    ("BT_DISC_CMD", "5"),
    ("BT_CLR", "BT_CLR_CMD 0"),
    ("BT_NXT", "BT_NXT_CMD 0"),
    ("BT_PRV", "BT_PRV_CMD 0"),
    ("BT_SEL", "BT_SEL_CMD"),
    ("BT_CLR_ALL", "BT_CLR_ALL_CMD 0"),
    ("BT_DISC", "BT_DISC_CMD"),
    ("RGB_TOG_CMD", "0"),
    ("RGB_ON_CMD", "1"),
    ("RGB_OFF_CMD", "2"),
    ("RGB_HUI_CMD", "3"),
    ("RGB_HUD_CMD", "4"),
    ("RGB_SAI_CMD", "5"),
    ("RGB_SAD_CMD", "6"),
    ("RGB_BRI_CMD", "7"),
    ("RGB_BRD_CMD", "8"),
    ("RGB_SPI_CMD", "9"),
    ("RGB_SPD_CMD", "10"),
    ("RGB_EFF_CMD", "11"),
    ("RGB_EFR_CMD", "12"),
    ("RGB_EFS_CMD", "13"),
    ("RGB_COLOR_HSB_CMD", "14"),
    ("RGB_TOG", "RGB_TOG_CMD 0"),
    ("RGB_ON", "RGB_ON_CMD 0"),
    ("RGB_OFF", "RGB_OFF_CMD 0"),
    ("RGB_HUI", "RGB_HUI_CMD 0"),
    ("RGB_HUD", "RGB_HUD_CMD 0"),
    ("RGB_SAI", "RGB_SAI_CMD 0"),
    ("RGB_SAD", "RGB_SAD_CMD 0"),
    ("RGB_BRI", "RGB_BRI_CMD 0"),
    ("RGB_BRD", "RGB_BRD_CMD 0"),
    ("RGB_SPI", "RGB_SPI_CMD 0"),
    ("RGB_SPD", "RGB_SPD_CMD 0"),
    ("RGB_EFF", "RGB_EFF_CMD 0"),
    ("RGB_EFR", "RGB_EFR_CMD 0"),
    ("OUT_TOG", "0"),
    ("OUT_USB", "1"),
    ("OUT_BLE", "2"),
    ("OUT_NONE", "3"),
    ("BL_ON_CMD", "0"),
    ("BL_OFF_CMD", "1"),
    ("BL_TOG_CMD", "2"),
    ("BL_INC_CMD", "3"),
    ("BL_DEC_CMD", "4"),
    ("BL_CYCLE_CMD", "5"),
    ("BL_SET_CMD", "6"),
    ("BL_ON", "BL_ON_CMD 0"),
    ("BL_OFF", "BL_OFF_CMD 0"),
    ("BL_TOG", "BL_TOG_CMD 0"),
    ("BL_INC", "BL_INC_CMD 0"),
    ("BL_DEC", "BL_DEC_CMD 0"),
    ("BL_CYCLE", "BL_CYCLE_CMD 0"),
    ("BL_SET", "BL_SET_CMD"),
    ("EXT_POWER_OFF_CMD", "0"),
    ("EXT_POWER_ON_CMD", "1"),
    ("EXT_POWER_TOGGLE_CMD", "2"),
    ("EP_OFF", "0"),
    ("EP_ON", "1"),
    ("EP_TOG", "2"),
    ("MB1", "1"),
    ("MB2", "2"),
    ("MB3", "4"),
    ("MB4", "8"),
    ("MB5", "16"),
    ("LCLK", "1"),
    ("RCLK", "2"),
    ("MCLK", "4"),
    ("ZMK_POINTING_DEFAULT_MOVE_VAL", "600"),
    ("ZMK_POINTING_DEFAULT_SCRL_VAL", "10"),
    ("MOVE_UP", "MOVE_Y(-ZMK_POINTING_DEFAULT_MOVE_VAL)"),
    ("MOVE_DOWN", "MOVE_Y(ZMK_POINTING_DEFAULT_MOVE_VAL)"),
    ("MOVE_LEFT", "MOVE_X(-ZMK_POINTING_DEFAULT_MOVE_VAL)"),
    ("MOVE_RIGHT", "MOVE_X(ZMK_POINTING_DEFAULT_MOVE_VAL)"),
    ("SCRL_UP", "MOVE_Y(ZMK_POINTING_DEFAULT_SCRL_VAL)"),
    ("SCRL_DOWN", "MOVE_Y(-ZMK_POINTING_DEFAULT_SCRL_VAL)"),
    ("SCRL_LEFT", "MOVE_X(-ZMK_POINTING_DEFAULT_SCRL_VAL)"),
    ("SCRL_RIGHT", "MOVE_X(ZMK_POINTING_DEFAULT_SCRL_VAL)"),
    ("MOD_LCTL", "0x01"),
    ("MOD_LSFT", "0x02"),
    ("MOD_LALT", "0x04"),
    ("MOD_LGUI", "0x08"),
    ("MOD_RCTL", "0x10"),
    ("MOD_RSFT", "0x20"),
    ("MOD_RALT", "0x40"),
    ("MOD_RGUI", "0x80"),
];

/// The `#define`s in scope: the file's (which win) over the built-in ones.
#[derive(Debug, Clone, Default)]
pub struct Defines {
    pub file: HashMap<String, String>,
}

impl Defines {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.file.get(name).map(String::as_str).or_else(|| {
            BUILTIN_DEFINES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|&(_, v)| v)
        })
    }
}

/// Macro expansion depth before giving up (a define that expands to itself).
const MAX_DEPTH: usize = 16;

/// Evaluates one cell expression to the 32-bit value the firmware gets.
pub fn eval(src: &str, defines: &Defines) -> Result<u32, String> {
    eval_depth(src, defines, 0).map(|v| v as u32)
}

fn eval_depth(src: &str, defines: &Defines, depth: usize) -> Result<i64, String> {
    if depth > MAX_DEPTH {
        return Err(format!("macro expansion too deep in `{src}`"));
    }
    let toks = tokenize(src)?;
    let mut p = Parser {
        toks: &toks,
        i: 0,
        defines,
        depth,
    };
    let v = p.expr(0)?;
    if p.i != toks.len() {
        return Err(format!("unexpected `{}` in `{src}`", toks[p.i].text()));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    Op(&'static str),
}

impl Tok {
    fn text(&self) -> String {
        match self {
            Tok::Num(n) => n.to_string(),
            Tok::Ident(s) => s.clone(),
            Tok::Op(o) => o.to_string(),
        }
    }
}

const OPS: [&str; 15] = [
    "<<", ">>", "(", ")", ",", "+", "-", "*", "/", "%", "&", "|", "^", "~", "!",
];

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric()) {
                i += 1;
            }
            let lit = s[start..i].trim_end_matches(['u', 'U', 'l', 'L']);
            let n = if let Some(h) = lit.strip_prefix("0x").or_else(|| lit.strip_prefix("0X")) {
                i64::from_str_radix(h, 16)
            } else if lit.len() > 1 && lit.starts_with('0') {
                i64::from_str_radix(&lit[1..], 8)
            } else {
                lit.parse()
            }
            .map_err(|_| format!("bad number `{}`", &s[start..i]))?;
            out.push(Tok::Num(n));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(s[start..i].to_string()));
        } else if let Some(op) = OPS.iter().find(|o| s[i..].starts_with(**o)) {
            out.push(Tok::Op(op));
            i += op.len();
        } else {
            return Err(format!(
                "unexpected `{}` in `{s}`",
                s[i..].chars().next().unwrap_or('?')
            ));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: &'a [Tok],
    i: usize,
    defines: &'a Defines,
    depth: usize,
}

/// Binary operators and their precedence (C's, higher binds tighter).
fn binary(op: &str) -> Option<u8> {
    Some(match op {
        "*" | "/" | "%" => 10,
        "+" | "-" => 9,
        "<<" | ">>" => 8,
        "&" => 5,
        "^" => 4,
        "|" => 3,
        _ => return None,
    })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn expect(&mut self, op: &str) -> Result<(), String> {
        match self.peek() {
            Some(Tok::Op(o)) if *o == op => {
                self.i += 1;
                Ok(())
            }
            other => Err(format!(
                "expected `{op}`, got {}",
                other.map_or("the end".into(), |t| format!("`{}`", t.text()))
            )),
        }
    }

    fn expr(&mut self, min: u8) -> Result<i64, String> {
        let mut lhs = self.unary()?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            let Some(prec) = binary(op).filter(|&p| p >= min) else {
                break;
            };
            self.i += 1;
            let rhs = self.expr(prec + 1)?;
            lhs = match op {
                "*" => lhs.wrapping_mul(rhs),
                "/" | "%" if rhs == 0 => return Err("division by zero".into()),
                "/" => lhs.wrapping_div(rhs),
                "%" => lhs.wrapping_rem(rhs),
                "+" => lhs.wrapping_add(rhs),
                "-" => lhs.wrapping_sub(rhs),
                "<<" => lhs.wrapping_shl(rhs as u32),
                ">>" => lhs.wrapping_shr(rhs as u32),
                "&" => lhs & rhs,
                "^" => lhs ^ rhs,
                _ => lhs | rhs,
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<i64, String> {
        match self.peek().cloned() {
            Some(Tok::Op("-")) => {
                self.i += 1;
                Ok(self.unary()?.wrapping_neg())
            }
            Some(Tok::Op("+")) => {
                self.i += 1;
                self.unary()
            }
            Some(Tok::Op("~")) => {
                self.i += 1;
                Ok(!self.unary()?)
            }
            Some(Tok::Op("!")) => {
                self.i += 1;
                Ok(i64::from(self.unary()? == 0))
            }
            Some(Tok::Op("(")) => {
                self.i += 1;
                let v = self.expr(0)?;
                self.expect(")")?;
                Ok(v)
            }
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(n)
            }
            Some(Tok::Ident(name)) => {
                self.i += 1;
                if self.peek() == Some(&Tok::Op("(")) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::Op(")")) {
                        loop {
                            args.push(self.expr(0)?);
                            if self.peek() == Some(&Tok::Op(",")) {
                                self.i += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(")")?;
                    call(&name, &args)
                } else {
                    self.ident(&name)
                }
            }
            Some(t) => Err(format!("unexpected `{}`", t.text())),
            None => Err("unexpected end of expression".into()),
        }
    }

    fn ident(&self, name: &str) -> Result<i64, String> {
        if let Some(text) = self.defines.get(name) {
            return eval_depth(text, self.defines, self.depth + 1);
        }
        keys::value(name)
            .map(i64::from)
            .ok_or_else(|| format!("unknown name `{name}`"))
    }
}

/// The function-like macros of ZMK's dt-bindings headers.
fn call(name: &str, args: &[i64]) -> Result<i64, String> {
    let want = |n: usize| {
        if args.len() == n {
            Ok(())
        } else {
            Err(format!(
                "{name}() takes {n} argument(s), got {}",
                args.len()
            ))
        }
    };
    if let Some(bit) = keys::modifier_bit(name) {
        want(1)?;
        return Ok(i64::from(bit) << 24 | args[0]);
    }
    match name {
        "APPLY_MODS" => {
            want(2)?;
            Ok(args[0] << 24 | args[1])
        }
        "ZMK_HID_USAGE" => {
            want(2)?;
            Ok(args[0] << 16 | args[1])
        }
        "MOVE_X" => {
            want(1)?;
            Ok((args[0] & 0xffff) << 16)
        }
        "MOVE_Y" => {
            want(1)?;
            Ok(args[0] & 0xffff)
        }
        "MOVE" => {
            want(2)?;
            Ok(((args[0] & 0xffff) << 16) + (args[1] & 0xffff))
        }
        "BIT" => {
            want(1)?;
            Ok(1i64.wrapping_shl(args[0] as u32))
        }
        "RGB_COLOR_HSB_VAL" | "RGB_COLOR_HSV_VAL" => {
            want(3)?;
            Ok((args[0] << 16) + (args[1] << 8) + args[2])
        }
        _ => Err(format!("unknown macro `{name}()`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> Result<u32, String> {
        eval(s, &Defines::default())
    }

    #[test]
    fn numbers_and_operators() {
        assert_eq!(ev("2"), Ok(2));
        assert_eq!(ev("0x1F"), Ok(31));
        assert_eq!(ev("(1 << 3) | 1"), Ok(9));
        assert_eq!(ev("-1"), Ok(u32::MAX));
        assert_eq!(ev("2 + 3 * 4"), Ok(14));
        assert_eq!(ev("10U"), Ok(10));
        assert!(ev("1 +").is_err());
        assert!(ev("1 / 0").is_err());
        assert!(ev("(1").is_err());
        assert!(ev("1 2").is_err());
        assert!(ev("$").is_err());
    }

    #[test]
    fn keys_modifiers_and_zmk_macros() {
        assert_eq!(ev("A"), Ok(0x0007_0004));
        assert_eq!(ev("LG(LS(H))"), Ok(0x0a07_000b));
        assert_eq!(ev("LS(LBKT)"), ev("LBRC"));
        assert_eq!(ev("BT_SEL_CMD"), Ok(3));
        assert_eq!(ev("MOVE_UP"), Ok(0xfda8)); // -600 & 0xffff
        assert_eq!(ev("MOVE_X(-1200)"), Ok(0xfb50_0000));
        assert_eq!(ev("RGB_COLOR_HSB_VAL(120,100,50)"), Ok(0x0078_6432));
        assert_eq!(ev("BIT(4)"), Ok(16));
        assert_eq!(ev("NOPE"), Err("unknown name `NOPE`".into()));
        assert!(ev("NOPE(1)").unwrap_err().contains("unknown macro"));
        assert!(ev("LS(A, B)").unwrap_err().contains("takes 1"));
    }

    #[test]
    fn file_defines_win_and_recursion_stops() {
        let mut d = Defines::default();
        d.file
            .insert("ZMK_POINTING_DEFAULT_MOVE_VAL".into(), "1200".into());
        d.file.insert("HYPER".into(), "LC(LS(LA(LGUI)))".into());
        d.file.insert("LOOP".into(), "LOOP".into());
        assert_eq!(eval("MOVE_UP", &d), Ok(0xfb50));
        assert_eq!(eval("HYPER", &d), Ok(0x0707_00e3));
        assert!(eval("LOOP", &d).unwrap_err().contains("too deep"));
    }
}
