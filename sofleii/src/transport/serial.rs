//! USB CDC-ACM transport (left half only) and device discovery.
//!
//! The VID/PID (`1d50:615e`) is shared by every ZMK board, so the keyboard
//! is found by its USB product string instead. Never assume a fixed
//! `/dev/ttyACM*` number.

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::time::Duration;

use serialport::{SerialPort, SerialPortType};

use super::{Transport, TransportInfo, TransportKind};
use crate::{Error, Result};

pub const USB_PRODUCT: &str = "Eyelash Sofle";
pub const USB_MANUFACTURER: &str = "sofle";
const BY_ID_DIR: &str = "/dev/serial/by-id";

/// A serial port that looks like the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: String,
    pub serial_number: Option<String>,
}

/// Lists serial ports whose USB product string is [`USB_PRODUCT`].
///
/// Uses the `serialport` enumeration (sysfs on Linux) first, then falls back
/// to the udev `/dev/serial/by-id/` symlinks.
pub fn discover() -> Vec<Candidate> {
    let mut found: Vec<Candidate> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            SerialPortType::UsbPort(usb) if usb.product.as_deref() == Some(USB_PRODUCT) => {
                Some(Candidate {
                    path: p.port_name,
                    serial_number: usb.serial_number,
                })
            }
            _ => None,
        })
        .collect();
    if found.is_empty() {
        found = discover_by_id(Path::new(BY_ID_DIR));
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

/// udev names the links `usb-<manufacturer>_<product>_<serial>-if<NN>`, with
/// spaces turned into underscores.
fn discover_by_id(dir: &Path) -> Vec<Candidate> {
    let prefix = format!("usb-{USB_MANUFACTURER}_{}_", USB_PRODUCT.replace(' ', "_"));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rest = name.strip_prefix(&prefix)?;
            let serial = rest.rsplit_once("-if").map(|(s, _)| s.to_string());
            let target = std::fs::canonicalize(e.path()).ok()?;
            Some(Candidate {
                path: target.to_string_lossy().into_owned(),
                serial_number: serial,
            })
        })
        .collect()
}

pub struct SerialTransport {
    port: Box<dyn SerialPort>,
    path: String,
}

impl SerialTransport {
    /// Opens `path`, or the discovered keyboard when `path` is `None`.
    pub fn open(path: Option<&str>) -> Result<Self> {
        let path = match path {
            Some(p) => {
                if !Path::new(p).exists() {
                    return Err(Error::NotFound(format!("{p} does not exist")));
                }
                p.to_string()
            }
            None => {
                let mut found = discover();
                match found.len() {
                    0 => {
                        return Err(Error::NotFound(format!(
                            "no USB serial port with product \"{USB_PRODUCT}\" \
                             (is the left half plugged in over USB? use --device to pick a port)"
                        )));
                    }
                    1 => {}
                    _ => log::warn!(
                        "several keyboards found ({}); using {}",
                        found
                            .iter()
                            .map(|c| c.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        found[0].path
                    ),
                }
                found.swap_remove(0).path
            }
        };
        // CDC-ACM ignores the baud rate; serialport sets raw mode for us.
        let port = serialport::new(&path, 115_200)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| match e.kind() {
                serialport::ErrorKind::NoDevice => Error::NotFound(format!("{path}: {e}")),
                serialport::ErrorKind::Io(ErrorKind::NotFound) => {
                    Error::NotFound(format!("{path}: {e}"))
                }
                serialport::ErrorKind::Io(k) => Error::io(
                    format!("opening {path}"),
                    std::io::Error::new(k, e.description),
                ),
                _ => Error::io(
                    format!("opening {path}"),
                    std::io::Error::other(e.description),
                ),
            })?;
        // Drop anything left over from an earlier session.
        let _ = port.clear(serialport::ClearBuffer::Input);
        Ok(Self { port, path })
    }
}

impl Transport for SerialTransport {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.port
            .write_all(bytes)
            .and_then(|()| self.port.flush())
            .map_err(|e| Error::io(format!("writing to {}", self.path), e))
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        self.port
            .set_timeout(timeout.max(Duration::from_millis(1)))
            .map_err(|e| Error::io(format!("configuring {}", self.path), e.into()))?;
        match self.port.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => Ok(0),
            Err(e) if e.kind() == ErrorKind::Interrupted => Ok(0),
            Err(e) => Err(Error::io(format!("reading from {}", self.path), e)),
        }
    }

    fn info(&self) -> TransportInfo {
        TransportInfo {
            kind: TransportKind::Usb,
            device: self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_id_matches_product_and_extracts_serial() {
        let dir = std::env::temp_dir().join(format!("sofleii-byid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ttyFAKE1");
        std::fs::write(&target, b"").unwrap();
        let other = dir.join("ttyFAKE0");
        std::fs::write(&other, b"").unwrap();
        std::os::unix::fs::symlink(
            &target,
            dir.join("usb-sofle_Eyelash_Sofle_94708422E5F2FD3E-if00"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&other, dir.join("usb-EMEET_EMEET_SmartCam_S800_X-if05"))
            .unwrap();

        let found = discover_by_id(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("ttyFAKE1"));
        assert_eq!(found[0].serial_number.as_deref(), Some("94708422E5F2FD3E"));
    }
}
