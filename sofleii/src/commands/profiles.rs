//! `sofleii profiles`: BLE profiles from the `cormoran_ble` custom subsystem.

use serde::Serialize;

use crate::client::{Client, rpc::ble};
use crate::proto::ble_management::{self, OutputPriority, response::ResponseType};
use crate::transport::Transport;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize)]
pub struct Profiles {
    pub max_profiles: u32,
    pub active_profile: Option<u32>,
    /// Where keystrokes go when both USB and BLE are available.
    pub output_priority: Option<&'static str>,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Profile {
    pub index: u32,
    pub active: bool,
    pub connected: bool,
    /// The slot holds a bond (the firmware's `!is_open`).
    pub bonded: bool,
    /// Peer MAC address; `null` for an empty slot. The firmware formats it
    /// as `"XX:… (random)"` but nanopb's `max_size:18` truncates it to the
    /// bare MAC, so the address type never reaches the host.
    pub address: Option<String>,
    /// Custom name set through `set_profile_name`.
    pub name: Option<String>,
}

pub fn run<T: Transport>(client: &mut Client<T>) -> Result<Profiles> {
    let index = client.resolve_subsystem(ble::IDENTIFIER)?;

    let payload = client.custom_call("get_profiles", index, ble::get_profiles())?;
    let ResponseType::GetProfiles(resp) = ble::decode(&payload)? else {
        return Err(Error::Protocol(
            "get_profiles: unexpected response type".into(),
        ));
    };

    // Output priority is optional extra context: a failure here shouldn't
    // hide the profiles.
    let output_priority = client
        .custom_call("get_output_priority", index, ble::get_output_priority())
        .and_then(|p| ble::decode(&p))
        .ok()
        .and_then(|r| match r {
            ResponseType::GetOutputPriority(o) => match OutputPriority::try_from(o.priority) {
                Ok(OutputPriority::Usb) => Some("usb"),
                Ok(OutputPriority::Ble) => Some("ble"),
                Err(_) => None,
            },
            _ => None,
        });

    Ok(from_response(resp, output_priority))
}

pub fn from_response(
    resp: ble_management::GetProfilesResponse,
    output_priority: Option<&'static str>,
) -> Profiles {
    let profiles: Vec<Profile> = resp.profiles.into_iter().map(profile).collect();
    Profiles {
        max_profiles: resp.max_profiles,
        active_profile: profiles.iter().find(|p| p.active).map(|p| p.index),
        output_priority,
        profiles,
    }
}

fn profile(p: ble_management::ProfileInfo) -> Profile {
    Profile {
        index: p.index,
        active: p.is_active,
        connected: p.is_connected,
        bonded: !p.is_open,
        address: parse_address(&p.address),
        name: non_empty(p.name),
    }
}

/// Strips a `" (type)"` suffix if a future firmware sends one, and maps the
/// empty slot's `00:00:00:00:00:00` (and `""`) to `None`.
fn parse_address(s: &str) -> Option<String> {
    let mac = s.split_once(" (").map_or(s, |(mac, _)| mac).trim();
    let unset = mac.is_empty() || mac.chars().all(|c| c == '0' || c == ':');
    (!unset).then(|| mac.to_string())
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_parsing() {
        assert_eq!(
            parse_address("84:9E:56:52:6A:D2"),
            Some("84:9E:56:52:6A:D2".into())
        );
        assert_eq!(
            parse_address("AA:BB:CC:DD:EE:FF (random)"),
            Some("AA:BB:CC:DD:EE:FF".into())
        );
        assert_eq!(parse_address("00:00:00:00:00:00"), None);
        assert_eq!(parse_address(""), None);
    }

    #[test]
    fn open_slot_is_not_bonded_and_active_is_found() {
        let resp = ble_management::GetProfilesResponse {
            max_profiles: 2,
            profiles: vec![
                ble_management::ProfileInfo {
                    index: 0,
                    name: "".into(),
                    address: "AA:BB:CC:DD:EE:FF (public)".into(),
                    is_connected: true,
                    is_open: false,
                    is_active: false,
                },
                ble_management::ProfileInfo {
                    index: 1,
                    name: "".into(),
                    address: "".into(),
                    is_connected: false,
                    is_open: true,
                    is_active: true,
                },
            ],
        };
        let p = from_response(resp, None);
        assert_eq!(p.active_profile, Some(1));
        assert!(p.profiles[0].bonded);
        assert!(!p.profiles[1].bonded);
        assert_eq!(p.profiles[1].address, None);
        assert_eq!(p.profiles[0].name, None);
    }
}
