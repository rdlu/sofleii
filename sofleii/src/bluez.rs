//! BlueZ over the system D-Bus (zbus, no tokio). zbus 6 dropped its
//! blocking API, so every call goes through [`call`], which runs it to
//! completion on the calling thread with `zbus::block_on`.
//!
//! - The read-only [`Gatt`] implementation behind `sofleii battery`: only
//!   `GetManagedObjects`, `ReadValue({})` and cached property values.
//! - The connection and object tree the BLE transport
//!   ([`crate::transport::ble`]) builds its Studio RPC link on.
//!
//! Nothing here pairs, connects or disconnects, and nothing assumes `hci0`
//! or fixed handles; everything comes from the object tree.

use std::collections::HashMap;
use std::time::Duration;

use zbus::connection::Builder;
use zbus::{Connection, DBusError, DynamicType, Message, OwnedObjectPath, OwnedVariant, Variant};

use crate::commands::battery::Gatt;
use crate::gatt::{CharacteristicObj, DescriptorObj, DeviceObj, ServiceObj, Tree};
use crate::{Error, Result};

pub const BLUEZ: &str = "org.bluez";

type Props = HashMap<String, OwnedVariant>;
type Managed = HashMap<OwnedObjectPath, HashMap<String, Props>>;

pub struct Bluez {
    conn: Connection,
}

