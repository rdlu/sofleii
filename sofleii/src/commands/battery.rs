//! `sofleii battery`: both halves' battery levels, read from the two BLE
//! Battery Services the left half exposes since #45.
//!
//! The left (central) half's own service has no 0x2901 user description;
//! the proxied right half's is described "Peripheral 0" (a second
//! peripheral would be "Peripheral 1"). BlueZ's `org.bluez.Battery1` (and so
//! upower) only reports the first service, so both levels are read from the
//! GATT objects directly.
//!
//! This module knows the BlueZ object model as plain data ([`Tree`]) and
//! reads through the small [`Gatt`] trait; the D-Bus implementation lives in
//! [`crate::bluez`], and tests use a fake.

use serde::Serialize;

use crate::{Error, Result};

/// The name the keyboard advertises (same as its USB product string).
pub const DEVICE_NAME: &str = "Eyelash Sofle";
/// Battery Service.
pub const BATTERY_SERVICE_UUID: &str = "0000180f-0000-1000-8000-00805f9b34fb";
/// Battery Level characteristic.
pub const BATTERY_LEVEL_UUID: &str = "00002a19-0000-1000-8000-00805f9b34fb";
/// Characteristic User Description descriptor.
pub const USER_DESCRIPTION_UUID: &str = "00002901-0000-1000-8000-00805f9b34fb";
/// Prefix of the description ZMK gives each proxied peripheral's level.
pub const PERIPHERAL_PREFIX: &str = "Peripheral";

/// Default `--warn-below` (class `warning` in waybar).
pub const DEFAULT_WARN_BELOW: u8 = 20;
/// Default `--critical-below` (class `critical` in waybar).
pub const DEFAULT_CRITICAL_BELOW: u8 = 10;

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
    pub value: Vec<u8>,
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
    pub devices: Vec<DeviceObj>,
    pub services: Vec<ServiceObj>,
    pub characteristics: Vec<CharacteristicObj>,
    pub descriptors: Vec<DescriptorObj>,
}

/// Read-only access to BlueZ. Implementations never write, pair or
/// subscribe.
pub trait Gatt {
    /// All objects (`ObjectManager.GetManagedObjects`).
    fn tree(&mut self) -> Result<Tree>;
    /// `GattCharacteristic1.ReadValue({})`.
    fn read_characteristic(&mut self, path: &str) -> Result<Vec<u8>>;
    /// `GattDescriptor1.ReadValue({})`.
    fn read_descriptor(&mut self, path: &str) -> Result<Vec<u8>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BtDevice {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct BatteryLevel {
    pub percent: Option<u8>,
}

/// The result of `sofleii battery`.
#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    /// Always `"bluez"` for now.
    pub source: &'static str,
    pub device: BtDevice,
    pub left: BatteryLevel,
    pub right: BatteryLevel,
    /// Anything worth knowing about how the levels were found, or `null`.
    pub note: Option<String>,
    /// Diagnostics for stderr (not part of the JSON).
    #[serde(skip)]
    pub warnings: Vec<String>,
}

impl Reading {
    /// The lower of the known levels.
    pub fn min_percent(&self) -> Option<u8> {
        [self.left.percent, self.right.percent]
            .into_iter()
            .flatten()
            .min()
    }
}

/// waybar class for a reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warning,
    Critical,
    /// Neither half could be read.
    Unknown,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warning => "warning",
            Level::Critical => "critical",
            Level::Unknown => "unknown",
        }
    }
}

/// Thresholds: a half strictly below `critical_below` is critical, below
/// `warn_below` a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub warn_below: u8,
    pub critical_below: u8,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            warn_below: DEFAULT_WARN_BELOW,
            critical_below: DEFAULT_CRITICAL_BELOW,
        }
    }
}

impl Thresholds {
    /// Classifies by the lower of the known halves.
    pub fn classify(&self, r: &Reading) -> Level {
        match r.min_percent() {
            None => Level::Unknown,
            Some(p) if p < self.critical_below => Level::Critical,
            Some(p) if p < self.warn_below => Level::Warning,
            Some(_) => Level::Ok,
        }
    }
}

