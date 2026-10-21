//! The keymap model shared by `keymap show`, `keymap diff` and, later, the
//! TUI view (#76), editing (#77) and snapshots (#78). Pure data and pure
//! functions: the reads live in [`crate::commands::keymap`].
//!
//! - [`Behavior`]: one firmware behaviour, from `get_behavior_details`,
//!   with its **devicetree label** (`kp`, `mo`, `hml`) resolved by
//!   [`Labels`]. The firmware only sends a display name: `display-name` for
//!   ZMK's own behaviours ("Key Press"), the `label` property for macros
//!   ("PIPE_OP" for `M_PIPE`), else the node name ("home_row_mod_left" for
//!   `hml`). The built-in table covers ZMK's; the `.keymap` maps the rest.
//! - [`render`]: a binding `{behavior_id, param1, param2}` as ZMK source
//!   (`&kp A`, `&mo 2`, `&hml LALT S`), its parameters typed from the
//!   behaviour's metadata (HID usage, layer, constant), and a short
//!   [`Legend`] for grids. The ZMK form is what #78's snapshots store for
//!   custom behaviours (`{"type":"raw","zmk":"&hml LALT S"}`).
//! - [`dts`]: a tolerant parser for the `.keymap` (devicetree subset plus
//!   `#define`s), and [`diff`]: live against file, both sides normalised to
//!   values (so `LSHIFT` = `LSHFT`, `LBRC` = `LS(LBKT)`).

pub mod diff;
pub mod dts;
mod expr;
pub mod keys;
mod keys_table;
pub mod render;

use std::collections::HashMap;

use serde::Serialize;

use crate::proto::behaviors as pb;

pub use render::{Legend, Param, Rendered};

/// One behaviour on the running firmware.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Behavior {
    /// The firmware's local id: what bindings carry. It can change between
    /// builds, so snapshots store the label instead (#78).
    pub id: u32,
    pub display_name: String,
    /// The devicetree label (`kp`, `hml`, `M_PIPE`), or `null` when neither
    /// the built-in table nor the `.keymap` knows the display name and it
    /// isn't a usable identifier itself.
    pub label: Option<String>,
    /// The parameter sets the behaviour accepts, as the firmware describes
    /// them (empty for behaviours without parameters, or without metadata).
    pub metadata: Vec<ParamSet>,
}

/// One accepted combination: `param1` is one of these values, and with it
/// `param2` one of those (none: the behaviour takes one parameter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamSet {
    pub param1: Vec<ParamValue>,
    pub param2: Vec<ParamValue>,
}

/// `zmk.behaviors.BehaviorParameterValueDescription`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamValue {
    Nil {
        name: Option<String>,
    },
    Constant {
        name: Option<String>,
        value: u32,
    },
    /// `max < min` means any value (mouse move/scroll send `0..-1`).
    Range {
        name: Option<String>,
        min: i32,
        max: i32,
    },
    HidUsage {
        name: Option<String>,
        keyboard_max: u32,
        consumer_max: u32,
    },
    LayerId {
        name: Option<String>,
    },
}

/// How a parameter renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A key: `A`, `LS(N1)`.
    Hid,
    /// A layer: `2`, shown by name in legends.
    Layer,
    /// Anything else, as a number (or a name the behaviour's own table
    /// knows: `BT_SEL`, `RGB_SAI`, `LCLK`).
    Number,
}

impl ParamValue {
    fn accepts(&self, v: u32) -> bool {
        match *self {
            ParamValue::Nil { .. } => v == 0,
            ParamValue::Constant { value, .. } => value == v,
            ParamValue::Range { min, max, .. } => {
                max < min || (i64::from(min)..=i64::from(max)).contains(&i64::from(v as i32))
            }
            ParamValue::HidUsage { .. } | ParamValue::LayerId { .. } => true,
        }
    }

    fn kind(&self) -> Option<Kind> {
        match self {
            ParamValue::Nil { .. } => None,
            ParamValue::HidUsage { .. } => Some(Kind::Hid),
            ParamValue::LayerId { .. } => Some(Kind::Layer),
            ParamValue::Constant { .. } | ParamValue::Range { .. } => Some(Kind::Number),
        }
    }

