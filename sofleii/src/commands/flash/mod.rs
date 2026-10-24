//! Flashing through the UF2 bootloader drive. `flash identify` (#98):
//! find the drive, read its `INFO_UF2.TXT` and `CURRENT.UF2`, and say whose
//! chip it is (read-only; the identification itself is
//! [`crate::flash::identify`]). The flash job (#100) builds on it:
//! [`source`] fetches the firmware, [`drive`] does the sysfs and file I/O,
//! [`chips`](self::chips) keeps the chip memory on disk, and [`job`] runs
//! the steps.
//!
//! The drive is found by its `INFO_UF2.TXT`, not by the `NICENANO` label:
//! every mount in `/proc/self/mounts` is a candidate.

pub mod chips;
pub mod drive;
pub mod job;
pub mod source;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result, exit};
use crate::flash::{self, Half, Identity, identify::KEYBOARD};

pub const INFO_FILE: &str = "INFO_UF2.TXT";
pub const CURRENT_FILE: &str = "CURRENT.UF2";

/// `CURRENT.UF2` covers at most the 1 MiB flash: 2 blocks of 512 bytes per
/// 256 bytes of it. Anything larger is not a bootloader drive.
const CURRENT_MAX: u64 = 4 << 20;

/// The bootloader, from `INFO_UF2.TXT`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Bootloader {
    /// The first line, e.g. `UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) …`.
    pub version: Option<String>,
    /// `Model:`, e.g. `nice!nano`.
    pub model: Option<String>,
    /// `Board-ID:`, e.g. `nRF52840-nicenano`.
    pub board_id: Option<String>,
}

/// Parses `INFO_UF2.TXT`: a first line naming the bootloader, then
/// `Key: value` lines.
pub fn parse_info(text: &str) -> Bootloader {
    let mut b = Bootloader::default();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        match line.split_once(':') {
            Some(("Model", v)) => b.model = Some(v.trim().to_string()),
            Some(("Board-ID", v)) => b.board_id = Some(v.trim().to_string()),
            _ if n == 0 && !line.is_empty() => b.version = Some(line.to_string()),
            _ => {}
        }
    }
    b
}

/// Mount points in `/proc/self/mounts` format, octal escapes (`\040` for
/// a space) decoded.
pub fn parse_mounts(text: &str) -> Vec<PathBuf> {
    parse_mount_entries(text)
        .into_iter()
        .map(|(_, m)| m)
        .collect()
}

/// `(device, mount point)` pairs from `/proc/self/mounts`.
pub fn parse_mount_entries(text: &str) -> Vec<(String, PathBuf)> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((
                unescape_mount(f.next()?),
                PathBuf::from(unescape_mount(f.next()?)),
            ))
        })
        .collect()
}

fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && let Some(oct) = b.get(i + 1..i + 4)
            && oct.iter().all(|c| (b'0'..=b'7').contains(c))
        {
            out.push((oct[0] - b'0') * 64 + (oct[1] - b'0') * 8 + (oct[2] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Every mounted UF2 bootloader drive.
pub fn find_drives() -> Result<Vec<PathBuf>> {
    drive::Sys::default().drives()
}

/// `mount` if given (it must be a bootloader drive), else the one mounted
/// drive. None is "not found"; several need `--mount`.
pub fn pick_drive(mount: Option<&Path>, found: Vec<PathBuf>) -> Result<PathBuf> {
    if let Some(m) = mount {
        if m.join(INFO_FILE).is_file() {
            return Ok(m.to_path_buf());
        }
        return Err(Error::NotFound(format!(
            "{} has no {INFO_FILE}; is it a UF2 bootloader drive?",
            m.display()
        )));
    }
    match found.len() {
        0 => Err(Error::NotFound(
            "no UF2 bootloader drive is mounted (double-tap reset on the half first)".into(),
        )),
        1 => Ok(found.into_iter().next().expect("one")),
        _ => Err(Error::Usage(format!(
            "{} bootloader drives are mounted ({}); pick one with --mount",
            found.len(),
            found
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// How the identity compares with what the caller expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// This keyboard, and the expected half (or any half when none is
    /// expected).
    Match,
    /// Another keyboard, or the other half.
    Mismatch,
    /// Not enough evidence either way.
    Unknown,
}

/// `flash identify`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identified {
    pub mount: PathBuf,
    pub bootloader: Bootloader,
    #[serde(flatten)]
    pub identity: Identity,
    pub expected: Option<Half>,
    /// From the firmware alone, as in #98; the flash job also weighs the
    /// chip serial ([`crate::flash::decide`]).
    pub verdict: Verdict,
    /// The chip's USB serial (its nRF52840 device ID), `null` when sysfs
    /// doesn't say.
    pub serial: Option<String>,
    /// What sofleii remembers about that chip (#100), if anything.
    pub chip: Option<crate::flash::Chip>,
}

impl Identified {
    /// 0 on a match, 1 otherwise: a script must not flash on 1.
    pub fn exit_code(&self) -> u8 {
        match self.verdict {
            Verdict::Match => exit::OK,
            Verdict::Mismatch | Verdict::Unknown => exit::CHECK_FAILED,
        }
    }
}

/// The verdict for `identity` against `expected`.
pub fn verdict(identity: &Identity, expected: Option<Half>) -> Verdict {
    match identity.keyboard.as_deref() {
        Some(k) if k != KEYBOARD => return Verdict::Mismatch,
        Some(_) => {}
        None => return Verdict::Unknown,
    }
    match (identity.half, expected) {
        (None, _) => Verdict::Unknown,
        (Some(_), None) => Verdict::Match,
        (Some(h), Some(e)) if h == e => Verdict::Match,
        (Some(_), Some(_)) => Verdict::Mismatch,
    }
}

/// Reads the drive at `mount` (or the one mounted) and identifies it.
pub fn identify(mount: Option<&Path>, expected: Option<Half>) -> Result<Identified> {
    let found = if mount.is_some() {
        Vec::new()
    } else {
        find_drives()?
    };
    let mount = pick_drive(mount, found)?;
    let (bootloader, image) = read_drive(&mount)?;
    let identity = flash::identify(&image);
    let verdict = verdict(&identity, expected);
    let serial = drive::Sys::default().serial(&mount);
    let chip = serial
        .as_deref()
        .and_then(|s| chips::load(&chips::default_path()).get(s).cloned());
    Ok(Identified {
        mount,
        bootloader,
        identity,
        expected,
        verdict,
        serial,
        chip,
    })
}

/// The bootloader (`INFO_UF2.TXT`) and the flash (`CURRENT.UF2`) of the
/// drive at `mount`.
pub fn read_drive(mount: &Path) -> Result<(Bootloader, flash::Image)> {
    let info_path = mount.join(INFO_FILE);
    let info = fs::read_to_string(&info_path)
        .map_err(|e| Error::io(format!("reading {}", info_path.display()), e))?;
    let current_path = mount.join(CURRENT_FILE);
    if !current_path.is_file() {
        return Err(Error::Unsupported(format!(
            "the bootloader at {} exposes no {CURRENT_FILE}, so what it holds can't be read",
            mount.display()
        )));
    }
    let mut data = Vec::new();
    fs::File::open(&current_path)
        .and_then(|f| f.take(CURRENT_MAX + 1).read_to_end(&mut data))
        .map_err(|e| Error::io(format!("reading {}", current_path.display()), e))?;
    if data.len() as u64 > CURRENT_MAX {
        return Err(Error::Protocol(format!(
            "{} is larger than any nRF52840 image",
            current_path.display()
        )));
    }
    let image = flash::uf2::parse(&data)
        .map_err(|e| Error::Protocol(format!("{}: {e}", current_path.display())))?;
    Ok((parse_info(&info), image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::Firmware;

    #[test]
    fn info_file() {
        let b = parse_info(
            "UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) lib/tinyusb (0.10.1-293-gaf8e5a90) lib/uf2 (remotes/origin/configupdate-9-gadbb8c7)\r\n\
             Model: nice!nano\r\n\
             Board-ID: nRF52840-nicenano\r\n\
             Date: Jan  2 2021\r\n",
        );
        assert_eq!(b.model.as_deref(), Some("nice!nano"));
        assert_eq!(b.board_id.as_deref(), Some("nRF52840-nicenano"));
        assert!(b.version.unwrap().starts_with("UF2 Bootloader 0.6.0"));
        assert_eq!(parse_info(""), Bootloader::default());
    }

    #[test]
    fn mounts() {
        let m = parse_mounts(
            "/dev/nvme0n1p2 / btrfs rw 0 0\n\
             /dev/sdb /run/media/rdlu/NICENANO vfat rw,nosuid 0 0\n\
             /dev/sdc /run/media/rdlu/MY\\040DRIVE vfat rw 0 0\n",
        );
        assert_eq!(
            m,
            [
                PathBuf::from("/"),
                PathBuf::from("/run/media/rdlu/NICENANO"),
                PathBuf::from("/run/media/rdlu/MY DRIVE"),
            ]
        );
        assert_eq!(unescape_mount(r"a\04b\\x"), r"a\04b\\x");
    }

    #[test]
    fn picking_a_drive() {
        let err = pick_drive(None, vec![]).unwrap_err();
        assert_eq!(err.exit_code(), exit::NOT_FOUND);
        let two = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let err = pick_drive(None, two).unwrap_err();
        assert_eq!(err.exit_code(), exit::USAGE);
        assert!(err.to_string().contains("/a, /b"), "{err}");
        assert_eq!(
            pick_drive(None, vec![PathBuf::from("/a")]).unwrap(),
            PathBuf::from("/a")
        );
        let err = pick_drive(Some(Path::new("/nonexistent")), vec![]).unwrap_err();
        assert_eq!(err.exit_code(), exit::NOT_FOUND);
    }

    fn identity(keyboard: Option<&str>, half: Option<Half>) -> Identity {
        Identity {
            firmware: Firmware::Sofleii,
            keyboard: keyboard.map(String::from),
            half,
            notes: vec![],
        }
    }

    #[test]
    fn verdicts() {
        use Half::*;
        use Verdict::*;
        let ours = |h| identity(Some(KEYBOARD), h);
        assert_eq!(verdict(&ours(Some(Left)), Some(Left)), Match);
        assert_eq!(verdict(&ours(Some(Left)), None), Match);
        assert_eq!(verdict(&ours(Some(Left)), Some(Right)), Mismatch);
        assert_eq!(verdict(&ours(None), Some(Left)), Unknown);
        assert_eq!(verdict(&ours(None), None), Unknown);
        assert_eq!(verdict(&identity(None, None), Some(Left)), Unknown);
        let corne = identity(Some("corne"), Some(Left));
        assert_eq!(verdict(&corne, Some(Left)), Mismatch);
        assert_eq!(verdict(&corne, None), Mismatch);
    }
}