/// Which keyboard to read.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// `--bt-address`: match this address instead of the name.
    pub address: Option<String>,
}

/// Reads both halves through `gatt`.
pub fn run(gatt: &mut dyn Gatt, query: &Query) -> Result<Reading> {
    let tree = gatt.tree()?;
    let mut warnings = Vec::new();
    let device = find_device(&tree, query, &mut warnings)?.clone();
    let mut notes = Vec::new();
    let (left, right) = identify(gatt, &tree, &device, &mut notes)?;
    let left = read_level(gatt, left, "left", &mut notes);
    let right = read_level(gatt, right, "right", &mut notes);
    if left.failed && right.failed {
        return Err(Error::Bluez(format!(
            "could not read either Battery Level: {}",
            notes.join("; ")
        )));
    }
    Ok(Reading {
        source: "bluez",
        device: BtDevice {
            name: device
                .alias
                .clone()
                .or(device.name.clone())
                .unwrap_or_else(|| DEVICE_NAME.to_string()),
            address: device.address.clone(),
        },
        left: BatteryLevel {
            percent: left.percent,
        },
        right: BatteryLevel {
            percent: right.percent,
        },
        note: (!notes.is_empty()).then(|| notes.join("; ")),
        warnings,
    })
}

fn find_device<'t>(
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

/// How a Battery Level characteristic describes itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Role {
    /// No 0x2901 descriptor: the central's own level (left half).
    Central,
    /// "Peripheral n": a proxied peripheral (n = 0 is the right half).
    Peripheral(u8),
    /// The description could not be read, or says something else.
    Unknown,
}

struct Candidate<'t> {
    ch: &'t CharacteristicObj,
    role: Role,
}

type Halves<'t> = (Option<&'t CharacteristicObj>, Option<&'t CharacteristicObj>);

/// Finds the left and right halves' Battery Level characteristics.
fn identify<'t>(
    gatt: &mut dyn Gatt,
    tree: &'t Tree,
    device: &DeviceObj,
    notes: &mut Vec<String>,
) -> Result<Halves<'t>> {
    let mut services: Vec<&ServiceObj> = tree
        .services
        .iter()
        .filter(|s| s.device == device.path && s.uuid.eq_ignore_ascii_case(BATTERY_SERVICE_UUID))
        .collect();
    services.sort_by_key(|s| s.handle);
    let mut cands = Vec::new();
    for s in services {
        let Some(ch) = tree
            .characteristics
            .iter()
            .filter(|c| c.service == s.path && c.uuid.eq_ignore_ascii_case(BATTERY_LEVEL_UUID))
            .min_by_key(|c| c.handle)
        else {
            continue;
        };
        let role = role(gatt, tree, ch);
        cands.push(Candidate { ch, role });
    }
    if cands.is_empty() {
        let hint = if device.services_resolved {
            "the firmware exposes none"
        } else {
            "BlueZ hasn't resolved its GATT services yet (ServicesResolved=false)"
        };
        return Err(Error::Bluez(format!(
            "{} has no Battery Level characteristic: {hint}",
            device.address
        )));
    }

    let centrals: Vec<&Candidate> = cands.iter().filter(|c| c.role == Role::Central).collect();
    let mut peripherals: Vec<(u8, &Candidate)> = cands
        .iter()
        .filter_map(|c| match c.role {
            Role::Peripheral(n) => Some((n, c)),
            _ => None,
        })
        .collect();
    peripherals.sort_by_key(|(n, _)| *n);
    let unknown = cands.iter().any(|c| c.role == Role::Unknown);

    if !unknown && centrals.len() <= 1 && !(centrals.is_empty() && peripherals.is_empty()) {
        let left = centrals.first().map(|c| c.ch);
        let right = peripherals.first().map(|(_, c)| c.ch);
        if left.is_none() {
            notes.push("no Battery Service without a description; left half unknown".into());
        }
        if right.is_none() {
            notes.push(
                "only the left half's Battery Service is exposed; the right half's level \
                 isn't forwarded (firmware before #45, or the right half is disconnected)"
                    .into(),
            );
        }
        if peripherals.len() > 1 {
            notes.push(format!(
                "{} peripheral levels exposed; right = \"{PERIPHERAL_PREFIX} {}\"",
                peripherals.len(),
                peripherals[0].0
            ));
        }
        return Ok((left, right));
    }

    // Fallback: service order by handle, first = left.
    notes.push(
        "halves assigned by Battery Service handle order (first = left): \
         the user descriptions don't identify them"
            .into(),
    );
    Ok((cands.first().map(|c| c.ch), cands.get(1).map(|c| c.ch)))
}