    /// The metadata's name for this value (`"Select Profile"`), if any.
    pub fn name(&self) -> Option<&str> {
        match self {
            ParamValue::Nil { name }
            | ParamValue::Constant { name, .. }
            | ParamValue::Range { name, .. }
            | ParamValue::HidUsage { name, .. }
            | ParamValue::LayerId { name } => name.as_deref(),
        }
    }

    fn from_pb(d: pb::BehaviorParameterValueDescription) -> Self {
        use pb::behavior_parameter_value_description::ValueType;
        let name = (!d.name.is_empty()).then_some(d.name);
        match d.value_type {
            Some(ValueType::Constant(value)) => ParamValue::Constant { name, value },
            Some(ValueType::Range(r)) => ParamValue::Range {
                name,
                min: r.min,
                max: r.max,
            },
            Some(ValueType::HidUsage(h)) => ParamValue::HidUsage {
                name,
                keyboard_max: h.keyboard_max,
                consumer_max: h.consumer_max,
            },
            Some(ValueType::LayerId(_)) => ParamValue::LayerId { name },
            Some(ValueType::Nil(_)) | None => ParamValue::Nil { name },
        }
    }
}

impl Behavior {
    pub fn from_details(d: pb::GetBehaviorDetailsResponse, labels: &Labels) -> Self {
        let label = labels.resolve(&d.display_name);
        Behavior {
            id: d.id,
            label,
            display_name: d.display_name,
            metadata: d
                .metadata
                .into_iter()
                .map(|s| ParamSet {
                    param1: s.param1.into_iter().map(ParamValue::from_pb).collect(),
                    param2: s.param2.into_iter().map(ParamValue::from_pb).collect(),
                })
                .collect(),
        }
    }

    /// The parameter kinds for a binding whose first parameter is `p1`
    /// (`None`: the behaviour doesn't take it). The set whose `param1`
    /// accepts `p1` decides (Bluetooth's `BT_SEL` takes a profile, its
    /// `BT_CLR` nothing).
    pub fn kinds(&self, p1: u32) -> [Option<Kind>; 2] {
        let set = self
            .metadata
            .iter()
            .find(|s| s.param1.iter().any(|v| v.accepts(p1)))
            .or_else(|| self.metadata.first());
        let Some(set) = set else {
            return [None, None];
        };
        let k1 = set
            .param1
            .iter()
            .find(|v| v.accepts(p1))
            .or(set.param1.first())
            .and_then(ParamValue::kind);
        let k2 = set.param2.first().and_then(ParamValue::kind);
        [k1, k2]
    }

    /// The metadata name of `param1`'s value (`"Select Profile"`).
    pub fn param1_name(&self, p1: u32) -> Option<&str> {
        self.metadata
            .iter()
            .flat_map(|s| &s.param1)
            .find(|v| matches!(v, ParamValue::Constant { value, .. } if *value == p1))
            .and_then(ParamValue::name)
    }

    /// The label if known, else a placeholder made from the display name
    /// (lowercase, `_` for anything not alphanumeric).
    pub fn label_or_placeholder(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| sanitize(&self.display_name))
    }
}

fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".into()
    } else {
        out
    }
}

/// ZMK's own behaviours: display name (or the node name, where ZMK sets no
/// `display-name`) → devicetree label.
const BUILTIN_LABELS: &[(&str, &str)] = &[
    ("Key Press", "kp"),
    ("Momentary Layer", "mo"),
    ("Layer-Tap", "lt"),
    ("Mod-Tap", "mt"),
    ("Transparent", "trans"),
    ("None", "none"),
    ("To Layer", "to"),
    ("Toggle Layer", "tog"),
    ("Bluetooth", "bt"),
    ("Output Selection", "out"),
    ("Underglow", "rgb_ug"),
    ("Backlight", "bl"),
    ("External Power", "ext_power"),
    ("Caps Word", "caps_word"),
    ("Mouse Key Press", "mkp"),
    ("Sticky Key", "sk"),
    ("Sticky Layer", "sl"),
    ("Key Repeat", "key_repeat"),
    ("Key Toggle", "kt"),
    ("Grave/Escape", "gresc"),
    ("Reset", "sys_reset"),
    ("Bootloader", "bootloader"),
    ("Studio Unlock", "studio_unlock"),
    ("Soft Off", "soft_off"),
    ("Mouse Move", "mmv"),
    ("Mouse Scroll", "msc"),
    // Node names (no display-name in ZMK's dtsi).
    ("z_so_off", "soft_off"),
    ("mouse_move", "mmv"),
    ("mouse_scroll", "msc"),
];

