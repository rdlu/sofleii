//! Byte transports to the keyboard. The client only sees [`Transport`], so
//! USB serial and BLE GATT are interchangeable; [`plan`] picks one.

pub mod ble;
#[cfg(test)]
pub(crate) mod mock;
pub mod serial;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::time::Duration;

use crate::{Error, Result};

/// Which kind of link a transport is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Usb,
    Ble,
}

impl std::fmt::Display for TransportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TransportKind::Usb => "usb",
            TransportKind::Ble => "ble",
        })
    }
}

/// What a transport is connected to, for display and `--json`.
#[derive(Debug, Clone)]
pub struct TransportInfo {
    pub kind: TransportKind,
    /// Device path (USB) or address (BLE).
    pub device: String,
}

/// JSON: `{"kind":"usb","device":"/dev/ttyACM1"}` or
/// `{"kind":"ble","address":"DD:1F:23:76:6E:71"}`.
impl Serialize for TransportInfo {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("TransportInfo", 2)?;
        st.serialize_field("kind", &self.kind)?;
        match self.kind {
            TransportKind::Usb => st.serialize_field("device", &self.device)?,
            TransportKind::Ble => st.serialize_field("address", &self.device)?,
        }
        st.end()
    }
}

/// A bidirectional byte pipe carrying framed Studio RPC messages. `Send`, so
/// a [`crate::client::Client`] can live in a worker thread (the TUI session).
pub trait Transport: Send {
    /// Writes all of `bytes` (one or more complete frames).
    fn send(&mut self, bytes: &[u8]) -> Result<()>;

    /// Reads whatever is available into `buf`, waiting at most `timeout`.
    /// Returns `Ok(0)` when nothing arrived in time; that is not an error.
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    fn info(&self) -> TransportInfo;

    /// A likely reason when a request times out, added to the error.
    fn timeout_hint(&self) -> Option<String> {
        None
    }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        (**self).send(bytes)
    }
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        (**self).recv(buf, timeout)
    }
    fn info(&self) -> TransportInfo {
        (**self).info()
    }
    fn timeout_hint(&self) -> Option<String> {
        (**self).timeout_hint()
    }
}

/// `--transport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    Auto,
    Usb,
    Ble,
}

/// The transport [`plan`] picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// `device`: `--device`, or `None` to discover the port. `auto`: picked
    /// by `--transport auto` only because the port exists, so the caller may
    /// fall back to BLE if USB doesn't answer.
    Usb { device: Option<String>, auto: bool },
    /// `address`: `--bt-address`, or `None` to match the name.
    Ble { address: Option<String> },
}

