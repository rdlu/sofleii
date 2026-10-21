//! `sofleii keymap show` and `keymap diff` (#76): the live keymap over
//! Studio RPC, decoded with [`crate::keymap`], and its drift from
//! `config/eyelash_sofle.keymap`. Read-only: `get_keymap`,
//! `get_physical_layouts`, `check_unsaved_changes`, `list_all_behaviors`
//! and `get_behavior_details`; nothing here writes to the keyboard.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::client::Client;
use crate::error::exit;
use crate::keymap::diff::{self, Diff};
use crate::keymap::dts::{self, KeymapFile};
use crate::keymap::{self, Behavior, Labels, Layer};
use crate::transport::Transport;
use crate::{Error, Result};

/// Where the repo keeps the keymap, relative to its root.
pub const KEYMAP_FILE: &str = "config/eyelash_sofle.keymap";

/// A `.keymap` read and parsed.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub path: PathBuf,
    pub file: KeymapFile,
}

/// [`KEYMAP_FILE`] under `start` or the nearest ancestor that has one.
pub fn find_keymap(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|d| d.join(KEYMAP_FILE))
        .find(|p| p.is_file())
}

/// Loads `explicit` (`--keymap`), else the [`find_keymap`] one above the
/// current directory. Problems with an explicit or `required` file are
/// usage errors (exit 3); otherwise they're a warning and `None` (`keymap
/// show` only uses the file for behaviour labels).
pub fn load(explicit: Option<&Path>, required: bool) -> Result<Option<Loaded>> {
    let strict = required || explicit.is_some();
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| Error::io("current directory", e))?;
            match find_keymap(&cwd) {
                Some(p) => p,
                None if required => {
                    return Err(Error::Usage(format!(
                        "no {KEYMAP_FILE} in {} or above it; pass --keymap <path>",
                        cwd.display()
                    )));
                }
                None => return Ok(None),
            }
        }
    };
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|src| dts::parse(&src));
    match parsed {
        Ok(file) => Ok(Some(Loaded { path, file })),
        Err(e) if strict => Err(Error::Usage(format!("{}: {e}", path.display()))),
        Err(e) => {
            log::warn!(
                "{}: {e}; showing behaviours by display name",
                path.display()
            );
            Ok(None)
        }
    }
}

/// The keymap on the keyboard now.
#[derive(Debug, Clone, Serialize)]
pub struct Live {
    pub layers: Vec<Layer>,
    /// The behaviours the bindings use, by id.
    pub behaviors: Vec<Behavior>,
    /// The active physical layout.
    pub physical_layout: Option<PhysicalLayout>,
    /// Free layer slots Studio could add.
    pub available_layers: u32,
    pub max_layer_name_length: u32,
    /// Studio edits applied but not saved; `null` when the firmware didn't
    /// say. (Saved edits are what `keymap diff` finds.)
    pub unsaved_changes: Option<bool>,
}

/// `zmk.keymap.PhysicalLayout`, positions numbered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhysicalLayout {
    pub index: u32,
    pub name: String,
    pub keys: Vec<Key>,
}

/// One key's geometry, in ZMK's units: 100 = 1u for sizes and positions,
/// rotation `r` in hundredths of a degree around (`rx`, `ry`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Key {
    pub position: usize,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub r: i32,
    pub rx: i32,
    pub ry: i32,
}

/// Behaviour details, fetched once per id per run.
pub struct BehaviorCache {
    labels: Labels,
    listed: Option<BTreeSet<u32>>,
    details: HashMap<u32, Behavior>,
}

impl BehaviorCache {
    pub fn new(labels: Labels) -> Self {
        Self {
            labels,
            listed: None,
            details: HashMap::new(),
        }
    }

