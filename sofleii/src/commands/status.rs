//! `sofleii status`: a one-shot summary (device, Studio lock, active BLE
//! profile, output priority), the split link from `cormoran_ble`'s
//! `get_split_info` ([`crate::commands::split`]), and both batteries, read
//! over BLE GATT by [`crate::commands::battery`] and merged in with
//! [`Status::set_battery`].

use serde::Serialize;

use crate::Result;
use crate::client::{Client, rpc::ble};
use crate::commands::battery::{self, Reading};
use crate::commands::info::{self, Device, Studio};
use crate::commands::profiles::{self, Profile, Profiles};
use crate::commands::split::{self, UNKNOWN_NOTE};
use crate::proto::core::LockState;
use crate::transport::{Transport, TransportInfo, TransportKind};

pub use split::Split;

/// Why both batteries are `null` when nothing tried to read them.
pub const BATTERY_NOTE: &str = "unavailable: not read";
/// Why `split` is `null` when nothing tried to read it.
pub const SPLIT_NOT_READ: &str = "unavailable: not read";
/// Why `split` is `null` on firmware without `cormoran_ble`.
pub const SPLIT_NO_SUBSYSTEM: &str = "unavailable: no cormoran_ble subsystem";

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub device: Device,
    pub studio: Studio,
    /// `"usb"` or `"ble"`: where keystrokes go when both are available.
    pub output_priority: Option<&'static str>,
    pub max_profiles: Option<u32>,
    pub bonded_profiles: Option<u32>,
    /// Index of the active BLE profile.
    pub active_profile: Option<u32>,
    /// The active profile's slot (same shape as in `profiles`).
    pub profile: Option<Profile>,
    /// The split link; `null` when it couldn't be read (see `split_note`),
    /// `known: false` on firmware without the split-info fix.
    pub split: Option<Split>,
    /// Why `split` is `null` or unknown; `null` when it is known.
    pub split_note: Option<String>,
    pub battery: Battery,
}

/// Both halves, from the same BLE reader as `sofleii battery`. Each is
/// `null` when BLE isn't available, and `note` says why. A right half known
/// to be off the split link is `{"percent": null, "state": "disconnected"}`.
#[derive(Debug, Clone, Serialize)]
pub struct Battery {
    pub left: Option<BatteryLevel>,
    pub right: Option<BatteryLevel>,
    pub note: Option<String>,
}

pub use battery::BatteryLevel;

impl Battery {
    /// The batteries from a `battery` reading, or `null`s with the reason.
    pub fn from_reading(r: Result<Reading>) -> Self {
        match r {
            Ok(r) => Battery {
                left: Some(r.left),
                right: Some(r.right),
                note: r.note,
            },
            Err(e) => Battery {
                left: None,
                right: None,
                note: Some(format!("unavailable: {e}")),
            },
        }
    }

    /// Drops the right half's level when the split link is known to be
    /// down: the central's battery proxy keeps a stale 0% for it. An unknown
    /// link (older firmware) keeps the level as read.
    pub fn with_split(mut self, split: Option<&Split>) -> Self {
        if split.is_some_and(Split::disconnected) {
            self.right = Some(BatteryLevel::disconnected());
        }
        self
    }
}

impl Status {
    /// The split read's result: known, unknown (with [`UNKNOWN_NOTE`]), or
    /// `null` with the error. Never fails `status`.
    pub fn set_split(&mut self, r: Result<Split>) {
        (self.split, self.split_note) = match r {
            Ok(s) if s.known => (Some(s), None),
            Ok(s) => (Some(s), Some(UNKNOWN_NOTE.into())),
            Err(e) => (None, Some(format!("unavailable: {e}"))),
        };
    }

    /// The batteries from a `battery` reading, with the split link's
    /// [`Battery::with_split`] applied (so set the split first).
    pub fn set_battery(&mut self, r: Result<Reading>) {
        self.battery = Battery::from_reading(r).with_split(self.split.as_ref());
    }
}

