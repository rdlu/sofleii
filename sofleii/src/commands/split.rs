//! The split link from `cormoran_ble`'s `get_split_info`: whether the right
//! (peripheral) half is connected to the left (central) one.
//!
//! Our fork of zmk-module-ble-management (`rdlu/zmk-module-ble-management`,
//! `5ea5b4f`) fills the reply's `info` and reads the central's split
//! transport status. cormoran's module before that fix answers with no
//! `info` at all, which maps to [`Split::unknown`], never to disconnected.

use serde::Serialize;

use crate::client::{Client, rpc::ble};
use crate::proto::ble_management::{self, response::ResponseType};
use crate::transport::Transport;
use crate::{Error, Result};

/// Why `known` is `false`.
pub const UNKNOWN_NOTE: &str = "unknown: the firmware's get_split_info carries no info (ble-management without the split-info fix)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Split {
    /// The firmware reported the link. `false` for older firmware, whose
    /// reply is empty; every field below is then `null`.
    pub known: bool,
    /// The right half is connected to the left. `null` when unknown, and
    /// on a half that isn't a split central.
    pub connected: Option<bool>,
    pub is_split: Option<bool>,
    /// The half answering Studio is the central (always, on this board).
    pub is_central: Option<bool>,
}

impl Split {
    /// What an empty reply (older firmware) means.
    pub const fn unknown() -> Self {
        Self {
            known: false,
            connected: None,
            is_split: None,
            is_central: None,
        }
    }

    /// The right half is known to be off the link (powered off or out of
    /// range).
    pub fn disconnected(&self) -> bool {
        self.connected == Some(false)
    }
}

pub fn run<T: Transport>(client: &mut Client<T>) -> Result<Split> {
    let index = client.resolve_subsystem(ble::IDENTIFIER)?;
    fetch(client, index)
}

/// Reads the split link from `cormoran_ble` at a subsystem index the caller
/// already resolved (shared by `status` and the session).
pub fn fetch<T: Transport>(client: &mut Client<T>, index: u32) -> Result<Split> {
    let payload = client.custom_call("get_split_info", index, ble::get_split_info())?;
    match ble::decode(&payload)? {
        ResponseType::GetSplitInfo(r) => Ok(from_response(r)),
        _ => Err(Error::Protocol(
            "get_split_info: unexpected response type".into(),
        )),
    }
}

/// Pure mapping from the decoded reply.
pub fn from_response(r: ble_management::GetSplitInfoResponse) -> Split {
    let Some(i) = r.info else {
        return Split::unknown();
    };
    Split {
        known: true,
        // Only a split central knows whether its peripheral is there.
        connected: (i.is_split && i.is_central).then_some(i.peripheral_connected),
        is_split: Some(i.is_split),
        is_central: Some(i.is_central),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ble_management::{GetSplitInfoResponse, SplitInfo};

    fn info(split: bool, central: bool, connected: bool) -> GetSplitInfoResponse {
        GetSplitInfoResponse {
            info: Some(SplitInfo {
                is_split: split,
                is_central: central,
                peripheral_connected: connected,
                central_bonded: false,
            }),
        }
    }

    #[test]
    fn known_connected() {
        let s = from_response(info(true, true, true));
        assert!(s.known && !s.disconnected());
        assert_eq!(s.connected, Some(true));
    }

    #[test]
    fn known_disconnected() {
        let s = from_response(info(true, true, false));
        assert!(s.known && s.disconnected());
        assert_eq!(s.connected, Some(false));
    }

    #[test]
    fn empty_reply_is_unknown_not_disconnected() {
        let s = from_response(GetSplitInfoResponse { info: None });
        assert_eq!(s, Split::unknown());
        assert!(!s.disconnected());
    }

    #[test]
    fn not_a_split_central_has_no_link_state() {
        assert_eq!(from_response(info(false, false, false)).connected, None);
        assert_eq!(from_response(info(true, false, false)).connected, None);
    }

    #[test]
    fn json_has_explicit_nulls() {
        let v = serde_json::to_value(Split::unknown()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"known": false, "connected": null, "is_split": null, "is_central": null})
        );
    }
}
