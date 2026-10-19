//! Pure request builders and message encode/decode (no I/O), so the wire
//! format can be tested without a keyboard.

use prost::Message;

use crate::proto::{ble_management, core, custom, studio};
use crate::{Error, Result};

pub use studio::request::Subsystem as RequestSubsystem;
pub use studio::request_response::Subsystem as ResponseSubsystem;

/// Encodes a `zmk.studio.Request` (unframed).
pub fn encode_request(request_id: u32, subsystem: RequestSubsystem) -> Vec<u8> {
    studio::Request {
        request_id,
        subsystem: Some(subsystem),
    }
    .encode_to_vec()
}

/// Decodes one unframed `zmk.studio.Response`.
pub fn decode_response(frame: &[u8]) -> Result<studio::response::Type> {
    let resp = studio::Response::decode(frame)
        .map_err(|e| Error::Protocol(format!("undecodable response frame: {e}")))?;
    resp.r#type
        .ok_or_else(|| Error::Protocol("response frame with no type".into()))
}

pub fn get_device_info() -> RequestSubsystem {
    RequestSubsystem::Core(core::Request {
        request_type: Some(core::request::RequestType::GetDeviceInfo(true)),
    })
}

pub fn get_lock_state() -> RequestSubsystem {
    RequestSubsystem::Core(core::Request {
        request_type: Some(core::request::RequestType::GetLockState(true)),
    })
}

pub fn list_custom_subsystems() -> RequestSubsystem {
    RequestSubsystem::Custom(custom::Request {
        request_type: Some(custom::request::RequestType::ListCustomSubsystems(
            custom::ListCustomSubsystemRequest {},
        )),
    })
}

pub fn custom_call(subsystem_index: u32, payload: Vec<u8>) -> RequestSubsystem {
    RequestSubsystem::Custom(custom::Request {
        request_type: Some(custom::request::RequestType::Call(custom::CallRequest {
            subsystem_index,
            payload,
        })),
    })
}

/// `cormoran_ble` (zmk-module-ble-management) payloads.
pub mod ble {
    use super::*;
    use ble_management::request::RequestType;

    pub const IDENTIFIER: &str = "cormoran_ble";

    pub fn get_profiles() -> Vec<u8> {
        request(RequestType::GetProfiles(
            ble_management::GetProfilesRequest {},
        ))
    }

    pub fn get_output_priority() -> Vec<u8> {
        request(RequestType::GetOutputPriority(
            ble_management::GetOutputPriorityRequest {},
        ))
    }

    pub fn get_split_info() -> Vec<u8> {
        request(RequestType::GetSplitInfo(
            ble_management::GetSplitInfoRequest {},
        ))
    }

    pub fn switch_profile(index: u32) -> Vec<u8> {
        request(RequestType::SwitchProfile(
            ble_management::SwitchProfileRequest { index },
        ))
    }

    /// The caller checks the length first ([`NAME_MAX_BYTES`]): a longer
    /// name fails nanopb's decode on the keyboard.
    pub fn set_profile_name(index: u32, name: &str) -> Vec<u8> {
        request(RequestType::SetProfileName(
            ble_management::SetProfileNameRequest {
                index,
                name: name.into(),
            },
        ))
    }

    pub fn unpair_profile(index: u32) -> Vec<u8> {
        request(RequestType::UnpairProfile(
            ble_management::UnpairProfileRequest { index },
        ))
    }

    pub fn set_output_priority(priority: ble_management::OutputPriority) -> Vec<u8> {
        request(RequestType::SetOutputPriority(
            ble_management::SetOutputPriorityRequest {
                priority: priority as i32,
            },
        ))
    }

    /// Longest profile name in UTF-8 bytes: `SetProfileNameRequest.name` is
    /// `max_size:32` in the module's `ble_management.options`, and nanopb's
    /// `max_size` counts the terminating NUL.
    pub const NAME_MAX_BYTES: usize = 31;

    fn request(r: RequestType) -> Vec<u8> {
        ble_management::Request {
            request_type: Some(r),
        }
        .encode_to_vec()
    }

    /// Decodes a `cormoran_ble` response payload, turning its `error`
    /// variant into [`Error::Rpc`].
    pub fn decode(payload: &[u8]) -> Result<ble_management::response::ResponseType> {
        let resp = ble_management::Response::decode(payload)
            .map_err(|e| Error::Protocol(format!("undecodable {IDENTIFIER} payload: {e}")))?;
        match resp.response_type {
            Some(ble_management::response::ResponseType::Error(e)) => {
                Err(Error::Rpc(format!("{IDENTIFIER}: {}", e.message)))
            }
            Some(t) => Ok(t),
            None => Err(Error::Protocol(format!("empty {IDENTIFIER} response"))),
        }
    }
}

/// `zmk__device_info` (zmk-feature-device-info, #46) payloads.
pub mod device_info {
    use super::*;
    use crate::proto::device_info as di;
    use di::{request::RequestType, response::ResponseType};

    /// The identifier is the C token passed to `ZMK_RPC_CUSTOM_SUBSYSTEM`,
    /// stringified (note the double underscore).
    pub const IDENTIFIER: &str = "zmk__device_info";

    pub fn get_device_info() -> Vec<u8> {
        di::Request {
            request_type: Some(RequestType::GetDeviceInfo(di::GetDeviceInfoRequest {})),
        }
        .encode_to_vec()
    }

    /// Decodes a `zmk__device_info` response payload, turning its `error`
    /// variant into [`Error::Rpc`].
    pub fn decode(payload: &[u8]) -> Result<di::DeviceInfoResponse> {
        let resp = di::Response::decode(payload)
            .map_err(|e| Error::Protocol(format!("undecodable {IDENTIFIER} payload: {e}")))?;
        match resp.response_type {
            Some(ResponseType::DeviceInfo(d)) => Ok(d),
            Some(ResponseType::Error(e)) => Err(Error::Rpc(format!("{IDENTIFIER}: {}", e.message))),
            None => Err(Error::Protocol(format!("empty {IDENTIFIER} response"))),
        }
    }
}