    /// Fetches the details of every id in `ids` the firmware lists (once:
    /// `list_all_behaviors` is asked the first time). An id it doesn't
    /// list, or whose details fail with an RPC error, stays unknown and
    /// renders as `&behavior_<id>`; transport errors are errors.
    pub fn fetch<T: Transport>(
        &mut self,
        client: &mut Client<T>,
        ids: impl IntoIterator<Item = u32>,
    ) -> Result<()> {
        if self.listed.is_none() {
            self.listed = Some(client.list_all_behaviors()?.into_iter().collect());
        }
        let listed = self.listed.as_ref().expect("set above");
        let wanted: BTreeSet<u32> = ids
            .into_iter()
            .filter(|id| listed.contains(id) && !self.details.contains_key(id))
            .collect();
        for id in wanted {
            match client.get_behavior_details(id) {
                Ok(d) => {
                    self.details
                        .insert(id, Behavior::from_details(d, &self.labels));
                }
                Err(e @ Error::Rpc(_)) => log::warn!("behaviour {id}: {e}"),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn map(&self) -> &HashMap<u32, Behavior> {
        &self.details
    }
}

/// Reads the live keymap, its layout and the behaviours it uses. `file`
/// supplies behaviour labels and the mouse step values, when there is one.
pub fn read<T: Transport>(client: &mut Client<T>, file: Option<&KeymapFile>) -> Result<Live> {
    read_cached(client, file).map(|(live, _)| live)
}

/// [`read`], also returning the behaviour cache (for the diff).
fn read_cached<T: Transport>(
    client: &mut Client<T>,
    file: Option<&KeymapFile>,
) -> Result<(Live, BehaviorCache)> {
    let km = client.get_keymap()?;
    let layouts = client.get_physical_layouts()?;
    let unsaved_changes = match client.check_unsaved_changes() {
        Ok(b) => Some(b),
        Err(Error::Rpc(e)) => {
            log::debug!("check_unsaved_changes: {e}");
            None
        }
        Err(e) => return Err(e),
    };
    let labels = file.map_or_else(Labels::builtin, Labels::with_file);
    let mut cache = BehaviorCache::new(labels);
    let ids = km
        .layers
        .iter()
        .flat_map(|l| &l.bindings)
        .filter_map(|b| u32::try_from(b.behavior_id).ok());
    cache.fetch(client, ids)?;
    let (move_val, scrl_val) = file
        .map_or((keymap::DEFAULT_MOVE_VAL, keymap::DEFAULT_SCRL_VAL), |f| {
            (f.move_val, f.scrl_val)
        });
    let ctx = keymap::live_context(&km.layers, move_val, scrl_val);
    let layers = keymap::decode_layers(km.layers, cache.map(), &ctx);
    let mut behaviors: Vec<Behavior> = cache.map().values().cloned().collect();
    behaviors.sort_by_key(|b| b.id);
    let active = layouts.active_layout_index;
    let physical_layout =
        layouts
            .layouts
            .into_iter()
            .nth(active as usize)
            .map(|l| PhysicalLayout {
                index: active,
                name: l.name,
                keys: l
                    .keys
                    .into_iter()
                    .enumerate()
                    .map(|(position, k)| Key {
                        position,
                        x: k.x,
                        y: k.y,
                        width: k.width,
                        height: k.height,
                        r: k.r,
                        rx: k.rx,
                        ry: k.ry,
                    })
                    .collect(),
            });
    let live = Live {
        layers,
        behaviors,
        physical_layout,
        available_layers: km.available_layers,
        max_layer_name_length: km.max_layer_name_length,
        unsaved_changes,
    };
    Ok((live, cache))
}

/// `keymap show`.
#[derive(Debug, Clone, Serialize)]
pub struct Show {
    /// The `.keymap` behaviour labels came from, `null` if none.
    pub keymap_file: Option<String>,
    #[serde(flatten)]
    pub live: Live,
    pub notes: Vec<String>,
}

/// The layer `sel` names: an index (`1`) or a name, any case (`nav`).
pub fn select_layer(layers: &[Layer], sel: &str) -> Result<usize> {
    let by_index = sel
        .parse::<usize>()
        .ok()
        .filter(|&i| layers.iter().any(|l| l.index == i));
    by_index
        .or_else(|| {
            layers
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(sel))
                .map(|l| l.index)
        })
        .ok_or_else(|| {
            let names: Vec<String> = layers
                .iter()
                .map(|l| format!("{} {}", l.index, l.name))
                .collect();
            Error::Usage(format!(
                "no layer \"{sel}\"; the keyboard has: {}",
                names.join(", ")
            ))
        })
}

pub fn show<T: Transport>(
    client: &mut Client<T>,
    file: Option<&Loaded>,
    layer: Option<&str>,
) -> Result<Show> {
    let mut live = read(client, file.map(|l| &l.file))?;
    if let Some(sel) = layer {
        let i = select_layer(&live.layers, sel)?;
        live.layers.retain(|l| l.index == i);
    }
    let mut notes = Vec::new();
    if file.is_none() {
        notes.push(format!(
            "no {KEYMAP_FILE} found: custom behaviours show by the firmware's names \
             (pass --keymap for their labels)"
        ));
    }
    let unknown: BTreeSet<String> = live
        .layers
        .iter()
        .flat_map(|l| &l.bindings)
        .filter(|b| b.behavior.name.is_none())
        .map(|b| match &b.behavior.display_name {
            Some(d) => format!("\"{d}\" (id {})", b.behavior.id),
            None => format!("id {}", b.behavior.id),
        })
        .collect();
    if !unknown.is_empty() {
        notes.push(format!(
            "behaviours without a known label: {}",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(Show {
        keymap_file: file.map(|l| l.path.display().to_string()),
        live,
        notes,
    })
}

/// `keymap diff`.
#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    pub keymap_file: String,
    pub unsaved_changes: Option<bool>,
    #[serde(flatten)]
    pub diff: Diff,
    /// The parser's notes (what it skipped or couldn't evaluate).
    pub notes: Vec<String>,
}

impl DiffReport {
    /// 0 without drift, 1 with (like `update check` when behind).
    pub fn exit_code(&self) -> u8 {
        if self.diff.drift {
            exit::CHECK_FAILED
        } else {
            exit::OK
        }
    }
}

pub fn diff<T: Transport>(client: &mut Client<T>, loaded: &Loaded) -> Result<DiffReport> {
    let (live, cache) = read_cached(client, Some(&loaded.file))?;
    Ok(DiffReport {
        keymap_file: loaded.path.display().to_string(),
        unsaved_changes: live.unsaved_changes,
        diff: diff::diff(&live.layers, cache.map(), &loaded.file),
        notes: loaded.file.notes.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ResponseSubsystem;
    use crate::proto::studio;
    use crate::transport::mock::Mock;
    use prost::Message;
    use std::time::Duration;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/keymap_live.hex"
    ));
    /// The `.keymap` the capture was taken with, frozen beside it, so
    /// editing `config/eyelash_sofle.keymap` never breaks these tests
    /// (`keymap::dts`'s tests parse the live file).
    const KEYMAP: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/keymap_live.keymap"
    ));

    /// The captured replies by request name, as `RequestResponse` payloads.
    fn replies(name: &str) -> Vec<ResponseSubsystem> {
        FIXTURE
            .lines()
            .filter_map(|l| l.split_once(' '))
            .filter(|(n, _)| *n == name)
            .map(|(_, hex)| {
                let b: Vec<u8> = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                    .collect();
                match studio::Response::decode(b.as_slice()).unwrap().r#type {
                    Some(studio::response::Type::RequestResponse(r)) => r.subsystem.unwrap(),
                    other => panic!("{other:?}"),
                }
            })
            .collect()
    }

    /// The replies `read` asks for, in its order: only the details of the
    /// ids the keymap uses, ascending.
    fn mock() -> Mock {
        use crate::proto::{behaviors as pb, keymap as kpb};
        let mut r = replies("get_keymap");
        let ResponseSubsystem::Keymap(kpb::Response {
            response_type: Some(kpb::response::ResponseType::GetKeymap(km)),
        }) = &r[0]
        else {
            panic!()
        };
        let used: BTreeSet<u32> = km
            .layers
            .iter()
            .flat_map(|l| &l.bindings)
            .map(|b| b.behavior_id as u32)
            .collect();
        r.extend(replies("get_physical_layouts"));
        r.extend(replies("check_unsaved_changes"));
        r.extend(replies("list_all_behaviors"));
        r.extend(replies("get_behavior_details").into_iter().filter(|d| {
            matches!(d, ResponseSubsystem::Behaviors(pb::Response {
                response_type: Some(pb::response::ResponseType::GetBehaviorDetails(x)),
            }) if used.contains(&x.id))
        }));
        Mock::echoing(r)
    }

    fn file() -> Loaded {
        Loaded {
            path: KEYMAP_FILE.into(),
            file: dts::parse(KEYMAP).unwrap(),
        }
    }

    #[test]
    fn show_decodes_the_captured_keymap() {
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let f = file();
        let s = show(&mut c, Some(&f), None).unwrap();
        let l = &s.live.layers;
        assert_eq!(l.len(), 10);
        assert_eq!(l[0].name, "BASE");
        assert_eq!(l[0].bindings.len(), 64);
        let zmk = |layer: usize, pos: usize| l[layer].bindings[pos].zmk.as_str();
        assert_eq!(zmk(0, 0), "&kp GRAVE");
        assert_eq!(zmk(0, 28), "&hml LALT S");
        assert_eq!(zmk(0, 30), "&hml_idx LSHFT F");
        assert_eq!(zmk(0, 53), "&mo 4");
        assert_eq!(zmk(0, 61), "&caps_word");
        assert_eq!(zmk(0, 62), "&ralt_mt RALT F12");
        assert_eq!(zmk(1, 39), "&kp LG(LC(U))");
        assert_eq!(zmk(1, 41), "&kp LG(LS(LC(H)))");
        assert_eq!(zmk(1, 15), "&trans");
        assert_eq!(zmk(2, 1), "&M_SIGS");
        assert_eq!(zmk(2, 27), "&kp LBRC");
        assert_eq!(zmk(2, 28), "&mt LALT RBRC");
        assert_eq!(zmk(4, 1), "&bt BT_SEL 0");
        assert_eq!(zmk(4, 0), "&bt BT_CLR");
        assert_eq!(zmk(4, 14), "&rgb_ug RGB_SAI");
        assert_eq!(zmk(4, 6), "&mmv MOVE_UP");
        assert_eq!(zmk(4, 30), "&mkp LCLK");
        assert_eq!(zmk(4, 63), "&to 5");
        assert_eq!(zmk(5, 6), "&GAME_MC_TO");
        assert_eq!(zmk(5, 0), "&none");
        assert_eq!(s.live.unsaved_changes, Some(false));
        assert_eq!(s.live.physical_layout.as_ref().unwrap().keys.len(), 64);
        assert!(s.notes.is_empty(), "{:?}", s.notes);
        assert_eq!(s.keymap_file.as_deref(), Some(KEYMAP_FILE));
    }

    #[test]
    fn the_captured_keymap_matches_the_file() {
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let d = diff(&mut c, &file()).unwrap();
        assert!(!d.diff.drift, "{:#?}", d.diff.differences);
        assert_eq!(d.exit_code(), 0);
        assert_eq!(d.diff.layers.len(), 10);
    }

    #[test]
    fn layer_selection() {
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let f = file();
        let s = show(&mut c, Some(&f), Some("nav")).unwrap();
        assert_eq!(s.live.layers.len(), 1);
        assert_eq!(s.live.layers[0].name, "NAV");
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let s = show(&mut c, Some(&f), Some("4")).unwrap();
        assert_eq!(s.live.layers[0].name, "SYS|NUM");
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let e = show(&mut c, Some(&f), Some("NOPE")).unwrap_err();
        assert_eq!(e.exit_code(), exit::USAGE);
        assert!(e.to_string().contains("1 NAV"), "{e}");
    }

    #[test]
    fn without_the_file_labels_fall_back_to_firmware_names() {
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let s = show(&mut c, None, Some("CODE")).unwrap();
        let b = &s.live.layers[0].bindings;
        // M_PIPE's macro `label` is what the firmware reports.
        assert_eq!(b[16].zmk, "&PIPE_OP");
        assert_eq!(s.notes.len(), 1, "{:?}", s.notes);
    }

    #[test]
    fn json_shape_has_explicit_nulls() {
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let f = file();
        let s = show(&mut c, Some(&f), Some("BASE")).unwrap();
        let v = serde_json::to_value(&s).unwrap();
        let b = &v["layers"][0]["bindings"][28];
        assert_eq!(b["zmk"], "&hml LALT S");
        assert_eq!(b["behavior"]["name"], "hml");
        assert_eq!(b["behavior"]["display_name"], "home_row_mod_left");
        assert_eq!(b["params"][0]["zmk"], "LALT");
        assert_eq!(b["params"][0]["kind"], "hid_usage");
        assert!(b["params"][0]["name"].is_null());
        assert_eq!(b["legend"]["tap"], "S");
        let b = &v["layers"][0]["bindings"][0];
        assert!(b["legend"]["hold"].is_null());
        for k in [
            "keymap_file",
            "layers",
            "behaviors",
            "physical_layout",
            "unsaved_changes",
            "notes",
        ] {
            assert!(v.get(k).is_some(), "{k}");
        }
        let mut c = Client::new(mock(), Duration::from_secs(1));
        let v = serde_json::to_value(diff(&mut c, &f).unwrap()).unwrap();
        for k in [
            "keymap_file",
            "unsaved_changes",
            "drift",
            "layers",
            "differences",
            "added",
            "removed",
            "reordered",
            "renamed",
            "notes",
        ] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn finds_the_keymap_upwards() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let found = find_keymap(&root.join("src/keymap")).unwrap();
        assert!(found.ends_with(KEYMAP_FILE));
        assert_eq!(find_keymap(Path::new("/")), None);
    }

    #[test]
    fn transport_errors_are_exit_4() {
        let mut c = Client::new(Mock::new(Default::default()), Duration::from_millis(20));
        let e = read(&mut c, None).unwrap_err();
        assert_eq!(e.exit_code(), exit::TRANSPORT);
    }
}
