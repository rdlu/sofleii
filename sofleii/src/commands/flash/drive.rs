//! The bootloader drive as the system shows it: mounts, the chip's USB
//! serial and the "bootloader without a disk" case from sysfs, and writing
//! the firmware file. [`Sys`] reads the real `/proc` and `/sys`; tests give
//! it a fake root.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{INFO_FILE, parse_mount_entries};
use crate::error::{Error, Result};

/// The nice!nano bootloader's USB vendor (Adafruit), e.g. `239a:00b3`.
pub const BOOTLOADER_VID: &str = "239a";

/// What sofleii reads the system through.
#[derive(Debug, Clone)]
pub struct Sys {
    pub proc_mounts: PathBuf,
    pub sys: PathBuf,
}

impl Default for Sys {
    fn default() -> Self {
        Self {
            proc_mounts: PathBuf::from("/proc/self/mounts"),
            sys: PathBuf::from("/sys"),
        }
    }
}

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

impl Sys {
    fn mounts(&self) -> Result<Vec<(String, PathBuf)>> {
        let text = fs::read_to_string(&self.proc_mounts)
            .map_err(|e| Error::io(format!("reading {}", self.proc_mounts.display()), e))?;
        Ok(parse_mount_entries(&text))
    }

    /// Every mounted UF2 bootloader drive (a mount with `INFO_UF2.TXT`).
    pub fn drives(&self) -> Result<Vec<PathBuf>> {
        Ok(self
            .mounts()?
            .into_iter()
            .map(|(_, m)| m)
            .filter(|m| m.join(INFO_FILE).is_file())
            .collect())
    }

    /// The USB serial of the device behind the drive at `mount`: its block
    /// device (`/dev/sdb`) → `/sys/class/block/sdb` → up to the USB device
    /// directory, the first one with both `idVendor` and `serial`.
    pub fn serial(&self, mount: &Path) -> Option<String> {
        let mounts = self.mounts().ok()?;
        let (dev, _) = mounts.iter().find(|(_, m)| m == mount)?;
        let name = Path::new(dev).file_name()?;
        let mut p = fs::canonicalize(self.sys.join("class/block").join(name)).ok()?;
        loop {
            if p.join("idVendor").is_file()
                && let Some(s) = read_trim(&p.join("serial")).filter(|s| !s.is_empty())
            {
                return Some(s);
            }
            if !p.pop() || !p.starts_with(&self.sys) {
                return None;
            }
        }
    }

    /// A nice!nano bootloader is on USB but its mass-storage interface has
    /// no driver: the disk never appears. Seen after a kernel upgrade
    /// without a reboot (the running kernel's `usb_storage` is gone).
    pub fn bootloader_without_disk(&self) -> bool {
        let Ok(devices) = fs::read_dir(self.sys.join("bus/usb/devices")) else {
            return false;
        };
        devices.flatten().any(|d| {
            let dev = d.path();
            if read_trim(&dev.join("idVendor")).as_deref() != Some(BOOTLOADER_VID) {
                return false;
            }
            let Ok(ifaces) = fs::read_dir(&dev) else {
                return false;
            };
            ifaces.flatten().any(|i| {
                let i = i.path();
                read_trim(&i.join("bInterfaceClass")).as_deref() == Some("08")
                    && !i.join("driver").exists()
            })
        })
    }
}

