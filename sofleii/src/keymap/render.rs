//! A binding as ZMK source and as a grid legend.
//!
//! Parameter types come from the behaviour's metadata (`HidUsage` → key
//! name, `LayerId` → layer), with tables for the behaviours whose ZMK
//! source uses named commands the metadata doesn't carry (`&bt BT_SEL 0`,
//! `&rgb_ug RGB_SAI`, `&out OUT_USB`, `&mkp LCLK`, `&mmv MOVE_UP`). A value
//! nothing knows renders as a number; nothing here panics on odd input.

use serde::Serialize;

use super::{Behavior, Context, Kind, keys};

/// A binding rendered: the label, its typed parameters, and the ZMK form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rendered {
    /// Devicetree label (or the placeholder for an unknown behaviour).
    pub label: String,
    /// As many as the behaviour takes for this `param1` (0–2).
    pub params: Vec<Param>,
    /// `&kp A`, `&mo 2`, `&hml LALT S`, `&bt BT_SEL 0`, `&none`.
    pub zmk: String,
}

/// One parameter: the raw value, how it was read, its ZMK token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Param {
    pub value: u32,
    /// `hid_usage`, `layer` or `number`.
    pub kind: &'static str,
    /// The token in the ZMK form: `LALT`, `2`, `BT_SEL`.
    pub zmk: String,
    /// The layer's name for `layer`, the metadata's name for a named
    /// constant (`"Select Profile"`), else `null`.
    pub name: Option<String>,
}

/// A short two-line legend for a grid cell: `tap` on top, `hold` (the
/// hold side of a hold-tap, or what kind of key it is) below.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Legend {
    pub tap: String,
    pub hold: Option<String>,
}

/// Renders `{label, param1, param2}`. `behavior` gives the parameter
/// metadata (the live firmware's); without it, the kinds come from ZMK's
/// built-in behaviours by label.
pub fn render(
    label: &str,
    behavior: Option<&Behavior>,
    p1: u32,
    p2: u32,
    ctx: &Context,
) -> Rendered {
    let params = match special(label, p1, p2, ctx) {
        Some(p) => p,
        None => {
            let [k1, k2] = kinds(label, behavior, p1, p2);
            let mut params = Vec::new();
            if let Some(k) = k1 {
                params.push(param(k, p1, ctx, behavior.and_then(|b| b.param1_name(p1))));
                if let Some(k) = k2 {
                    params.push(param(k, p2, ctx, None));
                }
            }
            params
        }
    };
    let mut zmk = format!("&{label}");
    for p in zmk_tokens(label, &params) {
        zmk.push(' ');
        zmk.push_str(&p);
    }
    Rendered {
        label: label.to_string(),
        params,
        zmk,
    }
}

/// The parameter kinds for a binding: the metadata's, else ZMK's built-in
/// behaviours by label; a non-zero value with no kind is still shown, as
/// a number, so nothing is silently dropped.
pub fn kinds(label: &str, behavior: Option<&Behavior>, p1: u32, p2: u32) -> [Option<Kind>; 2] {
    let mut k = match behavior.filter(|b| !b.metadata.is_empty()) {
        Some(b) => b.kinds(p1),
        None => builtin_kinds(label),
    };
    if k[1].is_none() && p2 != 0 {
        k[1] = Some(Kind::Number);
    }
    if k[0].is_none() && (p1 != 0 || k[1].is_some()) {
        k[0] = Some(Kind::Number);
    }
    k
}

/// ZMK's own behaviours' parameter kinds, for a `.keymap` binding whose
/// behaviour isn't on the keyboard (or no metadata came with it).
fn builtin_kinds(label: &str) -> [Option<Kind>; 2] {
    match label {
        "kp" | "kt" | "sk" => [Some(Kind::Hid), None],
        "mt" => [Some(Kind::Hid), Some(Kind::Hid)],
        "lt" => [Some(Kind::Layer), Some(Kind::Hid)],
        "mo" | "to" | "tog" | "sl" => [Some(Kind::Layer), None],
        _ => [None, None],
    }
}

