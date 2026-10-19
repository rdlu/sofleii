//! `sofleii info`: device info, Studio lock state, custom subsystems, and the
//! firmware build info from the `zmk__device_info` subsystem (#46).

use serde::Serialize;

use crate::Result;
use crate::client::{Client, rpc::device_info};
use crate::proto::core::LockState;
use crate::proto::device_info as di;
use crate::transport::Transport;

/// Why `build` is null-filled on firmware without the subsystem.
pub const BUILD_MISSING_NOTE: &str = "unavailable: this firmware has no zmk__device_info \
     subsystem (built before #46); flash a newer build";
/// Why `build` is null-filled when the reply carried no `BuildInfo`.
pub const BUILD_EMPTY_NOTE: &str = "unavailable: zmk__device_info sent no build info";

#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub device: Device,
    /// Firmware build info, mirroring `zmk.device_info.BuildInfo`. On
    /// firmware without `zmk__device_info` every field is `null`,
    /// `available` is `false` and `note` says why; the command still exits 0.
    pub build: Build,
    /// `zmk.device_info.HardwareInfo`, from the same reply; all `null`
    /// when `build` is unavailable.
    pub hardware: Hardware,
    pub studio: Studio,
    pub custom_subsystems: Vec<CustomSubsystem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub name: String,
    /// Hex string (the firmware sends raw bytes); `null` if empty.
    pub serial_number: Option<String>,
}

