//! Drift between the live keymap and a parsed `.keymap`.
//!
//! Both sides are normalised before comparing: a binding is its label plus
//! its parameter **values** (so `&kp LSHIFT` = `&kp LSHFT`, `&kp LBRC` =
//! `&kp LS(LBKT)`, `&bt BT_SEL 0` = `3 0`), and a layer parameter is the
//! **layer pair** it points at (on the keyboard it's a layer id, in the
//! file an index; a reordered or renamed layer keeps its `&mo`s equal).
//! Layers are paired by name, then leftover ones by index (a Studio rename).

use std::collections::HashMap;

use serde::Serialize;

use super::dts::{FileBinding, FileParam, KeymapFile};
use super::render::{self, kinds};
use super::{Behavior, Context, Kind, Layer, LayerName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// Anything below differs.
    pub drift: bool,
    /// Every paired layer, in the keyboard's order.
    pub layers: Vec<LayerPair>,
    /// Keys that differ, layer by layer.
    pub differences: Vec<Difference>,
    /// On the keyboard, not in the file.
    pub added: Vec<LayerRef>,
    /// In the file, not on the keyboard.
    pub removed: Vec<LayerRef>,
    /// Paired layers whose order relative to another pair changed.
    pub reordered: Vec<Moved>,
    /// Layers paired by index whose names differ.
    pub renamed: Vec<Renamed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerPair {
    pub name: String,
    pub file_index: usize,
    pub live_index: usize,
    pub live_id: u32,
    /// How many of its keys differ.
    pub differences: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    /// The layer's name on the keyboard.
    pub layer: String,
    pub live_index: usize,
    pub position: usize,
    /// The file's binding in ZMK form (`null`: the file has no key here).
    pub file: Option<String>,
    /// The keyboard's (`null`: the keyboard has no key here).
    pub live: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerRef {
    pub index: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moved {
    pub name: String,
    pub file_index: usize,
    pub live_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Renamed {
    pub file_name: String,
    pub live_name: String,
    pub file_index: usize,
    pub live_index: usize,
}

/// A parameter value, comparable across both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Norm {
    Num(u32),
    /// The layer pointed at: its file index when paired, else which side
    /// and the raw value.
    Layer(String),
    /// A `.keymap` parameter that didn't evaluate: never equal.
    Unresolved(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Normal {
    label: String,
    params: [Norm; 2],
}

/// Compares `live` (with the behaviours its bindings use) to `file`.
pub fn diff(live: &[Layer], behaviors: &HashMap<u32, Behavior>, file: &KeymapFile) -> Diff {
    let by_label: HashMap<&str, &Behavior> = behaviors
        .values()
        .filter_map(|b| b.label.as_deref().map(|l| (l, b)))
        .collect();
    let live_ctx = Context {
        layers: live
            .iter()
            .map(|l| LayerName {
                id: l.id,
                index: l.index,
                name: l.name.clone(),
            })
            .collect(),
        layer_params_are_ids: true,
        move_val: file.move_val,
        scrl_val: file.scrl_val,
    };
    let file_ctx = Context {
        layers: file
            .layers
            .iter()
            .map(|l| LayerName {
                id: l.index as u32,
                index: l.index,
                name: l.name.clone(),
            })
            .collect(),
        layer_params_are_ids: false,
        move_val: file.move_val,
        scrl_val: file.scrl_val,
    };

    // Pair by name, then what's left by index.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut renamed = Vec::new();
    let mut live_used = vec![false; live.len()];
    let mut file_used = vec![false; file.layers.len()];
    for (fi, f) in file.layers.iter().enumerate() {
        if let Some(li) = live
            .iter()
            .position(|l| l.name == f.name)
            .filter(|&li| !live_used[li])
        {
            pairs.push((fi, li));
            live_used[li] = true;
            file_used[fi] = true;
        }
    }
    for fi in 0..file.layers.len() {
        if !file_used[fi] && fi < live.len() && !live_used[fi] {
            pairs.push((fi, fi));
            file_used[fi] = true;
            live_used[fi] = true;
            renamed.push(Renamed {
                file_name: file.layers[fi].name.clone(),
                live_name: live[fi].name.clone(),
                file_index: fi,
                live_index: fi,
            });
        }
    }
    pairs.sort_by_key(|&(_, li)| li);
    // Layer parameters compare by the pair they point at.
    let live_layer = |v: u32| match live_ctx.layer(v) {
        Some(l) => match pairs.iter().find(|&&(_, li)| li == l.index) {
            Some(&(fi, _)) => fi.to_string(),
            None => format!("live {}", l.name),
        },
        None => format!("live #{v}"),
    };
    let file_layer = |v: u32| {
        if (v as usize) < file.layers.len() {
            v.to_string()
        } else {
            format!("file #{v}")
        }
    };

    let mut layers = Vec::new();
    let mut differences = Vec::new();
    for &(fi, li) in &pairs {
        let (f, l) = (&file.layers[fi], &live[li]);
        let n = f.bindings.len().max(l.bindings.len());
        let mut count = 0;
        for pos in 0..n {
            let lb = l.bindings.get(pos);
            let fb = f.bindings.get(pos);
            let ln = lb.map(|b| {
                let beh = u32::try_from(b.behavior.id)
                    .ok()
                    .and_then(|id| behaviors.get(&id));
                normal_live(b, beh, &live_layer)
            });
            let fnorm =
                fb.map(|b| normal_file(b, by_label.get(b.label.as_str()).copied(), &file_layer));
            if ln == fnorm {
                continue;
            }
            count += 1;
            differences.push(Difference {
                layer: l.name.clone(),
                live_index: li,
                position: pos,
                file: fb.map(|b| file_zmk(b, by_label.get(b.label.as_str()).copied(), &file_ctx)),
                live: lb.map(|b| b.zmk.clone()),
            });
        }
        layers.push(LayerPair {
            name: l.name.clone(),
            file_index: fi,
            live_index: li,
            live_id: l.id,
            differences: count,
        });
    }

    // A pair is reordered when it swapped places with another pair; index
    // shifts from an added or removed layer alone aren't.
    let reordered: Vec<Moved> = pairs
        .iter()
        .filter(|&&(fa, la)| {
            pairs
                .iter()
                .any(|&(fb, lb)| (fa < fb && la > lb) || (fa > fb && la < lb))
        })
        .map(|&(fi, li)| Moved {
            name: live[li].name.clone(),
            file_index: fi,
            live_index: li,
        })
        .collect();
    let added: Vec<LayerRef> = live
        .iter()
        .filter(|l| !live_used[l.index])
        .map(|l| LayerRef {
            index: l.index,
            name: l.name.clone(),
        })
        .collect();
    let removed: Vec<LayerRef> = file
        .layers
        .iter()
        .filter(|l| !file_used[l.index])
        .map(|l| LayerRef {
            index: l.index,
            name: l.name.clone(),
        })
        .collect();
    Diff {
        drift: !(differences.is_empty()
            && added.is_empty()
            && removed.is_empty()
            && reordered.is_empty()
            && renamed.is_empty()),
        layers,
        differences,
        added,
        removed,
        reordered,
        renamed,
    }
}

type LayerKey<'a> = &'a dyn Fn(u32) -> String;

fn norm(kind: Option<Kind>, v: u32, layer: LayerKey) -> Norm {
    match kind {
        Some(Kind::Layer) => Norm::Layer(layer(v)),
        _ => Norm::Num(v),
    }
}

fn normal_live(b: &super::Binding, beh: Option<&Behavior>, layer: LayerKey) -> Normal {
    let label = super::binding_label(b.behavior.id, beh);
    let [k1, k2] = kinds(&label, beh, b.param1, b.param2);
    Normal {
        params: [norm(k1, b.param1, layer), norm(k2, b.param2, layer)],
        label,
    }
}

fn normal_file(b: &FileBinding, beh: Option<&Behavior>, layer: LayerKey) -> Normal {
    let (p1, p2) = (b.value(0), b.value(1));
    let k = kinds(&b.label, beh, p1, p2);
    let param = |i: usize| match b.params.get(i) {
        Some(FileParam::Unresolved(t)) => Norm::Unresolved(t.clone()),
        Some(FileParam::Value(v)) => norm(k[i], *v, layer),
        None => norm(k[i], 0, layer),
    };
    Normal {
        label: b.label.clone(),
        params: [param(0), param(1)],
    }
}

/// The file's binding as the keyboard's would render, or as written when a
/// parameter didn't evaluate.
fn file_zmk(b: &FileBinding, beh: Option<&Behavior>, ctx: &Context) -> String {
    if !b.resolved() {
        return b.source.clone();
    }
    render::render(&b.label, beh, b.value(0), b.value(1), ctx).zmk
}

#[cfg(test)]
mod tests {
    use super::super::dts;
    use super::super::{Labels, ParamSet, ParamValue, decode_layers, keys, live_context};
    use super::*;
    use crate::proto::keymap as pb;

    const FILE: &str = r#"
/ {
    behaviors {
        hml: home_row_mod_left {
            compatible = "zmk,behavior-hold-tap";
            #binding-cells = <2>;
        };
    };
    keymap {
        compatible = "zmk,keymap";
        base { display-name = "BASE"; bindings = <&kp LSHIFT &hml LALT S &mo 1 &kp LBRC>; };
        nav { display-name = "NAV"; bindings = <&trans &trans &to 0 &none>; };
    };
};
"#;

    fn behaviors() -> HashMap<u32, Behavior> {
        let hid = || ParamValue::HidUsage {
            name: None,
            keyboard_max: 255,
            consumer_max: 4095,
        };
        let layer = || ParamValue::LayerId { name: None };
        let set = |p1, p2| {
            vec![ParamSet {
                param1: p1,
                param2: p2,
            }]
        };
        let mk = |id, name: &str, metadata| Behavior {
            id,
            display_name: name.into(),
            label: Labels::builtin().resolve(name),
            metadata,
        };
        [
            mk(1, "Key Press", set(vec![hid()], vec![])),
            mk(2, "home_row_mod_left", set(vec![hid()], vec![hid()])),
            mk(3, "Momentary Layer", set(vec![layer()], vec![])),
            mk(4, "Transparent", vec![]),
            mk(5, "To Layer", set(vec![layer()], vec![])),
            mk(6, "None", vec![]),
        ]
        .into_iter()
        .map(|mut b| {
            if b.display_name == "home_row_mod_left" {
                b.label = Some("hml".into());
            }
            (b.id, b)
        })
        .collect()
    }

    fn k(n: &str) -> u32 {
        keys::value(n).unwrap()
    }

    fn bind(id: i32, p1: u32, p2: u32) -> pb::BehaviorBinding {
        pb::BehaviorBinding {
            behavior_id: id,
            param1: p1,
            param2: p2,
        }
    }

    /// The live keymap matching `FILE`, with layer ids that aren't indices.
    fn live_layers() -> Vec<pb::Layer> {
        vec![
            pb::Layer {
                id: 7,
                name: "BASE".into(),
                bindings: vec![
                    bind(1, k("LSHFT"), 0),
                    bind(2, k("LALT"), k("S")),
                    bind(3, 9, 0),
                    bind(1, 0x0207_002f, 0),
                ],
            },
            pb::Layer {
                id: 9,
                name: "NAV".into(),
                bindings: vec![bind(4, 0, 0), bind(4, 0, 0), bind(5, 7, 0), bind(6, 0, 0)],
            },
        ]
    }

    fn run(layers: Vec<pb::Layer>) -> Diff {
        let file = dts::parse(FILE).unwrap();
        let b = behaviors();
        let ctx = live_context(&layers, 600, 10);
        let live = decode_layers(layers, &b, &ctx);
        diff(&live, &b, &file)
    }

    #[test]
    fn aliases_and_layer_ids_normalise_away() {
        let d = run(live_layers());
        assert!(!d.drift, "{d:#?}");
        assert_eq!(d.layers.len(), 2);
        assert!(d.differences.is_empty());
    }

    #[test]
    fn a_changed_key_is_reported() {
        let mut l = live_layers();
        l[1].bindings[1] = bind(1, k("A"), 0);
        let d = run(l);
        assert!(d.drift);
        assert_eq!(
            d.differences,
            [Difference {
                layer: "NAV".into(),
                live_index: 1,
                position: 1,
                file: Some("&trans".into()),
                live: Some("&kp A".into()),
            }]
        );
        assert_eq!(d.layers[1].differences, 1);
    }

    #[test]
    fn reordered_added_removed_and_renamed_layers() {
        // Swapped: both report as moved, and `&mo`/`&to` still match by name.
        let mut l = live_layers();
        l.swap(0, 1);
        let d = run(l);
        assert!(d.drift && d.differences.is_empty(), "{d:#?}");
        assert_eq!(d.reordered.len(), 2);
        assert_eq!(
            (d.reordered[0].name.as_str(), d.reordered[0].file_index),
            ("NAV", 1)
        );

        // An extra layer on the keyboard: added, nothing reordered.
        let mut l = live_layers();
        l.push(pb::Layer {
            id: 11,
            name: "EXTRA".into(),
            bindings: vec![],
        });
        let d = run(l);
        assert_eq!(
            d.added,
            [LayerRef {
                index: 2,
                name: "EXTRA".into()
            }]
        );
        assert!(d.reordered.is_empty() && d.removed.is_empty());

        // A missing one: removed.
        let mut l = live_layers();
        l.pop();
        let d = run(l);
        assert_eq!(
            d.removed,
            [LayerRef {
                index: 1,
                name: "NAV".into()
            }]
        );

        // Renamed in Studio: paired by index, keys still compared.
        let mut l = live_layers();
        l[1].name = "MOVE".into();
        let d = run(l);
        assert_eq!(d.renamed.len(), 1);
        assert_eq!(d.renamed[0].file_name, "NAV");
        assert!(d.differences.is_empty() && d.drift);
    }

    #[test]
    fn missing_keys_and_unresolved_params() {
        let mut l = live_layers();
        l[0].bindings.pop();
        let d = run(l);
        assert_eq!(d.differences[0].live, None);
        assert_eq!(d.differences[0].file.as_deref(), Some("&kp LBRC"));

        let file = dts::parse(&FILE.replace("&kp LBRC", "&kp NOPE")).unwrap();
        let b = behaviors();
        let layers = live_layers();
        let ctx = live_context(&layers, 600, 10);
        let d = diff(&decode_layers(layers, &b, &ctx), &b, &file);
        assert_eq!(d.differences[0].file.as_deref(), Some("&kp NOPE"));
        assert!(file.notes.iter().any(|n| n.contains("NOPE")));
    }
}