pub fn run<T: Transport>(client: &mut Client<T>) -> Result<Status> {
    let dev = client.get_device_info()?;
    let lock = client.get_lock_state()?;
    // Looked up by identifier; a firmware without cormoran_ble still gets a
    // status, with the profile and split fields null.
    let ble_index = client
        .list_custom_subsystems()?
        .into_iter()
        .find(|s| s.identifier == ble::IDENTIFIER)
        .map(|s| s.index);
    let profiles = match ble_index {
        Some(i) => Some(profiles::fetch(client, i)?),
        None => None,
    };
    let mut s = build(
        Device {
            name: dev.name,
            serial_number: (!dev.serial_number.is_empty()).then(|| info::hex(&dev.serial_number)),
        },
        lock == LockState::ZmkStudioCoreLockStateLocked,
        profiles,
    );
    // Extra context, like the output priority: a failure is a note.
    match ble_index {
        Some(i) => s.set_split(split::fetch(client, i)),
        None => s.split_note = Some(SPLIT_NO_SUBSYSTEM.into()),
    }
    Ok(s)
}

/// Best guess of where keystrokes go: the output priority, with ZMK's
/// fallback to the other endpoint when the preferred one isn't connected.
///
/// - Over USB, USB counts as connected (we're talking over it).
/// - Over BLE, whether USB is plugged in is unknown, so only the priority
///   and the active profile's connection count: a connected profile means
///   `ble`. That also matches the firmware, which only answers Studio on the
///   selected endpoint, so a reply over BLE means BLE is selected (and, with
///   priority `usb`, that USB isn't connected).
pub fn effective_output(transport: &TransportInfo, s: &Status) -> Option<&'static str> {
    effective(transport, s.output_priority, s.profile.as_ref())
}

/// [`effective_output`] from its parts: the output priority and the active
/// profile, as `profiles` (and the session's status reads) report them.
pub fn effective(
    transport: &TransportInfo,
    priority: Option<&str>,
    active: Option<&Profile>,
) -> Option<&'static str> {
    let usb = transport.kind == TransportKind::Usb;
    let ble = active.is_some_and(|p| p.connected);
    match (priority, usb, ble) {
        (Some("ble"), _, true) => Some("ble"),
        (_, true, _) => Some("usb"),
        (_, false, true) => Some("ble"),
        _ => None,
    }
}

