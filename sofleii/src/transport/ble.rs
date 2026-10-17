//! BLE transport (not implemented yet).
//!
//! The plan (#44): reuse the existing HID connection, find the Studio GATT
//! service and write/notify on its RPC characteristic. Frames are the same as
//! over USB.

use std::time::Duration;

use super::{Transport, TransportInfo, TransportKind};
use crate::{Error, Result};

/// ZMK Studio GATT service.
pub const STUDIO_SERVICE_UUID: &str = "00000000-0196-6107-c967-c5cfb1c2482a";
/// Studio RPC characteristic (write + notify).
pub const STUDIO_RPC_CHAR_UUID: &str = "00000001-0196-6107-c967-c5cfb1c2482a";

pub struct BleTransport {
    address: String,
}

impl BleTransport {
    /// Always fails for now; kept so the CLI and client are already shaped
    /// around more than one transport.
    pub fn connect(_address: Option<&str>) -> Result<Self> {
        Err(Error::Usage(
            "the BLE transport is not implemented yet; connect the left half over USB".into(),
        ))
    }
}

impl Transport for BleTransport {
    fn send(&mut self, _bytes: &[u8]) -> Result<()> {
        Err(Error::Protocol("BLE transport not implemented".into()))
    }
    fn recv(&mut self, _buf: &mut [u8], _timeout: Duration) -> Result<usize> {
        Err(Error::Protocol("BLE transport not implemented".into()))
    }
    fn info(&self) -> TransportInfo {
        TransportInfo {
            kind: TransportKind::Ble,
            device: self.address.clone(),
        }
    }
}