/// `BuildInfo` with empty strings as `null`. A `*_dirty` flag is `null`
/// when its version is unknown (the firmware derives it from the version
/// string, so it means nothing then).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Build {
    pub available: bool,
    pub note: Option<String>,
    /// `git describe --dirty --always --tags` of the ZMK source.
    pub zmk_version: Option<String>,
    pub zmk_dirty: Option<bool>,
    /// `git describe --dirty --always --tags` of this repo (the zmk-config)
    /// at configure time: `v3.0.0-65-g1e468b7` locally, the bare short hash
    /// on CI (shallow checkout, no tags).
    pub zmk_config_version: Option<String>,
    pub zmk_config_dirty: Option<bool>,
    /// `git describe` of zmk-feature-device-info itself.
    pub module_version: Option<String>,
    pub module_dirty: Option<bool>,
    pub zephyr_version: Option<String>,
    /// UTC, ISO 8601 (`2026-10-17T20:50:06Z`), taken at configure time.
    pub build_timestamp: Option<String>,
    pub board: Option<String>,
    /// GNU build-id (SHA-1 of the linked image, hex): names the exact binary.
    pub build_hash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Hardware {
    /// `hwinfo_get_device_id()`, hex.
    pub device_id: Option<String>,
    /// Raw `hwinfo_get_reset_cause()` bitmask. On nRF52 the register
    /// accumulates until cleared, so several bits can be set.
    pub reset_cause: Option<u32>,
    /// `reset_cause` decoded into Zephyr's `RESET_*` names (lowercase).
    pub reset_causes: Option<Vec<&'static str>>,
    pub flash_size_kb: Option<u32>,
    pub sram_size_kb: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Studio {
    /// `true` when Studio requires an unlock before writes.
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CustomSubsystem {
    pub index: u32,
    pub identifier: String,
    pub ui_urls: Vec<String>,
}

pub fn run<T: Transport>(client: &mut Client<T>) -> Result<Info> {
    let dev = client.get_device_info()?;
    let lock = client.get_lock_state()?;
    let subsystems = client.list_custom_subsystems()?;
    // Resolved by identifier, never by index: indices change between builds.
    let di_index = subsystems
        .iter()
        .find(|s| s.identifier == device_info::IDENTIFIER)
        .map(|s| s.index);
    let (build, hardware) = match di_index {
        Some(i) => {
            let payload = client.custom_call(
                "zmk__device_info get_device_info",
                i,
                device_info::get_device_info(),
            )?;
            from_response(device_info::decode(&payload)?)
        }
        None => (Build::unavailable(BUILD_MISSING_NOTE), Hardware::default()),
    };
    Ok(Info {
        device: Device {
            name: dev.name,
            serial_number: (!dev.serial_number.is_empty()).then(|| hex(&dev.serial_number)),
        },
        build,
        hardware,
        studio: Studio {
            locked: lock == LockState::ZmkStudioCoreLockStateLocked,
        },
        custom_subsystems: subsystems
            .into_iter()
            .map(|s| CustomSubsystem {
                index: s.index,
                identifier: s.identifier,
                ui_urls: s.ui_url,
            })
            .collect(),
    })
}

impl Build {
    pub fn unavailable(note: &str) -> Self {
        Build {
            note: Some(note.into()),
            ..Default::default()
        }
    }
}

/// Pure mapping from a decoded `DeviceInfoResponse`.
pub fn from_response(resp: di::DeviceInfoResponse) -> (Build, Hardware) {
    let build = match resp.build {
        Some(b) => {
            let (zmk_version, zmk_dirty) = versioned(b.zmk_version, b.zmk_dirty);
            let (zmk_config_version, zmk_config_dirty) =
                versioned(b.zmk_config_version, b.zmk_config_dirty);
            let (module_version, module_dirty) = versioned(b.module_version, b.module_dirty);
            Build {
                available: true,
                note: None,
                zmk_version,
                zmk_dirty,
                zmk_config_version,
                zmk_config_dirty,
                module_version,
                module_dirty,
                zephyr_version: non_empty(b.zephyr_version),
                build_timestamp: non_empty(b.build_timestamp),
                board: non_empty(b.board),
                build_hash: non_empty(b.build_hash),
            }
        }
        None => Build::unavailable(BUILD_EMPTY_NOTE),
    };
    let hardware = resp.hardware.map_or_else(Hardware::default, |h| Hardware {
        device_id: non_empty(h.device_id),
        reset_cause: Some(h.reset_cause),
        reset_causes: Some(reset_causes(h.reset_cause)),
        flash_size_kb: (h.flash_size_kb != 0).then_some(h.flash_size_kb),
        sram_size_kb: (h.sram_size_kb != 0).then_some(h.sram_size_kb),
    });
    (build, hardware)
}

fn versioned(version: String, dirty: bool) -> (Option<String>, Option<bool>) {
    let v = non_empty(version);
    let d = v.as_ref().map(|_| dirty);
    (v, d)
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// Zephyr `drivers/hwinfo.h` `RESET_*` bits, in bit order.
const RESET_CAUSES: [&str; 15] = [
    "pin",
    "software",
    "brownout",
    "por",
    "watchdog",
    "debug",
    "security",
    "low_power_wake",
    "cpu_lockup",
    "parity",
    "pll",
    "clock",
    "hardware",
    "user",
    "temperature",
];

pub fn reset_causes(mask: u32) -> Vec<&'static str> {
    RESET_CAUSES
        .iter()
        .enumerate()
        .filter(|(bit, _)| mask & (1 << bit) != 0)
        .map(|(_, name)| *name)
        .collect()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::client::ResponseSubsystem;
    use crate::proto::{core, custom};
    use crate::transport::mock::{Mock, custom_reply};
    use prost::Message;
    use std::time::Duration;

    fn core_reply(r: core::response::ResponseType) -> ResponseSubsystem {
        ResponseSubsystem::Core(core::Response {
            response_type: Some(r),
        })
    }

    fn subsystem(index: u32, identifier: &str) -> custom::CustomSubsystemInfo {
        custom::CustomSubsystemInfo {
            index,
            identifier: identifier.into(),
            ui_url: vec![],
        }
    }

    /// The replies `info` expects before the device-info call.
    fn base_replies(subsystems: Vec<custom::CustomSubsystemInfo>) -> Vec<ResponseSubsystem> {
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
                    custom::ListCustomSubsystemResponse { subsystems },
                )),
            }),
        ]
    }

    /// A reply shaped like the left half's, including a Zephyr device list
    /// long enough that the frame spans several transport reads.
    pub fn sample_response() -> di::DeviceInfoResponse {
        di::DeviceInfoResponse {
            build: Some(di::BuildInfo {
                zmk_version: "v0.3-198-ge5c9b691".into(),
                zmk_dirty: false,
                zmk_config_version: "v3.0.0-65-g1e468b7-dirty".into(),
                zmk_config_dirty: true,
                module_version: "8cedd2d".into(),
                module_dirty: false,
                zephyr_version: "4.1.0".into(),
                build_timestamp: "2026-10-17T20:50:06Z".into(),
                board: "nice_nano".into(),
                build_hash: "1e37f75107aeb7993596e9d27b668701ed790839".into(),
            }),
            hardware: Some(di::HardwareInfo {
                device_id: "a1b2c3d4e5f60718".into(),
                reset_cause: 0b1001, // pin + por
                flash_size_kb: 1024,
                sram_size_kb: 256,
            }),
            zephyr_devices: (0..60)
                .map(|i| di::ZephyrDevice {
                    name: format!("device@{i:08x}"),
                    ready: i % 7 != 0,
                })
                .collect(),
            zmk_config: Some(di::ZmkConfig {
                split_role: "central".into(),
                ..Default::default()
            }),
            runtime: Some(di::RuntimeStatus { uptime_ms: 123_456 }),
        }
    }

    fn device_info_reply(index: u32, r: di::response::ResponseType) -> ResponseSubsystem {
        custom_reply(
            index,
            di::Response {
                response_type: Some(r),
            }
            .encode_to_vec(),
        )
    }

    #[test]
    fn subsystem_present_is_resolved_by_identifier_and_decoded() {
        // device_info deliberately not at the index a naive client would guess.
        let mut replies = base_replies(vec![
            subsystem(0, "zmk__device_info"),
            subsystem(1, "cormoran_ble"),
            subsystem(2, "cormoran_rsr"),
        ]);
        replies.push(device_info_reply(
            0,
            di::response::ResponseType::DeviceInfo(sample_response()),
        ));
        let mock = Mock::replying(replies);
        let mut c = Client::new(mock, Duration::from_secs(1));
        let info = run(&mut c).unwrap();

        assert!(info.build.available);
        assert_eq!(info.build.note, None);
        assert_eq!(
            info.build.zmk_config_version.as_deref(),
            Some("v3.0.0-65-g1e468b7-dirty")
        );
        assert_eq!(info.build.zmk_config_dirty, Some(true));
        assert_eq!(info.build.zmk_dirty, Some(false));
        assert_eq!(info.build.board.as_deref(), Some("nice_nano"));
        assert_eq!(info.hardware.reset_causes, Some(vec!["pin", "por"]));
        assert_eq!(info.hardware.flash_size_kb, Some(1024));
        assert_eq!(info.custom_subsystems.len(), 3);
    }

    #[test]
    fn subsystem_missing_keeps_build_null_with_a_note() {
        let replies = base_replies(vec![
            subsystem(0, "cormoran_ble"),
            subsystem(1, "cormoran_rsr"),
        ]);
        let mut c = Client::new(Mock::replying(replies), Duration::from_secs(1));
        let info = run(&mut c).unwrap();
        assert_eq!(info.build, Build::unavailable(BUILD_MISSING_NOTE));
        assert_eq!(info.hardware, Hardware::default());
        assert_eq!(info.device.name, "Eyelash Sofle");
    }

    #[test]
    fn subsystem_error_reply_is_an_rpc_error() {
        let mut replies = base_replies(vec![subsystem(2, "zmk__device_info")]);
        replies.push(device_info_reply(
            2,
            di::response::ResponseType::Error(di::ErrorResponse {
                message: "Failed to process request".into(),
            }),
        ));
        let mut c = Client::new(Mock::replying(replies), Duration::from_secs(1));
        let err = run(&mut c).unwrap_err();
        assert!(
            matches!(&err, crate::Error::Rpc(m) if m.contains("Failed to process")),
            "{err}"
        );
        assert_eq!(err.exit_code(), crate::error::exit::TRANSPORT);
    }

    #[test]
    fn empty_strings_become_null_and_dirty_follows_version() {
        let (b, h) = from_response(di::DeviceInfoResponse {
            build: Some(di::BuildInfo {
                zmk_version: "abc1234".into(),
                ..Default::default()
            }),
            hardware: None,
            ..Default::default()
        });
        assert!(b.available);
        assert_eq!(b.zmk_dirty, Some(false));
        assert_eq!(b.zmk_config_version, None);
        assert_eq!(b.zmk_config_dirty, None);
        assert_eq!(b.build_hash, None);
        assert_eq!(h, Hardware::default());

        let (b, _) = from_response(di::DeviceInfoResponse::default());
        assert_eq!(b, Build::unavailable(BUILD_EMPTY_NOTE));
    }

    #[test]
    fn reset_cause_names() {
        assert_eq!(reset_causes(0), Vec::<&str>::new());
        assert_eq!(reset_causes(1 << 1 | 1 << 4), vec!["software", "watchdog"]);
        assert_eq!(reset_causes(1 << 14 | 1 << 31), vec!["temperature"]);
    }
}
