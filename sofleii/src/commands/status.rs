//! `sofleii status`: a one-shot summary (device, Studio lock, active BLE
//! profile, output priority) plus the split link and batteries, which the
//! firmware cannot report yet and are `null` with a note.

use serde::Serialize;

use crate::Result;
use crate::client::{Client, rpc::ble};
use crate::commands::info::{self, Device, Studio};
use crate::commands::profiles::{self, Profile, Profiles};
use crate::proto::core::LockState;
use crate::transport::Transport;

/// Why `split` is `null`.
pub const SPLIT_NOTE: &str = "unavailable: cormoran_ble get_split_info is broken upstream; \
     it is always empty and hardcodes peripheral_connected=false";
/// Why both batteries are `null`.
pub const BATTERY_NOTE: &str = "unavailable: not read yet; the firmware exposes both levels since #45, read over BLE GATT by the upcoming `sofleii battery`";

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
    /// Split link state. Always `null` for now, see `split_note`.
    pub split: Option<Split>,
    pub split_note: Option<&'static str>,
    pub battery: Battery,
}

/// Reserved shape for when the split link becomes readable.
#[derive(Debug, Clone, Serialize)]
pub struct Split {
    pub peripheral_connected: bool,
}

/// Both halves; each is `null` until the firmware reports it. Filling them
/// later is additive: `{"percent": n}` replaces `null`, no schema bump.
#[derive(Debug, Clone, Serialize)]
pub struct Battery {
    pub left: Option<BatteryLevel>,
    pub right: Option<BatteryLevel>,
    pub note: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BatteryLevel {
    pub percent: Option<u8>,
}

pub fn run<T: Transport>(client: &mut Client<T>) -> Result<Status> {
    let dev = client.get_device_info()?;
    let lock = client.get_lock_state()?;
    // Looked up by identifier; a firmware without cormoran_ble still gets a
    // status, with the profile fields null.
    let ble_index = client
        .list_custom_subsystems()?
        .into_iter()
        .find(|s| s.identifier == ble::IDENTIFIER)
        .map(|s| s.index);
    let profiles = match ble_index {
        Some(i) => Some(profiles::fetch(client, i)?),
        None => None,
    };
    Ok(build(
        Device {
            name: dev.name,
            serial_number: (!dev.serial_number.is_empty()).then(|| info::hex(&dev.serial_number)),
        },
        lock == LockState::ZmkStudioCoreLockStateLocked,
        profiles,
    ))
}

/// Pure mapping from the decoded replies to [`Status`].
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
        split_note: Some(SPLIT_NOTE),
        battery: Battery {
            left: None,
            right: None,
            note: Some(BATTERY_NOTE),
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

    /// The five replies `status` expects, in request order, with
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
        ]
    }

    fn run_mock(replies: Vec<ResponseSubsystem>) -> Status {
        let mut c = Client::new(Mock::replying(replies), Duration::from_secs(1));
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
        assert!(s.split.is_none() && s.split_note.is_some());
    }

    #[test]
    fn json_keeps_null_fields() {
        let s = run_mock(replies(
            0,
            vec![slot(0, true, false, true, "")],
            bm::OutputPriority::Usb,
        ));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["split"], serde_json::Value::Null);
        assert_eq!(v["battery"]["left"], serde_json::Value::Null);
        assert_eq!(v["battery"]["right"], serde_json::Value::Null);
        assert!(v["battery"]["note"].is_string());
        assert_eq!(v["profile"]["address"], serde_json::Value::Null);
        assert_eq!(v["profile"]["bonded"], false);
        assert_eq!(v["output_priority"], "usb");
        // A filled battery serialises as {"percent": n}.
        let lvl = serde_json::to_value(BatteryLevel { percent: Some(80) }).unwrap();
        assert_eq!(lvl, serde_json::json!({"percent": 80}));
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
    }
}
