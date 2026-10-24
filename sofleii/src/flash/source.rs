//! Firmware to flash (#100): where it comes from, and which file is which.
//!
//! A set of `.uf2` files becomes a [`Bundle`] by **content**, not by name:
//! each file is parsed and identified like a chip ([`super::identify`]), so
//! the left half's file is the one whose identity record (or, before #98,
//! whose strings) says left. Artifact names differ between CI
//! (`eyelash_sofle_studio_left`, `nice_view-eyelash_sofle_right-zmk`) and
//! local builds (`eyelash_sofle_left.uf2`), and a renamed file can't
//! smuggle the wrong half in.
//!
//! The network side lives in [`crate::commands::flash::source`]; this module
//! only reads GitHub's JSON, unpacks artifact zips and classifies files.

use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};

use super::identify::{Firmware, Half, Identity, KEYBOARD};
use super::{identify, uf2};

/// Where a bundle came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Origin {
    /// A published `sofleii-fw-v*` release.
    Release { tag: String },
    /// A successful "Build ZMK firmware" run's `firmware` artifact.
    Ci {
        run_id: u64,
        branch: Option<String>,
        commit: String,
        /// ISO 8601 UTC.
        created_at: Option<String>,
    },
    /// Local files (`mise run build-*`).
    Files,
}

/// What a firmware file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Left,
    Right,
    SettingsReset,
}

impl Role {
    pub fn half(self) -> Option<Half> {
        match self {
            Role::Left => Some(Half::Left),
            Role::Right => Some(Half::Right),
            Role::SettingsReset => None,
        }
    }
}

impl From<Half> for Role {
    fn from(h: Half) -> Self {
        match h {
            Half::Left => Role::Left,
            Half::Right => Role::Right,
        }
    }
}

/// One firmware file, identified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Uf2File {
    pub name: String,
    pub role: Role,
    /// What the file's own content says (`sofleii` with a record, `legacy`
    /// for releases before #98, `settings_reset`).
    pub identity: Identity,
    #[serde(skip)]
    pub bytes: Vec<u8>,
    pub size: usize,
}

/// The firmware for one flash: whichever of the three files the source has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bundle {
    pub origin: Origin,
    pub left: Option<Uf2File>,
    pub right: Option<Uf2File>,
    pub settings_reset: Option<Uf2File>,
    /// Files skipped, and why.
    pub notes: Vec<String>,
}

impl Bundle {
    pub fn get(&self, role: Role) -> Option<&Uf2File> {
        match role {
            Role::Left => self.left.as_ref(),
            Role::Right => self.right.as_ref(),
            Role::SettingsReset => self.settings_reset.as_ref(),
        }
    }

    /// Classifies `files` (name, content). Anything that isn't a UF2 is
    /// skipped silently (a release's keymap PDF); a UF2 that is neither half
    /// of this keyboard nor settings_reset is skipped with a note. Two files
    /// for one role is an error: there's no telling which was meant.
    pub fn from_files(origin: Origin, files: Vec<(String, Vec<u8>)>) -> Result<Bundle, String> {
        let mut b = Bundle {
            origin,
            left: None,
            right: None,
            settings_reset: None,
            notes: Vec::new(),
        };
        for (name, bytes) in files {
            if !name.to_ascii_lowercase().ends_with(".uf2") {
                continue;
            }
            let f = match classify(&name, bytes) {
                Ok(f) => f,
                Err(why) => {
                    b.notes.push(format!("skipped {name}: {why}"));
                    continue;
                }
            };
            let slot = match f.role {
                Role::Left => &mut b.left,
                Role::Right => &mut b.right,
                Role::SettingsReset => &mut b.settings_reset,
            };
            if let Some(prev) = slot {
                return Err(format!(
                    "both {} and {} are {} firmware",
                    prev.name,
                    f.name,
                    role_name(f.role)
                ));
            }
            *slot = Some(f);
        }
        Ok(b)
    }
}

pub fn role_name(r: Role) -> &'static str {
    match r {
        Role::Left => "left-half",
        Role::Right => "right-half",
        Role::SettingsReset => "settings_reset",
    }
}

