//! Byte transports to the keyboard. The client only sees [`Transport`], so
//! USB serial and (later) BLE are interchangeable.

pub mod ble;
#[cfg(test)]
pub(crate) mod mock;
pub mod serial;

use serde::Serialize;
use std::time::Duration;

use crate::Result;

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
#[derive(Debug, Clone, Serialize)]
pub struct TransportInfo {
    pub kind: TransportKind,
    /// Device path (USB) or address (BLE).
    pub device: String,
}

/// A bidirectional byte pipe carrying framed Studio RPC messages.
pub trait Transport {
    /// Writes all of `bytes` (one or more complete frames).
    fn send(&mut self, bytes: &[u8]) -> Result<()>;

    /// Reads whatever is available into `buf`, waiting at most `timeout`.
    /// Returns `Ok(0)` when nothing arrived in time; that is not an error.
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    fn info(&self) -> TransportInfo;
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
}
