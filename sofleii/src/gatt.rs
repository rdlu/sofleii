//! The parts of BlueZ's GATT object tree sofleii uses, as plain data, plus
//! the keyboard lookup shared by `battery` and the BLE transport.
//!
//! No D-Bus here: [`crate::bluez`] fills a [`Tree`] from `GetManagedObjects`,
//! and tests build one by hand.

use crate::{Error, Result};

/// The name the keyboard advertises (same as its USB product string).
pub const DEVICE_NAME: &str = "Eyelash Sofle";

/// A BlueZ `org.bluez.Device1` object.
#[derive(Debug, Clone, Default)]
pub struct DeviceObj {
    pub path: String,
    pub address: String,
    pub name: Option<String>,
    pub alias: Option<String>,
    pub connected: bool,
    pub services_resolved: bool,
}

/// A `GattService1` object.
#[derive(Debug, Clone, Default)]
pub struct ServiceObj {
    pub path: String,
    /// Object path of the owning device.
    pub device: String,
    pub uuid: String,
    pub handle: u16,
}

/// A `GattCharacteristic1` object; `value` is BlueZ's cached `Value`.
#[derive(Debug, Clone, Default)]
pub struct CharacteristicObj {
    pub path: String,
    pub service: String,
    pub uuid: String,
    pub handle: u16,
    /// `Flags`, e.g. `["read", "write", "indicate"]`.
    pub flags: Vec<String>,
    pub value: Vec<u8>,
}

impl CharacteristicObj {
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// A `GattDescriptor1` object; `value` is BlueZ's cached `Value`.
#[derive(Debug, Clone, Default)]
pub struct DescriptorObj {
    pub path: String,
    pub characteristic: String,
    pub uuid: String,
    pub value: Vec<u8>,
}

/// The parts of BlueZ's `GetManagedObjects` that matter here.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    /// `Powered` of each `org.bluez.Adapter1`.
    pub adapters_powered: Vec<bool>,
    /// `Address` of each `org.bluez.Adapter1`: this host's own addresses,
    /// which a BLE profile's peer address is compared with.
    pub adapter_addresses: Vec<String>,
    pub devices: Vec<DeviceObj>,
    pub services: Vec<ServiceObj>,
    pub characteristics: Vec<CharacteristicObj>,
    pub descriptors: Vec<DescriptorObj>,
}

/// Which keyboard to use.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// `--bt-address`: match this address instead of the name.
    pub address: Option<String>,
}

/// The connected BlueZ device for `query`: by `--bt-address`, else the one
/// named [`DEVICE_NAME`] (`Name` or `Alias`). Several matches: the first wins
/// and `warnings` says so. Unknown or not connected: [`Error::NotFound`]
/// (exit 2) with the reason.
pub fn find_device<'t>(
    tree: &'t Tree,
    query: &Query,
    warnings: &mut Vec<String>,
) -> Result<&'t DeviceObj> {
    let wanted = |d: &DeviceObj| match &query.address {
        Some(a) => d.address.eq_ignore_ascii_case(a),
        None => d.name.as_deref() == Some(DEVICE_NAME) || d.alias.as_deref() == Some(DEVICE_NAME),
    };
    let what = match &query.address {
        Some(a) => format!("BLE device {a}"),
        None => format!("BLE device named \"{DEVICE_NAME}\""),
    };
    let known: Vec<&DeviceObj> = tree.devices.iter().filter(|d| wanted(d)).collect();
    let connected: Vec<&DeviceObj> = known.iter().copied().filter(|d| d.connected).collect();
    let adapter = if tree.adapters_powered.is_empty() {
        " (BlueZ has no Bluetooth adapter)"
    } else if !tree.adapters_powered.contains(&true) {
        " (the Bluetooth adapter is powered off)"
    } else {
        ""
    };
    match connected.as_slice() {
        [] if known.is_empty() => Err(Error::NotFound(format!(
            "no {what} known to BlueZ on this host, so not connected over BLE{adapter}"
        ))),
        [] => Err(Error::NotFound(format!(
            "{what} is known to BlueZ but not connected to this host over BLE{adapter}"
        ))),
        [one] => Ok(one),
        [first, rest @ ..] => {
            warnings.push(format!(
                "{} connected {what}s; using {} (pick one with --bt-address)",
                rest.len() + 1,
                first.address
            ));
            Ok(first)
        }
    }
}