/// Pure mapping from the decoded replies to [`Status`]; the split and the
/// batteries are filled in after ([`Status::set_split`], [`Status::set_battery`]).
pub fn build(device: Device, locked: bool, profiles: Option<Profiles>) -> Status {
    let p = profiles.as_ref();
    Status {
        device,
        studio: Studio { locked },
        output_priority: p.and_then(|p| p.output_priority),
        max_profiles: p.map(|p| p.max_profiles),
        bonded_profiles: p.map(|p| p.profiles.iter().filter(|x| x.bonded).count() as u32),
        active_profile: p.and_then(|p| p.active_profile),
        profile: p.and_then(|p| p.active().cloned()),
        split: None,
        split_note: Some(SPLIT_NOT_READ.into()),
        battery: Battery {
            left: None,
            right: None,
            note: Some(BATTERY_NOTE.into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ResponseSubsystem;
    use crate::proto::{ble_management as bm, core, custom};
    use crate::transport::mock::{Mock, ble_reply};
    use std::time::Duration;

    fn slot(index: u32, active: bool, connected: bool, open: bool, addr: &str) -> bm::ProfileInfo {
        bm::ProfileInfo {
            index,
            name: "".into(),
            address: addr.into(),
            is_connected: connected,
            is_open: open,
            is_active: active,
        }
    }

    fn core_reply(r: core::response::ResponseType) -> ResponseSubsystem {
        ResponseSubsystem::Core(core::Response {
            response_type: Some(r),
        })
    }

    /// The six replies `status` expects, in request order, with
    /// `cormoran_ble` at subsystem index `ble`.
    fn replies(
        ble: u32,
        slots: Vec<bm::ProfileInfo>,
        priority: bm::OutputPriority,
    ) -> Vec<ResponseSubsystem> {
        use bm::response::ResponseType as R;
        vec![
            core_reply(core::response::ResponseType::GetDeviceInfo(
                core::GetDeviceInfoResponse {
                    name: "Eyelash Sofle".into(),
                    serial_number: vec![0x94, 0x70],
                },
            )),
            core_reply(core::response::ResponseType::GetLockState(
                LockState::ZmkStudioCoreLockStateUnlocked as i32,
            )),
            ResponseSubsystem::Custom(custom::Response {
                response_type: Some(custom::response::ResponseType::ListCustomSubsystems(
                    custom::ListCustomSubsystemResponse {
                        subsystems: vec![
                            custom::CustomSubsystemInfo {
                                index: 1 - ble,
                                identifier: "cormoran_rsr".into(),
                                ui_url: vec![],
                            },
                            custom::CustomSubsystemInfo {
                                index: ble,
                                identifier: ble::IDENTIFIER.into(),
                                ui_url: vec![],
                            },
                        ],
                    },
                )),
            }),
            ble_reply(
                ble,
                R::GetProfiles(bm::GetProfilesResponse {
                    max_profiles: 5,
                    profiles: slots,
                }),
            ),
            ble_reply(
                ble,
                R::GetOutputPriority(bm::GetOutputPriorityResponse {
                    priority: priority as i32,
                }),
            ),
            split_reply(ble, Some(true)),
        ]
    }

    /// `get_split_info`'s reply: `Some(connected)` from the fixed firmware,
    /// `None` for the empty reply of older firmware.
    fn split_reply(ble: u32, connected: Option<bool>) -> ResponseSubsystem {
        ble_reply(
            ble,
            bm::response::ResponseType::GetSplitInfo(bm::GetSplitInfoResponse {
                info: connected.map(|c| bm::SplitInfo {
                    is_split: true,
                    is_central: true,
                    peripheral_connected: c,
                    central_bonded: false,
                }),
            }),
        )
    }

    /// [`replies`] with the split reply replaced.
    fn with_split(connected: Option<bool>) -> Vec<ResponseSubsystem> {
        let mut r = replies(
            0,
            vec![slot(0, true, true, false, "")],
            bm::OutputPriority::Usb,
        );
        *r.last_mut().unwrap() = split_reply(0, connected);
        r
    }

    fn run_mock(replies: Vec<ResponseSubsystem>) -> Status {
        let mut c = Client::with_first_id(Mock::replying(replies), Duration::from_secs(1), 1);
        run(&mut c).unwrap()
    }

    #[test]
    fn maps_active_connected_profile() {
        // cormoran_ble at index 1 here, to prove it's resolved by identifier.
        let s = run_mock(replies(
            1,
            vec![
                slot(0, false, false, false, "11:22:33:44:55:66"),
                slot(1, true, true, false, "84:9E:56:52:6A:D2"),
                slot(2, false, false, true, "00:00:00:00:00:00"),
            ],
            bm::OutputPriority::Ble,
        ));
        assert_eq!(s.device.name, "Eyelash Sofle");
        assert_eq!(s.device.serial_number.as_deref(), Some("9470"));
        assert!(!s.studio.locked);
        assert_eq!(s.output_priority, Some("ble"));
        assert_eq!(s.max_profiles, Some(5));
        assert_eq!(s.bonded_profiles, Some(2));
        assert_eq!(s.active_profile, Some(1));
        let p = s.profile.as_ref().unwrap();
        assert!(p.connected && p.bonded);
        assert_eq!(p.address.as_deref(), Some("84:9E:56:52:6A:D2"));
        let sp = s.split.unwrap();
        assert!(sp.known);
        assert_eq!(sp.connected, Some(true));
        assert_eq!(s.split_note, None);
    }

    #[test]
    fn json_keeps_null_fields() {
        let s = run_mock(replies(
            0,
            vec![slot(0, true, false, true, "")],
            bm::OutputPriority::Usb,
        ));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["split"],
            serde_json::json!({"known": true, "connected": true, "is_split": true, "is_central": true})
        );
        assert_eq!(v["split_note"], serde_json::Value::Null);
        assert_eq!(v["battery"]["left"], serde_json::Value::Null);
        assert_eq!(v["battery"]["right"], serde_json::Value::Null);
        assert!(v["battery"]["note"].is_string());
        assert_eq!(v["profile"]["address"], serde_json::Value::Null);
        assert_eq!(v["profile"]["bonded"], false);
        assert_eq!(v["output_priority"], "usb");
        // A filled battery serialises as {"percent": n, "state": null}.
        let lvl = serde_json::to_value(BatteryLevel {
            percent: Some(80),
            state: None,
        })
        .unwrap();
        assert_eq!(lvl, serde_json::json!({"percent": 80, "state": null}));
    }

    #[test]
    fn battery_filled_from_reading() {
        use crate::commands::battery::{Query, fake::FakeGatt};
        let mut s = run_mock(replies(0, vec![], bm::OutputPriority::Usb));
        s.battery =
            Battery::from_reading(battery::run(&mut FakeGatt::measured(), &Query::default()));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["battery"],
            serde_json::json!({"left": {"percent": 100, "state": null}, "right": {"percent": 77, "state": null}, "note": null})
        );
    }

    #[test]
    fn battery_unavailable_keeps_nulls_with_reason() {
        let mut s = run_mock(replies(0, vec![], bm::OutputPriority::Usb));
        s.battery = Battery::from_reading(Err(crate::Error::NotFound(
            "BlueZ unavailable: org.bluez is not running on the system D-Bus".into(),
        )));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["battery"]["left"], serde_json::Value::Null);
        assert_eq!(v["battery"]["right"], serde_json::Value::Null);
        let note = v["battery"]["note"].as_str().unwrap();
        assert!(
            note.starts_with("unavailable: ") && note.contains("org.bluez"),
            "{note}"
        );
    }

    #[test]
    fn without_ble_subsystem_profile_fields_are_null() {
        let mut r = replies(0, vec![], bm::OutputPriority::Usb);
        r.truncate(3);
        if let ResponseSubsystem::Custom(custom::Response {
            response_type: Some(custom::response::ResponseType::ListCustomSubsystems(l)),
        }) = &mut r[2]
        {
            l.subsystems.retain(|s| s.identifier != ble::IDENTIFIER);
        }
        let s = run_mock(r);
        assert_eq!(s.active_profile, None);
        assert!(s.profile.is_none());
        assert_eq!(s.output_priority, None);
        assert!(s.split.is_none());
        assert_eq!(s.split_note.as_deref(), Some(SPLIT_NO_SUBSYSTEM));
    }

    #[test]
    fn split_disconnected() {
        let s = run_mock(with_split(Some(false)));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["split"],
            serde_json::json!({"known": true, "connected": false, "is_split": true, "is_central": true})
        );
        assert_eq!(v["split_note"], serde_json::Value::Null);
    }

    #[test]
    fn split_empty_reply_is_unknown_with_a_note() {
        // Older firmware: get_split_info answers, but with no `info`.
        let s = run_mock(with_split(None));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["split"],
            serde_json::json!({"known": false, "connected": null, "is_split": null, "is_central": null})
        );
        assert_eq!(v["split_note"], UNKNOWN_NOTE);
    }

    #[test]
    fn split_error_is_a_note_not_a_failure() {
        let mut r = with_split(None);
        *r.last_mut().unwrap() = ble_reply(
            0,
            bm::response::ResponseType::Error(bm::ErrorResponse {
                message: "nope".into(),
            }),
        );
        let s = run_mock(r);
        assert!(s.split.is_none());
        let note = s.split_note.unwrap();
        assert!(
            note.starts_with("unavailable: ") && note.contains("nope"),
            "{note}"
        );
    }

    fn measured() -> Result<Reading> {
        use crate::commands::battery::{Query, fake::FakeGatt};
        battery::run(&mut FakeGatt::measured(), &Query::default())
    }

    #[test]
    fn right_battery_is_disconnected_when_the_link_is_down() {
        let mut s = run_mock(with_split(Some(false)));
        let mut r = measured().unwrap();
        // The central's proxy keeps 0% for a gone peripheral.
        r.right.percent = Some(0);
        s.set_battery(Ok(r));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["battery"],
            serde_json::json!({
                "left": {"percent": 100, "state": null},
                "right": {"percent": null, "state": "disconnected"},
                "note": null
            })
        );
        // Also when BlueZ couldn't be read: the link state is still known.
        s.set_battery(Err(crate::Error::NotFound("no BLE".into())));
        assert_eq!(s.battery.left, None);
        assert_eq!(s.battery.right, Some(BatteryLevel::disconnected()));
    }

    #[test]
    fn right_battery_kept_when_connected_or_unknown() {
        for connected in [Some(true), None] {
            let mut s = run_mock(with_split(connected));
            let mut r = measured().unwrap();
            r.right.percent = Some(0);
            s.set_battery(Ok(r));
            assert_eq!(
                s.battery.right.unwrap().percent,
                Some(0),
                "split {connected:?}"
            );
            assert_eq!(s.battery.right.unwrap().state, None);
        }
    }
}