fn param(kind: Kind, v: u32, ctx: &Context, name: Option<&str>) -> Param {
    match kind {
        Kind::Hid => Param {
            value: v,
            kind: "hid_usage",
            zmk: keys::name(v),
            name: None,
        },
        Kind::Layer => {
            let l = ctx.layer(v);
            Param {
                value: v,
                kind: "layer",
                // A `.keymap` names layers by index; on the keyboard the
                // parameter is the layer id, so show where it is now.
                zmk: l.map_or(v.to_string(), |l| l.index.to_string()),
                name: l.map(|l| l.name.clone()),
            }
        }
        Kind::Number => Param {
            value: v,
            kind: "number",
            zmk: v.to_string(),
            name: name.map(str::to_string),
        },
    }
}

fn named(v: u32, zmk: &str) -> Param {
    Param {
        value: v,
        kind: "number",
        zmk: zmk.to_string(),
        name: None,
    }
}

fn number(v: u32) -> Param {
    named(v, &v.to_string())
}

/// Behaviours whose ZMK source uses named commands (`dt-bindings/zmk/*.h`).
fn special(label: &str, p1: u32, p2: u32, ctx: &Context) -> Option<Vec<Param>> {
    // The command's name, and whether it takes a second cell itself
    // (`BT_SEL 0`) or the name already includes it (`BT_CLR` = `0 0`).
    let cmd = |names: &[(u32, &str, bool)]| -> Vec<Param> {
        match names.iter().find(|(v, _, _)| *v == p1) {
            Some(&(_, n, true)) => vec![named(p1, n), number(p2)],
            Some(&(_, n, false)) if p2 == 0 => vec![named(p1, n)],
            _ if p2 != 0 => vec![number(p1), number(p2)],
            _ => vec![number(p1)],
        }
    };
    Some(match label {
        "bt" => cmd(&[
            (0, "BT_CLR", false),
            (1, "BT_NXT", false),
            (2, "BT_PRV", false),
            (3, "BT_SEL", true),
            (4, "BT_CLR_ALL", false),
            (5, "BT_DISC", true),
        ]),
        "rgb_ug" if p1 == 14 => {
            let (h, s, v) = (p2 >> 16, (p2 >> 8) & 0xff, p2 & 0xff);
            vec![
                named(p1, "RGB_COLOR_HSB_CMD"),
                named(p2, &format!("RGB_COLOR_HSB_VAL({h},{s},{v})")),
            ]
        }
        "rgb_ug" => cmd(&[
            (0, "RGB_TOG", false),
            (1, "RGB_ON", false),
            (2, "RGB_OFF", false),
            (3, "RGB_HUI", false),
            (4, "RGB_HUD", false),
            (5, "RGB_SAI", false),
            (6, "RGB_SAD", false),
            (7, "RGB_BRI", false),
            (8, "RGB_BRD", false),
            (9, "RGB_SPI", false),
            (10, "RGB_SPD", false),
            (11, "RGB_EFF", false),
            (12, "RGB_EFR", false),
            (13, "RGB_EFS_CMD", true),
        ]),
        "bl" => cmd(&[
            (0, "BL_ON", false),
            (1, "BL_OFF", false),
            (2, "BL_TOG", false),
            (3, "BL_INC", false),
            (4, "BL_DEC", false),
            (5, "BL_CYCLE", false),
            (6, "BL_SET", true),
        ]),
        "out" => cmd(&[
            (0, "OUT_TOG", false),
            (1, "OUT_USB", false),
            (2, "OUT_BLE", false),
            (3, "OUT_NONE", false),
        ]),
        "ext_power" => cmd(&[
            (0, "EP_OFF", false),
            (1, "EP_ON", false),
            (2, "EP_TOG", false),
        ]),
        "mkp" => cmd(&[
            (1, "LCLK", false),
            (2, "RCLK", false),
            (4, "MCLK", false),
            (8, "MB4", false),
            (16, "MB5", false),
        ]),
        "mmv" | "msc" => {
            let (x, y) = ((p1 >> 16) as u16 as i16, p1 as u16 as i16);
            let (x, y) = (i32::from(x), i32::from(y));
            let (name, step) = if label == "mmv" {
                (
                    ["MOVE_UP", "MOVE_DOWN", "MOVE_LEFT", "MOVE_RIGHT"],
                    ctx.move_val,
                )
            } else {
                // ZMK's scroll is the other way up: SCRL_UP = MOVE_Y(+n).
                (
                    ["SCRL_DOWN", "SCRL_UP", "SCRL_LEFT", "SCRL_RIGHT"],
                    ctx.scrl_val,
                )
            };
            let t = match (x, y) {
                (0, y) if y == -step => name[0].to_string(),
                (0, y) if y == step => name[1].to_string(),
                (x, 0) if x == -step => name[2].to_string(),
                (x, 0) if x == step => name[3].to_string(),
                (0, y) => format!("MOVE_Y({y})"),
                (x, 0) => format!("MOVE_X({x})"),
                (x, y) => format!("MOVE({x},{y})"),
            };
            let mut v = vec![named(p1, &t)];
            if p2 != 0 {
                v.push(number(p2));
            }
            v
        }
        _ => return None,
    })
}