/// Writes `bytes` as `name` on the drive at `mount`, 64 KiB at a time,
/// calling `progress` with the bytes written so far, then fsyncs.
///
/// The bootloader resets as soon as it has the last block, so the flush
/// after the last byte routinely fails with EIO: the device is gone while
/// its mount point (and the cached `INFO_UF2.TXT`) lingers a moment longer,
/// so the drive can't be asked. Once every byte is handed over, a failing
/// flush is success here; the caller confirms by waiting for the drive to
/// go away (seen live 2026-10-24: EIO on the left half, which had taken
/// the whole image).
pub fn write_uf2(
    mount: &Path,
    name: &str,
    bytes: &[u8],
    progress: &mut dyn FnMut(usize),
) -> Result<()> {
    let path = mount.join(name);
    let ctx = || format!("writing {}", path.display());
    let mut f = fs::File::create(&path).map_err(|e| Error::io(ctx(), e))?;
    let mut done = 0;
    for chunk in bytes.chunks(64 << 10) {
        f.write_all(chunk).map_err(|e| Error::io(ctx(), e))?;
        done += chunk.len();
        progress(done);
    }
    let _ = f.flush().and_then(|()| f.sync_all());
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    pub fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sofleii-drive-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A fake root: a USB device `8-1.3` with serial `serial`, its storage
    /// interface (bound to a driver or not) and block device `sdb`, mounted
    /// at `mnt` with an `INFO_UF2.TXT`.
    pub fn fake(root: &Path, serial: &str, storage_driver: bool) -> (Sys, PathBuf) {
        let usb = root.join("sys/devices/pci0000:00/usb8/8-1/8-1.3");
        let iface = usb.join("8-1.3:1.2");
        let block = iface.join("host3/target3:0:0/3:0:0:0/block/sdb");
        fs::create_dir_all(&block).unwrap();
        fs::write(usb.join("idVendor"), "239a\n").unwrap();
        fs::write(usb.join("serial"), format!("{serial}\n")).unwrap();
        fs::write(iface.join("bInterfaceClass"), "08\n").unwrap();
        if storage_driver {
            fs::create_dir_all(iface.join("driver")).unwrap();
        }
        fs::create_dir_all(root.join("sys/class/block")).unwrap();
        symlink(&block, root.join("sys/class/block/sdb")).unwrap();
        fs::create_dir_all(root.join("sys/bus/usb/devices")).unwrap();
        symlink(&usb, root.join("sys/bus/usb/devices/8-1.3")).unwrap();
        let mnt = root.join("media/NICENANO");
        fs::create_dir_all(&mnt).unwrap();
        fs::write(
            mnt.join(INFO_FILE),
            "UF2 Bootloader 0.6.0\nModel: nice!nano\n",
        )
        .unwrap();
        let mounts = root.join("mounts");
        fs::write(
            &mounts,
            format!(
                "/dev/nvme0n1p2 / btrfs rw 0 0\n/dev/sdb {} vfat rw 0 0\n",
                mnt.display()
            ),
        )
        .unwrap();
        (
            Sys {
                proc_mounts: mounts,
                sys: root.join("sys"),
            },
            mnt,
        )
    }

    #[test]
    fn serial_from_sysfs() {
        let root = tmp("serial");
        let (sys, mnt) = fake(&root, "93B0CC1C158978F8", true);
        assert_eq!(sys.drives().unwrap(), std::slice::from_ref(&mnt));
        assert_eq!(sys.serial(&mnt).as_deref(), Some("93B0CC1C158978F8"));
        assert_eq!(sys.serial(Path::new("/elsewhere")), None);
        assert!(!sys.bootloader_without_disk());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn storage_without_driver() {
        let root = tmp("nodisk");
        let (sys, _) = fake(&root, "94708422E5F2FD3E", false);
        assert!(sys.bootloader_without_disk());
        let none = Sys {
            proc_mounts: root.join("none"),
            sys: root.join("nowhere"),
        };
        assert!(!none.bootloader_without_disk());
        assert!(none.drives().is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn writes_in_chunks() {
        let root = tmp("write");
        let (_, mnt) = fake(&root, "X", true);
        let data = vec![7u8; 150_000];
        let mut seen = Vec::new();
        write_uf2(&mnt, "fw.uf2", &data, &mut |n| seen.push(n)).unwrap();
        assert_eq!(seen, [65536, 131072, 150_000]);
        assert_eq!(fs::read(mnt.join("fw.uf2")).unwrap(), data);
        let _ = fs::remove_dir_all(&root);
    }
}