/// Display name → devicetree label: the built-in table, plus what a parsed
/// `.keymap` adds (each labelled node's name, `label` and `display-name`).
#[derive(Debug, Clone, Default)]
pub struct Labels {
    from_file: HashMap<String, String>,
}

impl Labels {
    /// Only the built-in table (no `.keymap` at hand).
    pub fn builtin() -> Self {
        Self::default()
    }

    /// The built-in table plus the file's labelled nodes.
    pub fn with_file(file: &dts::KeymapFile) -> Self {
        Self {
            from_file: file.behavior_names.clone(),
        }
    }

    /// The label for a display name. The file wins over the built-in
    /// table; an identifier-like display name that neither knows is taken
    /// as the label (node name = label is the common case: `rsr_vol`).
    pub fn resolve(&self, display_name: &str) -> Option<String> {
        if let Some(l) = self.from_file.get(display_name) {
            return Some(l.clone());
        }
        if let Some(&(_, l)) = BUILTIN_LABELS.iter().find(|(d, _)| *d == display_name) {
            return Some(l.to_string());
        }
        is_identifier(display_name).then(|| display_name.to_string())
    }
}

pub(crate) fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// A layer name for parameters and legends: `index`, `id`, `name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerName {
    pub id: u32,
    pub index: usize,
    pub name: String,
}

/// What rendering needs besides the binding: the layers (layer parameters
/// are ids on the keyboard, indices in a `.keymap`), and the mouse-move and
/// scroll steps the `.keymap` sets (ZMK's `ZMK_POINTING_DEFAULT_*_VAL`).
#[derive(Debug, Clone)]
pub struct Context {
    pub layers: Vec<LayerName>,
    /// Layer parameters are ids (live) rather than indices (file).
    pub layer_params_are_ids: bool,
    pub move_val: i32,
    pub scrl_val: i32,
}

/// ZMK's defaults when the `.keymap` doesn't override them.
pub const DEFAULT_MOVE_VAL: i32 = 600;
pub const DEFAULT_SCRL_VAL: i32 = 10;

impl Context {
    pub fn new(layers: Vec<LayerName>, layer_params_are_ids: bool) -> Self {
        Self {
            layers,
            layer_params_are_ids,
            move_val: DEFAULT_MOVE_VAL,
            scrl_val: DEFAULT_SCRL_VAL,
        }
    }

    /// The layer a layer parameter points at.
    pub fn layer(&self, v: u32) -> Option<&LayerName> {
        if self.layer_params_are_ids {
            self.layers.iter().find(|l| l.id == v)
        } else {
            self.layers.iter().find(|l| l.index == v as usize)
        }
    }
}

/// A layer as the keyboard has it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layer {
    /// Where it is in the keymap now (what `&mo` in a `.keymap` counts).
    pub index: usize,
    /// The firmware's layer id: stable when Studio moves layers.
    pub id: u32,
    pub name: String,
    pub bindings: Vec<Binding>,
}

/// One key's binding: the raw `{behavior_id, param1, param2}`, decoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub position: usize,
    pub behavior: BehaviorRef,
    pub param1: u32,
    pub param2: u32,
    pub params: Vec<Param>,
    pub zmk: String,
    pub legend: Legend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BehaviorRef {
    pub id: i32,
    /// The devicetree label, `null` when unknown (see [`Behavior::label`]).
    pub name: Option<String>,
    /// `null` when the firmware has no such behaviour.
    pub display_name: Option<String>,
}