/// Picks the transport from the flags and what's present.
///
/// - `--device` means USB and `--bt-address` means BLE; contradicting
///   `--transport`, or each other, is a usage error (exit 3).
/// - `auto`: USB if a serial port with the product string exists
///   (`usb_present`), else BLE if the keyboard is connected to BlueZ
///   (`ble_connected`, `Err` carrying why not), else not found (exit 2),
///   naming both.
///
/// The probes are only called when needed.
pub fn plan(
    pref: Preference,
    device: Option<&str>,
    bt_address: Option<&str>,
    usb_present: impl FnOnce() -> bool,
    ble_connected: impl FnOnce() -> std::result::Result<(), String>,
) -> Result<Plan> {
    let usb = |auto| Plan::Usb {
        device: device.map(str::to_string),
        auto,
    };
    let ble = || Plan::Ble {
        address: bt_address.map(str::to_string),
    };
    match (pref, device, bt_address) {
        (_, Some(_), Some(_)) => Err(Error::Usage(
            "--device (USB) and --bt-address (BLE) pick different transports; use one".into(),
        )),
        (Preference::Usb, _, Some(_)) => Err(Error::Usage(
            "--bt-address picks the BLE transport, but --transport is usb".into(),
        )),
        (Preference::Ble, Some(_), _) => Err(Error::Usage(
            "--device picks the USB transport, but --transport is ble".into(),
        )),
        (Preference::Usb, _, _) | (Preference::Auto, Some(_), None) => Ok(usb(false)),
        (Preference::Ble, _, _) | (Preference::Auto, None, Some(_)) => Ok(ble()),
        (Preference::Auto, None, None) => {
            if usb_present() {
                return Ok(usb(true));
            }
            match ble_connected() {
                Ok(()) => Ok(ble()),
                Err(why) => Err(Error::NotFound(format!(
                    "not over USB (no serial port with product \"{}\") nor over BLE ({why})",
                    serial::USB_PRODUCT
                ))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::exit;

    fn never_usb() -> bool {
        panic!("USB probed")
    }
    fn never_ble() -> std::result::Result<(), String> {
        panic!("BLE probed")
    }

    #[test]
    fn auto_prefers_usb_when_present() {
        let p = plan(Preference::Auto, None, None, || true, never_ble).unwrap();
        assert_eq!(
            p,
            Plan::Usb {
                device: None,
                auto: true
            }
        );
    }

    #[test]
    fn auto_uses_ble_without_usb() {
        let p = plan(Preference::Auto, None, None, || false, || Ok(())).unwrap();
        assert_eq!(p, Plan::Ble { address: None });
    }

    #[test]
    fn auto_with_neither_is_not_found_naming_both() {
        let e = plan(
            Preference::Auto,
            None,
            None,
            || false,
            || Err("BLE device named \"Eyelash Sofle\" is known to BlueZ but not connected".into()),
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), exit::NOT_FOUND);
        let m = e.to_string();
        assert!(
            m.contains("USB") && m.contains("BLE") && m.contains("not connected"),
            "{m}"
        );
    }

    #[test]
    fn flags_imply_a_transport_without_probing() {
        assert_eq!(
            plan(
                Preference::Auto,
                Some("/dev/ttyACM1"),
                None,
                never_usb,
                never_ble
            )
            .unwrap(),
            Plan::Usb {
                device: Some("/dev/ttyACM1".into()),
                auto: false
            }
        );
        assert_eq!(
            plan(
                Preference::Auto,
                None,
                Some("DD:1F:23:76:6E:71"),
                never_usb,
                never_ble
            )
            .unwrap(),
            Plan::Ble {
                address: Some("DD:1F:23:76:6E:71".into())
            }
        );
        assert_eq!(
            plan(Preference::Ble, None, None, never_usb, never_ble).unwrap(),
            Plan::Ble { address: None }
        );
        assert_eq!(
            plan(Preference::Usb, None, None, never_usb, never_ble).unwrap(),
            Plan::Usb {
                device: None,
                auto: false
            }
        );
    }

    #[test]
    fn contradictory_flags_are_usage_errors() {
        for (pref, dev, addr) in [
            (
                Preference::Auto,
                Some("/dev/ttyACM1"),
                Some("DD:1F:23:76:6E:71"),
            ),
            (Preference::Usb, None, Some("DD:1F:23:76:6E:71")),
            (Preference::Ble, Some("/dev/ttyACM1"), None),
        ] {
            let e = plan(pref, dev, addr, never_usb, never_ble).unwrap_err();
            assert_eq!(e.exit_code(), exit::USAGE, "{pref:?} {dev:?} {addr:?}");
        }
    }

    #[test]
    fn json_shape_by_kind() {
        let usb = TransportInfo {
            kind: TransportKind::Usb,
            device: "/dev/ttyACM1".into(),
        };
        let ble = TransportInfo {
            kind: TransportKind::Ble,
            device: "DD:1F:23:76:6E:71".into(),
        };
        assert_eq!(
            serde_json::to_value(&usb).unwrap(),
            serde_json::json!({"kind": "usb", "device": "/dev/ttyACM1"})
        );
        assert_eq!(
            serde_json::to_value(&ble).unwrap(),
            serde_json::json!({"kind": "ble", "address": "DD:1F:23:76:6E:71"})
        );
    }
}
