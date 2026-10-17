//! `sofleii info`: device info, Studio lock state and custom subsystems.

use serde::Serialize;

use crate::Result;
use crate::client::Client;
use crate::proto::core::LockState;
use crate::transport::Transport;

#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub device: Device,
    /// Firmware build info. Not exposed by the firmware yet (#46), so every
    /// field is `null` for now; the shape is fixed so scripts can rely on it.
    pub build: Build,
    pub studio: Studio,
    pub custom_subsystems: Vec<CustomSubsystem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub name: String,
    /// Hex string (the firmware sends raw bytes); `null` if empty.
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Build {
    pub available: bool,
    pub zmk_version: Option<String>,
    pub zmk_config_version: Option<String>,
    pub zmk_config_dirty: Option<bool>,
    pub zephyr_version: Option<String>,
    pub board: Option<String>,
    pub build_timestamp: Option<String>,
    pub build_hash: Option<String>,
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
    Ok(Info {
        device: Device {
            name: dev.name,
            serial_number: (!dev.serial_number.is_empty()).then(|| hex(&dev.serial_number)),
        },
        build: Build::default(),
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

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}