/// The tokens after `&label`: `RGB_COLOR_HSB(h,s,v)` folds its two cells
/// into one, as ZMK's macro does.
fn zmk_tokens(label: &str, params: &[Param]) -> Vec<String> {
    if label == "rgb_ug"
        && params.first().is_some_and(|p| p.zmk == "RGB_COLOR_HSB_CMD")
        && let Some(v) = params.get(1)
    {
        return vec![v.zmk.replace("RGB_COLOR_HSB_VAL", "RGB_COLOR_HSB")];
    }
    params.iter().map(|p| p.zmk.clone()).collect()
}

/// The grid legend for a rendered binding.
pub fn legend(r: &Rendered) -> Legend {
    let layer = |p: &Param| match &p.name {
        Some(n) if p.kind == "layer" => n.clone(),
        _ => format!("L{}", p.zmk),
    };
    let key = |p: &Param| keys::legend(p.value);
    let label = r.label.as_str();
    let hold = |s: &str| Some(s.to_string());
    match (label, r.params.as_slice()) {
        ("trans", []) => Legend {
            tap: "▽".into(),
            hold: None,
        },
        ("none", []) => Legend {
            tap: String::new(),
            hold: None,
        },
        ("caps_word", []) => Legend {
            tap: "CAPS".into(),
            hold: hold("word"),
        },
        ("kp", [p]) if p.kind == "hid_usage" => Legend {
            tap: key(p),
            hold: None,
        },
        (_, [p]) if p.kind == "hid_usage" => Legend {
            tap: key(p),
            hold: hold(label),
        },
        (_, [a, b]) if a.kind == "hid_usage" && b.kind == "hid_usage" => Legend {
            tap: key(b),
            hold: Some(key(a)),
        },
        (_, [a, b]) if a.kind == "layer" && b.kind == "hid_usage" => Legend {
            tap: key(b),
            hold: Some(layer(a)),
        },
        (_, [p]) if p.kind == "layer" => Legend {
            tap: layer(p),
            hold: hold(label),
        },
        (_, []) => split_name(label),
        (_, params) => {
            let text: Vec<String> = params.iter().map(|p| compact(&p.zmk)).collect();
            Legend {
                tap: text.join(" "),
                hold: hold(label),
            }
        }
    }
}