/// Identifies one `.uf2` by content.
pub fn classify(name: &str, bytes: Vec<u8>) -> Result<Uf2File, String> {
    let img = uf2::parse(&bytes).map_err(|e| format!("not a valid UF2 ({e})"))?;
    let identity = identify::identify(&img);
    let role = match (
        identity.firmware,
        identity.keyboard.as_deref(),
        identity.half,
    ) {
        (Firmware::SettingsReset, _, _) => Role::SettingsReset,
        (Firmware::Sofleii | Firmware::Legacy, Some(KEYBOARD), Some(h)) => h.into(),
        (_, Some(k), _) if k != KEYBOARD => {
            return Err(format!("firmware for another keyboard ({k})"));
        }
        _ => return Err("not firmware for either half of this keyboard".into()),
    };
    Ok(Uf2File {
        name: name.to_string(),
        role,
        identity,
        size: bytes.len(),
        bytes,
    })
}

/// The files in a zip (a CI artifact), flattened to their base names.
/// Directories are skipped; each entry is capped at 4 MiB (a UF2 of the
/// whole 1 MiB flash is 2 MiB).
pub fn unzip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    const MAX: u64 = 4 << 20;
    let mut z = zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("bad zip: {e}"))?;
    let mut out = Vec::new();
    for i in 0..z.len() {
        let f = z
            .by_index(i)
            .map_err(|e| format!("bad zip entry {i}: {e}"))?;
        if f.is_dir() {
            continue;
        }
        let full = f
            .name()
            .map_err(|e| format!("bad zip entry {i} name: {e}"))?;
        let name = full.rsplit('/').next().unwrap_or_default().to_string();
        if f.size() > MAX {
            return Err(format!(
                "{name} is {} bytes, too large for firmware",
                f.size()
            ));
        }
        let mut bytes = Vec::with_capacity(f.size() as usize);
        f.take(MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("unpacking {name}: {e}"))?;
        out.push((name, bytes));
    }
    Ok(out)
}

/// GitHub's names for the pieces sofleii reads.
pub const WORKFLOW: &str = "build.yml";
pub const ARTIFACT: &str = "firmware";