impl Bluez {
    /// Connects to the system bus; every method call times out after
    /// `timeout`. A missing bus is "not found" (exit 2).
    pub fn connect(timeout: Duration) -> Result<Self> {
        let conn =
            zbus::block_on(Builder::system().method_timeout(timeout).build()).map_err(|e| {
                Error::NotFound(format!(
                    "BlueZ unavailable: cannot connect to the system D-Bus ({e})"
                ))
            })?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// `GetManagedObjects`, as plain data.
    pub fn managed_tree(&self) -> Result<Tree> {
        let reply = call(
            &self.conn,
            "/",
            "org.freedesktop.DBus.ObjectManager",
            "GetManagedObjects",
            &(),
        )
        .map_err(|e| call_error("GetManagedObjects", e))?;
        let managed: Managed = reply
            .body()
            .deserialize()
            .map_err(|e| Error::Bluez(format!("GetManagedObjects: bad reply ({e})")))?;
        Ok(tree_from(&managed))
    }

    fn read(&self, path: &str, iface: &str) -> Result<Vec<u8>> {
        let opts: HashMap<&str, Variant> = HashMap::new();
        let reply = call(&self.conn, path, iface, "ReadValue", &(opts,))
            .map_err(|e| call_error("ReadValue", e))?;
        reply
            .body()
            .deserialize::<Vec<u8>>()
            .map_err(|e| Error::Bluez(format!("ReadValue on {path}: bad reply ({e})")))
    }
}

/// One method call on `org.bluez`, blocking the calling thread until the
/// reply (or the connection's method timeout).
pub fn call<B>(
    conn: &Connection,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<Message>
where
    B: serde::Serialize + DynamicType,
{
    zbus::block_on(conn.call_method(Some(BLUEZ), path, Some(iface), method, body))
}

/// Maps a zbus error; BlueZ not running, or the device dropping the
/// connection, is "not found" (exit 2).
pub fn call_error(what: &str, e: zbus::Error) -> Error {
    // zbus 6 reports the org.freedesktop.DBus.Error.* replies as `FDO`,
    // every other error reply as `MethodError`.
    let reply = match &e {
        zbus::Error::MethodError(name, desc, _) => Some((name.as_str().to_string(), desc.clone())),
        zbus::Error::FDO(f) => Some((
            f.name().as_str().to_string(),
            f.description().map(str::to_string),
        )),
        _ => None,
    };
    if let Some((name, desc)) = reply {
        let name = name.as_str();
        if name == "org.freedesktop.DBus.Error.ServiceUnknown"
            || name == "org.freedesktop.DBus.Error.NameHasNoOwner"
        {
            return Error::NotFound(
                "BlueZ unavailable: org.bluez is not running on the system D-Bus".into(),
            );
        }
        if name == "org.bluez.Error.NotConnected" {
            return Error::NotFound(format!("{what}: the keyboard is not connected over BLE"));
        }
        return Error::Bluez(format!(
            "{what}: {name}{}",
            desc.as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default()
        ));
    }
    Error::Bluez(format!("{what}: {e}"))
}

impl Gatt for Bluez {
    fn tree(&mut self) -> Result<Tree> {
        self.managed_tree()
    }

    fn read_characteristic(&mut self, path: &str) -> Result<Vec<u8>> {
        self.read(path, "org.bluez.GattCharacteristic1")
    }

    fn read_descriptor(&mut self, path: &str) -> Result<Vec<u8>> {
        self.read(path, "org.bluez.GattDescriptor1")
    }
}

fn tree_from(managed: &Managed) -> Tree {
    let mut t = Tree::default();
    for (path, ifaces) in managed {
        let path = path.as_str().to_string();
        if let Some(p) = ifaces.get("org.bluez.Adapter1") {
            t.adapters_powered.push(boolean(p, "Powered"));
        }
        if let Some(p) = ifaces.get("org.bluez.Device1") {
            t.devices.push(DeviceObj {
                path: path.clone(),
                address: string(p, "Address").unwrap_or_default(),
                name: string(p, "Name"),
                alias: string(p, "Alias"),
                connected: boolean(p, "Connected"),
                services_resolved: boolean(p, "ServicesResolved"),
            });
        }
        if let Some(p) = ifaces.get("org.bluez.GattService1") {
            t.services.push(ServiceObj {
                path: path.clone(),
                device: string(p, "Device").unwrap_or_default(),
                uuid: string(p, "UUID").unwrap_or_default(),
                handle: handle(p),
            });
        }
        if let Some(p) = ifaces.get("org.bluez.GattCharacteristic1") {
            t.characteristics.push(CharacteristicObj {
                path: path.clone(),
                service: string(p, "Service").unwrap_or_default(),
                uuid: string(p, "UUID").unwrap_or_default(),
                handle: handle(p),
                flags: strings(p, "Flags"),
                value: bytes(p, "Value"),
            });
        }
        if let Some(p) = ifaces.get("org.bluez.GattDescriptor1") {
            t.descriptors.push(DescriptorObj {
                path,
                characteristic: string(p, "Characteristic").unwrap_or_default(),
                uuid: string(p, "UUID").unwrap_or_default(),
                value: bytes(p, "Value"),
            });
        }
    }
    t
}

fn string(p: &Props, key: &str) -> Option<String> {
    match p.get(key).map(|v| &**v) {
        Some(Variant::Str(s)) => Some(s.as_str().to_string()),
        Some(Variant::ObjectPath(o)) => Some(o.as_str().to_string()),
        _ => None,
    }
}

fn strings(p: &Props, key: &str) -> Vec<String> {
    match p.get(key).map(|v| &**v) {
        Some(Variant::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Variant::Str(s) => Some(s.as_str().to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn boolean(p: &Props, key: &str) -> bool {
    matches!(p.get(key).map(|v| &**v), Some(Variant::Bool(true)))
}

fn handle(p: &Props) -> u16 {
    match p.get("Handle").map(|v| &**v) {
        Some(Variant::U16(h)) => *h,
        _ => 0,
    }
}

fn bytes(p: &Props, key: &str) -> Vec<u8> {
    match p.get(key).map(|v| &**v) {
        Some(Variant::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Variant::U8(b) => Some(*b),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_bluez_is_not_found_from_the_fdo_error() {
        // zbus 6 reports ServiceUnknown as `FDO`, not `MethodError`.
        let fdo = zbus::Error::FDO(Box::new(zbus::fdo::Error::ServiceUnknown(
            "The name org.bluez was not provided by any .service files".into(),
        )));
        let e = call_error("GetManagedObjects", fdo);
        assert_eq!(e.exit_code(), crate::error::exit::NOT_FOUND);
        assert!(e.to_string().contains("org.bluez is not running"), "{e}");
    }
}