/// `BT_SEL` → `SEL`, `RGB_SAI` → `SAI`, `MOVE_UP` → `UP`.
fn compact(token: &str) -> String {
    for pre in ["BT_", "RGB_", "OUT_", "BL_", "EP_", "MOVE_", "SCRL_"] {
        if let Some(rest) = token.strip_prefix(pre)
            && !rest.is_empty()
            && !rest.starts_with('(')
        {
            return rest.to_string();
        }
    }
    token.to_string()
}

/// A name with no parameters over two lines of at most [`LEGEND_WIDTH`],
/// split at an underscore where possible (`GAME_MC_TO` → `GAME` /
/// `MC_TO`). A short prefix goes below instead (`M_CONCAT` → `CONCAT` /
/// `M`), so the distinctive part is on top.
fn split_name(name: &str) -> Legend {
    let w = LEGEND_WIDTH;
    if name.chars().count() <= w {
        return Legend {
            tap: name.to_string(),
            hold: None,
        };
    }
    let unders: Vec<usize> = name.match_indices('_').map(|(i, _)| i).collect();
    let split = unders
        .iter()
        .copied()
        .find(|&i| i <= w && name[i + 1..].chars().count() <= w)
        .or_else(|| unders.iter().copied().rfind(|&i| i <= w));
    match split {
        Some(i @ 1..=2) => Legend {
            tap: name[i + 1..].to_string(),
            hold: Some(name[..i].to_string()),
        },
        Some(i) if i > 0 => Legend {
            tap: name[..i].to_string(),
            hold: Some(name[i + 1..].to_string()),
        },
        _ => {
            let cut: String = name.chars().take(w).collect();
            let rest: String = name.chars().skip(w).collect();
            Legend {
                tap: cut,
                hold: Some(rest),
            }
        }
    }
}

/// How wide a legend line is meant to be; grids truncate longer ones.
pub const LEGEND_WIDTH: usize = 6;

#[cfg(test)]
mod tests {
    use super::super::{Labels, LayerName, ParamSet, ParamValue};
    use super::*;

