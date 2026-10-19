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
        ble_management::Request {
            request_type: Some(RequestType::GetProfiles(
                ble_management::GetProfilesRequest {},
            )),
        }
        .encode_to_vec()
    }

    pub fn get_output_priority() -> Vec<u8> {
        ble_management::Request {
            request_type: Some(RequestType::GetOutputPriority(
                ble_management::GetOutputPriorityRequest {},
            )),
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