/// Percent-encodes a path segment (a branch like `feat/x`).
pub fn encode(segment: &str) -> String {
    let mut s = String::with_capacity(segment.len());
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// `GET` a release by tag.
pub fn release_path(repo: &str, tag: &str) -> String {
    format!("/repos/{repo}/releases/tags/{}", encode(tag))
}

/// `GET` the newest successful build run on `branch`.
pub fn runs_path(repo: &str, branch: &str) -> String {
    format!(
        "/repos/{repo}/actions/workflows/{WORKFLOW}/runs?branch={}&status=success&per_page=1",
        encode(branch)
    )
}

/// `GET` one run.
pub fn run_path(repo: &str, id: u64) -> String {
    format!("/repos/{repo}/actions/runs/{id}")
}

/// `GET` a run's artifacts.
pub fn artifacts_path(repo: &str, id: u64) -> String {
    format!("/repos/{repo}/actions/runs/{id}/artifacts")
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    assets: Vec<Asset>,
}

/// A release's `.uf2` assets.
pub fn release_assets(body: &str) -> Result<Vec<Asset>, String> {
    let r: ReleaseJson = serde_json::from_str(body).map_err(|e| format!("release JSON: {e}"))?;
    Ok(r.assets
        .into_iter()
        .filter(|a| a.name.to_ascii_lowercase().ends_with(".uf2"))
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Run {
    pub id: u64,
    pub head_sha: String,
    pub head_branch: Option<String>,
    pub conclusion: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunsJson {
    workflow_runs: Vec<Run>,
}

/// The newest run in a `runs` list, if any.
pub fn newest_run(body: &str) -> Result<Option<Run>, String> {
    let r: RunsJson = serde_json::from_str(body).map_err(|e| format!("runs JSON: {e}"))?;
    Ok(r.workflow_runs.into_iter().next())
}

pub fn run(body: &str) -> Result<Run, String> {
    serde_json::from_str(body).map_err(|e| format!("run JSON: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub archive_download_url: String,
    pub expired: bool,
}

#[derive(Debug, Deserialize)]
struct ArtifactsJson {
    artifacts: Vec<Artifact>,
}

/// The run's merged `firmware` artifact. Err explains a missing or expired
/// one (GitHub keeps artifacts 90 days by default).
pub fn firmware_artifact(body: &str) -> Result<Artifact, String> {
    let a: ArtifactsJson =
        serde_json::from_str(body).map_err(|e| format!("artifacts JSON: {e}"))?;
    match a.artifacts.into_iter().find(|a| a.name == ARTIFACT) {
        Some(a) if a.expired => Err(format!("the run's `{ARTIFACT}` artifact has expired")),
        Some(a) => Ok(a),
        None => Err(format!("the run has no `{ARTIFACT}` artifact")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::flash::uf2::encode as uf2;

    pub const LEFT_RESET: u32 = 0x35605;
    pub const RIGHT_RESET: u32 = 0x2be31;

    /// A firmware image as a built `.uf2` lays it out (from 0x26000), with
    /// an identity record for `half`, or the legacy strings when `record`
    /// is false.
    pub fn firmware(half: Half, record: bool) -> Vec<u8> {
        let reset = match half {
            Half::Left => LEFT_RESET,
            Half::Right => RIGHT_RESET,
        };
        let mut b = vec![0x5A; 4096];
        b[4..8].copy_from_slice(&reset.to_le_bytes());
        if record {
            let mut r = Vec::new();
            r.extend_from_slice(b"SOFLEII-ID/1\0\0\0\0");
            r.extend_from_slice(&reset.to_le_bytes());
            let mut kb = [0u8; 16];
            kb[..KEYBOARD.len()].copy_from_slice(KEYBOARD.as_bytes());
            r.extend_from_slice(&kb);
            let mut h = [0u8; 8];
            h[..half.as_str().len()].copy_from_slice(half.as_str().as_bytes());
            r.extend_from_slice(&h);
            b[0x800..0x800 + r.len()].copy_from_slice(&r);
        } else {
            let role = match half {
                Half::Left => "ble_central",
                Half::Right => "ble_peripheral",
            };
            for (at, s) in [(0xE00, "Eyelash Sofle"), (0xF00, role)] {
                b[at] = 0;
                b[at + 1..at + 1 + s.len()].copy_from_slice(s.as_bytes());
                b[at + 1 + s.len()] = 0;
            }
        }
        uf2(0x26000, &b, 0xADA5_2840)
    }

    pub fn settings_reset() -> Vec<u8> {
        let mut b = vec![0x5A; 1024];
        b[4..8].copy_from_slice(&0x2001u32.to_le_bytes());
        let s = b"\0settings_reset_kscan\0";
        b[0x100..0x100 + s.len()].copy_from_slice(s);
        uf2(0x26000, &b, 0xADA5_2840)
    }

    pub fn release_files(record: bool) -> Vec<(String, Vec<u8>)> {
        vec![
            (
                "eyelash_sofle_studio_left.uf2".into(),
                firmware(Half::Left, record),
            ),
            (
                "nice_view-eyelash_sofle_right-zmk.uf2".into(),
                firmware(Half::Right, record),
            ),
            ("settings_reset.uf2".into(), settings_reset()),
            ("sofleii-keymap.pdf".into(), b"%PDF".to_vec()),
        ]
    }

    #[test]
    fn bundles_by_content_not_name() {
        let b = Bundle::from_files(Origin::Files, release_files(true)).unwrap();
        assert_eq!(
            b.left.as_ref().unwrap().name,
            "eyelash_sofle_studio_left.uf2"
        );
        assert_eq!(b.right.as_ref().unwrap().role, Role::Right);
        assert_eq!(b.settings_reset.as_ref().unwrap().role, Role::SettingsReset);
        assert_eq!(
            b.left.as_ref().unwrap().identity.firmware,
            Firmware::Sofleii
        );
        assert!(b.notes.is_empty(), "{:?}", b.notes);
        // A right-half build named like the left one is still the right half.
        let swapped = vec![(
            "eyelash_sofle_left.uf2".to_string(),
            firmware(Half::Right, true),
        )];
        let b = Bundle::from_files(Origin::Files, swapped).unwrap();
        assert!(b.left.is_none());
        assert_eq!(b.get(Role::Right).unwrap().name, "eyelash_sofle_left.uf2");
        // Releases before #98 by their strings.
        let b = Bundle::from_files(
            Origin::Release {
                tag: "sofleii-fw-v3.2.0".into(),
            },
            release_files(false),
        )
        .unwrap();
        assert_eq!(b.left.as_ref().unwrap().identity.firmware, Firmware::Legacy);
        assert_eq!(b.right.as_ref().unwrap().role, Role::Right);
    }

    #[test]
    fn skips_and_refuses() {
        let files = vec![
            ("junk.uf2".to_string(), vec![0u8; 512]),
            ("blank.uf2".to_string(), uf2(0x26000, &[0xFF; 512], 0)),
            ("a.uf2".to_string(), firmware(Half::Left, true)),
        ];
        let b = Bundle::from_files(Origin::Files, files).unwrap();
        assert_eq!(b.notes.len(), 2, "{:?}", b.notes);
        assert!(b.notes[0].contains("not a valid UF2"), "{:?}", b.notes);
        assert!(b.notes[1].contains("either half"), "{:?}", b.notes);
        let twice = vec![
            ("a.uf2".to_string(), firmware(Half::Left, true)),
            ("b.uf2".to_string(), firmware(Half::Left, false)),
        ];
        let e = Bundle::from_files(Origin::Files, twice).unwrap_err();
        assert!(e.contains("a.uf2 and b.uf2 are left-half"), "{e}");
    }

    #[test]
    fn unzips_an_artifact() {
        use std::io::Write;
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            z.start_file("eyelash_sofle_studio_left.uf2", opts).unwrap();
            z.write_all(&firmware(Half::Left, true)).unwrap();
            z.add_directory("sub/", opts).unwrap();
            z.start_file("sub/settings_reset.uf2", opts).unwrap();
            z.write_all(&settings_reset()).unwrap();
            z.finish().unwrap();
        }
        let files = unzip(buf.get_ref()).unwrap();
        let names: Vec<_> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["eyelash_sofle_studio_left.uf2", "settings_reset.uf2"]
        );
        let b = Bundle::from_files(Origin::Files, files).unwrap();
        assert!(b.left.is_some() && b.settings_reset.is_some() && b.right.is_none());
        assert!(unzip(b"not a zip").is_err());
    }

    #[test]
    fn github_json() {
        assert_eq!(encode("feat/flash-core"), "feat%2Fflash-core");
        assert_eq!(
            runs_path("rdlu/sofleii", "feat/x"),
            "/repos/rdlu/sofleii/actions/workflows/build.yml/runs?branch=feat%2Fx&status=success&per_page=1"
        );
        let assets = release_assets(
            r#"{"tag_name":"sofleii-fw-v3.2.0","assets":[
                {"name":"eyelash_sofle_studio_left.uf2","browser_download_url":"https://github.com/x/l.uf2","size":1},
                {"name":"sofleii-keymap.pdf","browser_download_url":"https://github.com/x/k.pdf","size":1}]}"#,
        )
        .unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].browser_download_url, "https://github.com/x/l.uf2");
        let r = newest_run(
            r#"{"total_count":1,"workflow_runs":[{"id":40137344164,"head_sha":"5a7f788aa","head_branch":"feat/flash-identify","conclusion":"success","created_at":"2026-10-24T22:10:00Z"}]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (r.id, r.head_branch.as_deref()),
            (40137344164, Some("feat/flash-identify"))
        );
        assert_eq!(
            newest_run(r#"{"total_count":0,"workflow_runs":[]}"#).unwrap(),
            None
        );
        let body = |expired: bool| {
            format!(
                r#"{{"artifacts":[{{"name":"eyelash_sofle_studio_left","archive_download_url":"u1","expired":false}},
                   {{"name":"firmware","archive_download_url":"u2","expired":{expired}}}]}}"#
            )
        };
        assert_eq!(
            firmware_artifact(&body(false))
                .unwrap()
                .archive_download_url,
            "u2"
        );
        assert!(
            firmware_artifact(&body(true))
                .unwrap_err()
                .contains("expired")
        );
        assert!(
            firmware_artifact(r#"{"artifacts":[]}"#)
                .unwrap_err()
                .contains("no `firmware`")
        );
    }
}