/// The label a binding renders with: the behaviour's, its placeholder, or
/// `behavior_<id>` when the firmware didn't describe that id.
pub fn binding_label(id: i32, b: Option<&Behavior>) -> String {
    match b {
        Some(b) => b.label_or_placeholder(),
        None => format!("behavior_{id}"),
    }
}

/// Decodes `get_keymap`'s layers. `behaviors` holds the details fetched
/// for the ids in use; `ctx` the layer names (by id) and pointing steps.
pub fn decode_layers(
    layers: Vec<crate::proto::keymap::Layer>,
    behaviors: &HashMap<u32, Behavior>,
    ctx: &Context,
) -> Vec<Layer> {
    layers
        .into_iter()
        .enumerate()
        .map(|(index, l)| Layer {
            index,
            id: l.id,
            name: l.name,
            bindings: l
                .bindings
                .into_iter()
                .enumerate()
                .map(|(position, b)| {
                    let beh = u32::try_from(b.behavior_id)
                        .ok()
                        .and_then(|id| behaviors.get(&id));
                    let label = binding_label(b.behavior_id, beh);
                    let r = render::render(&label, beh, b.param1, b.param2, ctx);
                    Binding {
                        position,
                        behavior: BehaviorRef {
                            id: b.behavior_id,
                            name: beh.and_then(|x| x.label.clone()),
                            display_name: beh.map(|x| x.display_name.clone()),
                        },
                        param1: b.param1,
                        param2: b.param2,
                        legend: render::legend(&r),
                        params: r.params,
                        zmk: r.zmk,
                    }
                })
                .collect(),
        })
        .collect()
}

/// The [`Context`] for live layers (layer parameters are ids).
pub fn live_context(
    layers: &[crate::proto::keymap::Layer],
    move_val: i32,
    scrl_val: i32,
) -> Context {
    let mut c = Context::new(
        layers
            .iter()
            .enumerate()
            .map(|(index, l)| LayerName {
                id: l.id,
                index,
                name: l.name.clone(),
            })
            .collect(),
        true,
    );
    c.move_val = move_val;
    c.scrl_val = scrl_val;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_from_builtin_and_identifiers() {
        let l = Labels::builtin();
        assert_eq!(l.resolve("Key Press").as_deref(), Some("kp"));
        assert_eq!(l.resolve("z_so_off").as_deref(), Some("soft_off"));
        assert_eq!(l.resolve("mouse_move").as_deref(), Some("mmv"));
        assert_eq!(l.resolve("rsr_vol").as_deref(), Some("rsr_vol"));
        assert_eq!(l.resolve("Something Else"), None);
        assert!(is_identifier("_a1") && !is_identifier("1a") && !is_identifier(""));
        assert_eq!(sanitize("Something Else"), "something_else");
    }

    #[test]
    fn kinds_follow_the_matching_set() {
        let set = |p1: Vec<ParamValue>, p2: Vec<ParamValue>| ParamSet {
            param1: p1,
            param2: p2,
        };
        let c = |v| ParamValue::Constant {
            name: None,
            value: v,
        };
        let bt = Behavior {
            id: 32,
            display_name: "Bluetooth".into(),
            label: Some("bt".into()),
            metadata: vec![
                set(vec![c(0), c(1), c(2), c(4)], vec![]),
                set(
                    vec![c(3), c(5)],
                    vec![ParamValue::Range {
                        name: None,
                        min: 0,
                        max: 4,
                    }],
                ),
            ],
        };
        assert_eq!(bt.kinds(3), [Some(Kind::Number), Some(Kind::Number)]);
        assert_eq!(bt.kinds(0), [Some(Kind::Number), None]);
        // A value no set names still renders, as a number.
        assert_eq!(bt.kinds(9), [Some(Kind::Number), None]);
        let none = Behavior {
            id: 10,
            display_name: "None".into(),
            label: Some("none".into()),
            metadata: vec![],
        };
        assert_eq!(none.kinds(0), [None, None]);
    }
}