    fn ctx() -> Context {
        let names = ["BASE", "NAV", "CODE", "MEDIA"];
        Context::new(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| LayerName {
                    id: i as u32 + 10,
                    index: i,
                    name: n.to_string(),
                })
                .collect(),
            true,
        )
    }

    fn hid() -> ParamValue {
        ParamValue::HidUsage {
            name: Some("Key".into()),
            keyboard_max: 255,
            consumer_max: 4095,
        }
    }

    fn beh(label: &str, p1: Vec<ParamValue>, p2: Vec<ParamValue>) -> Behavior {
        Behavior {
            id: 1,
            display_name: label.into(),
            label: Labels::builtin().resolve(label),
            metadata: vec![ParamSet {
                param1: p1,
                param2: p2,
            }],
        }
    }

    fn zmk(label: &str, b: Option<&Behavior>, p1: u32, p2: u32) -> String {
        render(label, b, p1, p2, &ctx()).zmk
    }

    #[test]
    fn keys_layers_and_hold_taps() {
        let kp = beh("kp", vec![hid()], vec![]);
        assert_eq!(zmk("kp", Some(&kp), keys::value("A").unwrap(), 0), "&kp A");
        assert_eq!(zmk("kp", Some(&kp), 0x0207_002f, 0), "&kp LBRC");
        let hml = beh("hml", vec![hid()], vec![hid()]);
        let (lalt, s) = (keys::value("LALT").unwrap(), keys::value("S").unwrap());
        let r = render("hml", Some(&hml), lalt, s, &ctx());
        assert_eq!(r.zmk, "&hml LALT S");
        assert_eq!(
            legend(&r),
            Legend {
                tap: "S".into(),
                hold: Some("ALT".into())
            }
        );
        let mo = beh("mo", vec![ParamValue::LayerId { name: None }], vec![]);
        // Layer id 12 is index 2, CODE.
        let r = render("mo", Some(&mo), 12, 0, &ctx());
        assert_eq!(r.zmk, "&mo 2");
        assert_eq!(r.params[0].name.as_deref(), Some("CODE"));
        assert_eq!(legend(&r).tap, "CODE");
        // An id no layer has stays numeric.
        assert_eq!(zmk("mo", Some(&mo), 99, 0), "&mo 99");
        // Without metadata, ZMK's built-in kinds.
        assert_eq!(
            zmk("lt", None, 11, keys::value("SPACE").unwrap()),
            "&lt 1 SPACE"
        );
    }

    #[test]
    fn no_params_and_unknowns() {
        let none = Behavior {
            id: 10,
            display_name: "None".into(),
            label: Some("none".into()),
            metadata: vec![],
        };
        assert_eq!(zmk("none", Some(&none), 0, 0), "&none");
        assert_eq!(zmk("trans", None, 0, 0), "&trans");
        // Stray parameters on a parameterless behaviour are kept.
        assert_eq!(zmk("none", Some(&none), 7, 0), "&none 7");
        assert_eq!(zmk("none", Some(&none), 0, 7), "&none 0 7");
        assert_eq!(zmk("GAME_MC_TO", None, 0, 0), "&GAME_MC_TO");
        let l = legend(&render("GAME_NMS_TO", None, 0, 0, &ctx()));
        assert_eq!(
            (l.tap.as_str(), l.hold.as_deref()),
            ("GAME", Some("NMS_TO"))
        );
        let l = legend(&render("M_CONCAT", None, 0, 0, &ctx()));
        assert_eq!((l.tap.as_str(), l.hold.as_deref()), ("CONCAT", Some("M")));
        let l = legend(&render("M_SIGS", None, 0, 0, &ctx()));
        assert_eq!((l.tap.as_str(), l.hold), ("M_SIGS", None));
        let l = legend(&render("caps_word", None, 0, 0, &ctx()));
        assert_eq!(l.tap, "CAPS");
    }

    #[test]
    fn named_commands() {
        assert_eq!(zmk("bt", None, 3, 0), "&bt BT_SEL 0");
        assert_eq!(zmk("bt", None, 0, 0), "&bt BT_CLR");
        assert_eq!(zmk("bt", None, 9, 1), "&bt 9 1");
        assert_eq!(zmk("rgb_ug", None, 5, 0), "&rgb_ug RGB_SAI");
        assert_eq!(zmk("rgb_ug", None, 13, 0), "&rgb_ug RGB_EFS_CMD 0");
        let hsb = (120 << 16) | (100 << 8) | 50;
        assert_eq!(
            zmk("rgb_ug", None, 14, hsb),
            "&rgb_ug RGB_COLOR_HSB(120,100,50)"
        );
        assert_eq!(zmk("out", None, 1, 0), "&out OUT_USB");
        assert_eq!(zmk("mkp", None, 1, 0), "&mkp LCLK");
        assert_eq!(zmk("mkp", None, 8, 0), "&mkp MB4");
        assert_eq!(zmk("mkp", None, 3, 0), "&mkp 3");
        let mut c = ctx();
        c.move_val = 1200;
        let mmv = |p1| render("mmv", None, p1, 0, &c).zmk;
        assert_eq!(mmv(0xfb50), "&mmv MOVE_UP");
        assert_eq!(mmv(1200), "&mmv MOVE_DOWN");
        assert_eq!(mmv(0xfb50_0000), "&mmv MOVE_LEFT");
        assert_eq!(mmv(1200 << 16), "&mmv MOVE_RIGHT");
        assert_eq!(mmv(600), "&mmv MOVE_Y(600)");
        assert_eq!(mmv((5 << 16) | 7), "&mmv MOVE(5,7)");
        assert_eq!(zmk("msc", None, 10, 0), "&msc SCRL_UP");
        let l = legend(&render("bt", None, 3, 2, &ctx()));
        assert_eq!((l.tap.as_str(), l.hold.as_deref()), ("SEL 2", Some("bt")));
    }
}