fn role(gatt: &mut dyn Gatt, tree: &Tree, ch: &CharacteristicObj) -> Role {
    let Some(desc) = tree.descriptors.iter().find(|d| {
        d.characteristic == ch.path && d.uuid.eq_ignore_ascii_case(USER_DESCRIPTION_UUID)
    }) else {
        return Role::Central;
    };
    let bytes = if desc.value.is_empty() {
        match gatt.read_descriptor(&desc.path) {
            Ok(v) => v,
            Err(_) => return Role::Unknown,
        }
    } else {
        desc.value.clone()
    };
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_end_matches('\0').trim();
    match text.strip_prefix(PERIPHERAL_PREFIX) {
        Some(rest) => Role::Peripheral(rest.trim().parse().unwrap_or(0)),
        None => Role::Unknown,
    }
}

struct HalfRead {
    percent: Option<u8>,
    /// Neither a fresh read nor a cached value.
    failed: bool,
}

fn read_level(
    gatt: &mut dyn Gatt,
    ch: Option<&CharacteristicObj>,
    half: &str,
    notes: &mut Vec<String>,
) -> HalfRead {
    let Some(ch) = ch else {
        return HalfRead {
            percent: None,
            failed: false,
        };
    };
    let percent = |v: &[u8]| v.first().copied().filter(|p| *p <= 100);
    match gatt.read_characteristic(&ch.path) {
        Ok(v) if percent(&v).is_some() => HalfRead {
            percent: percent(&v),
            failed: false,
        },
        res => {
            let why = match res {
                Ok(v) => format!("unexpected value {v:?}"),
                Err(e) => e.to_string(),
            };
            match percent(&ch.value) {
                Some(p) => {
                    notes.push(format!(
                        "{half}: read failed ({why}); using BlueZ's cached value"
                    ));
                    HalfRead {
                        percent: Some(p),
                        failed: false,
                    }
                }
                None => {
                    notes.push(format!("{half}: read failed ({why}) and nothing cached"));
                    HalfRead {
                        percent: None,
                        failed: true,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory BlueZ tree for tests.
    use super::*;
    use std::collections::HashMap;

    pub const DEV: &str = "/org/bluez/hci1/dev_DD_1F_23_76_6E_71";

    #[derive(Default)]
    pub struct FakeGatt {
        pub tree: Tree,
        /// Fresh ReadValue results by path; a missing path fails.
        pub reads: HashMap<String, Vec<u8>>,
        pub tree_error: Option<fn() -> Error>,
    }

    impl Gatt for FakeGatt {
        fn tree(&mut self) -> Result<Tree> {
            match self.tree_error {
                Some(e) => Err(e()),
                None => Ok(self.tree.clone()),
            }
        }
        fn read_characteristic(&mut self, path: &str) -> Result<Vec<u8>> {
            self.reads
                .get(path)
                .cloned()
                .ok_or_else(|| Error::Bluez(format!("{path}: org.bluez.Error.Failed")))
        }
        fn read_descriptor(&mut self, path: &str) -> Result<Vec<u8>> {
            self.read_characteristic(path)
        }
    }

    impl FakeGatt {
        pub fn keyboard() -> Self {
            let mut g = FakeGatt::default();
            g.tree.adapters_powered.push(true);
            g.tree.devices.push(DeviceObj {
                path: DEV.into(),
                address: "DD:1F:23:76:6E:71".into(),
                name: Some(DEVICE_NAME.into()),
                alias: Some(DEVICE_NAME.into()),
                connected: true,
                services_resolved: true,
            });
            g
        }

        /// Adds a Battery Service at `handle` with level `level` (fresh and
        /// cached) and an optional user description (`Some(b"")` = present
        /// but not cached).
        pub fn battery(mut self, handle: u16, level: u8, desc: Option<&[u8]>) -> Self {
            let svc = format!("{DEV}/service{handle:04x}");
            let ch = format!("{svc}/char{:04x}", handle + 1);
            self.tree.services.push(ServiceObj {
                path: svc.clone(),
                device: DEV.into(),
                uuid: BATTERY_SERVICE_UUID.into(),
                handle,
            });
            self.tree.characteristics.push(CharacteristicObj {
                path: ch.clone(),
                service: svc,
                uuid: BATTERY_LEVEL_UUID.into(),
                handle: handle + 1,
                value: vec![level],
            });
            // Every level has a presentation format; it never decides the role.
            self.tree.descriptors.push(DescriptorObj {
                path: format!("{ch}/desc{:04x}", handle + 3),
                characteristic: ch.clone(),
                uuid: "00002904-0000-1000-8000-00805f9b34fb".into(),
                value: vec![4, 0, 0xad, 0x27, 1, 6, 1],
            });
            if let Some(d) = desc {
                self.tree.descriptors.push(DescriptorObj {
                    path: format!("{ch}/desc{:04x}", handle + 4),
                    characteristic: ch.clone(),
                    uuid: USER_DESCRIPTION_UUID.into(),
                    value: d.to_vec(),
                });
            }
            self.reads.insert(ch, vec![level]);
            self
        }

        /// The owner's keyboard as measured after #45.
        pub fn measured() -> Self {
            Self::keyboard()
                .battery(0x10, 100, None)
                .battery(0x15, 77, Some(b"Peripheral 0"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn read(g: &mut FakeGatt) -> Reading {
        run(g, &Query::default()).unwrap()
    }

    fn pct(r: &Reading) -> (Option<u8>, Option<u8>) {
        (r.left.percent, r.right.percent)
    }

    #[test]
    fn both_descriptions_present() {
        let r = read(&mut FakeGatt::measured());
        assert_eq!(pct(&r), (Some(100), Some(77)));
        assert_eq!(r.note, None);
        assert_eq!(r.device.address, "DD:1F:23:76:6E:71");
        assert_eq!(r.device.name, DEVICE_NAME);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "source": "bluez",
                "device": {"name": "Eyelash Sofle", "address": "DD:1F:23:76:6E:71"},
                "left": {"percent": 100},
                "right": {"percent": 77},
                "note": null
            })
        );
    }

    #[test]
    fn peripheral_listed_first() {
        // The peripheral's service has the lower handle: roles still win.
        let mut g = FakeGatt::keyboard()
            .battery(0x10, 64, Some(b"Peripheral 0"))
            .battery(0x15, 87, None);
        let r = read(&mut g);
        assert_eq!(pct(&r), (Some(87), Some(64)));
        assert_eq!(r.note, None);
    }

    #[test]
    fn uncached_description_is_read() {
        let mut g = FakeGatt::measured();
        let d = g
            .tree
            .descriptors
            .iter_mut()
            .find(|d| d.uuid == USER_DESCRIPTION_UUID)
            .unwrap();
        d.value.clear();
        let path = d.path.clone();
        g.reads.insert(path, b"Peripheral 0\0".to_vec());
        // Swap the order so only the description can get it right.
        g.tree
            .services
            .iter_mut()
            .for_each(|s| s.handle = 0x30 - s.handle);
        assert_eq!(pct(&read(&mut g)), (Some(100), Some(77)));
    }

    #[test]
    fn descriptions_unreadable_fall_back_to_order() {
        // 0x2901 present but uncached, and its read fails.
        let mut g = FakeGatt::keyboard()
            .battery(0x15, 77, Some(b""))
            .battery(0x10, 100, None);
        let r = read(&mut g);
        assert_eq!(pct(&r), (Some(100), Some(77)));
        assert!(r.note.as_deref().unwrap().contains("handle order"));
    }

    #[test]
    fn no_descriptions_at_all_fall_back_to_order() {
        let mut g = FakeGatt::keyboard()
            .battery(0x15, 77, None)
            .battery(0x10, 100, None);
        let r = read(&mut g);
        assert_eq!(pct(&r), (Some(100), Some(77)));
        assert!(r.note.as_deref().unwrap().contains("handle order"));
    }

    #[test]
    fn only_one_service() {
        let r = read(&mut FakeGatt::keyboard().battery(0x10, 90, None));
        assert_eq!(pct(&r), (Some(90), None));
        assert!(r.note.as_deref().unwrap().contains("#45"));
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["right"], serde_json::json!({"percent": null}));
    }

    #[test]
    fn read_failure_uses_cached_value() {
        let mut g = FakeGatt::measured();
        g.reads.retain(|p, _| !p.ends_with("char0016"));
        let r = read(&mut g);
        assert_eq!(pct(&r), (Some(100), Some(77)));
        assert!(r.note.as_deref().unwrap().contains("cached"));
    }

    #[test]
    fn both_reads_failing_is_an_error() {
        let mut g = FakeGatt::measured();
        g.reads.clear();
        g.tree
            .characteristics
            .iter_mut()
            .for_each(|c| c.value.clear());
        let e = run(&mut g, &Query::default()).unwrap_err();
        assert_eq!(e.exit_code(), crate::error::exit::TRANSPORT);
    }

    #[test]
    fn not_connected_and_unknown_are_not_found() {
        let mut g = FakeGatt::measured();
        g.tree.devices[0].connected = false;
        let e = run(&mut g, &Query::default()).unwrap_err();
        assert_eq!(e.exit_code(), crate::error::exit::NOT_FOUND);
        assert!(e.to_string().contains("not connected"), "{e}");

        let mut g = FakeGatt::default();
        let e = run(&mut g, &Query::default()).unwrap_err();
        assert!(e.to_string().contains("known to BlueZ"), "{e}");
        assert!(e.to_string().contains("no Bluetooth adapter"), "{e}");
    }

    #[test]
    fn address_override_and_duplicates() {
        let mut g = FakeGatt::measured();
        let mut other = g.tree.devices[0].clone();
        other.path = "/org/bluez/hci0/dev_11_22_33_44_55_66".into();
        other.address = "11:22:33:44:55:66".into();
        g.tree.devices.insert(0, other);
        // By name: two match, the first wins with a warning (it has no
        // Battery Service, so that's an error here).
        let e = run(&mut g, &Query::default()).unwrap_err();
        assert!(e.to_string().contains("11:22:33:44:55:66"), "{e}");
        // By address (case-insensitive): the right one.
        let r = run(
            &mut g,
            &Query {
                address: Some("dd:1f:23:76:6e:71".into()),
            },
        )
        .unwrap();
        assert_eq!(pct(&r), (Some(100), Some(77)));
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn duplicate_warning() {
        let mut g = FakeGatt::measured();
        let mut other = g.tree.devices[0].clone();
        other.path = "/org/bluez/hci0/dev_11_22_33_44_55_66".into();
        other.address = "11:22:33:44:55:66".into();
        g.tree.devices.push(other);
        let r = read(&mut g);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("--bt-address"));
    }

    #[test]
    fn threshold_classes() {
        let t = Thresholds::default();
        let mut r = read(&mut FakeGatt::measured());
        assert_eq!(t.classify(&r), Level::Ok);
        r.right.percent = Some(19);
        assert_eq!(t.classify(&r), Level::Warning);
        r.right.percent = Some(20);
        assert_eq!(t.classify(&r), Level::Ok);
        r.left.percent = Some(9);
        assert_eq!(t.classify(&r), Level::Critical);
        r.left.percent = None;
        r.right.percent = None;
        assert_eq!(t.classify(&r), Level::Unknown);
        let custom = Thresholds {
            warn_below: 90,
            critical_below: 10,
        };
        assert_eq!(
            custom.classify(&read(&mut FakeGatt::measured())),
            Level::Warning
        );
    }
}
