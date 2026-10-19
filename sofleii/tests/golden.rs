//! Golden request bytes from #44, captured from the v3.0.0 firmware, plus
//! live `get_split_info` requests and replies.

use sofleii::client::rpc;
use sofleii::commands::split::{self, Split};
use sofleii::framing;

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

#[test]
fn get_device_info_request() {
    assert_eq!(
        rpc::encode_request(1, rpc::get_device_info()),
        hex("08 01 1a 02 08 01")
    );
}

#[test]
fn list_custom_subsystems_request() {
    assert_eq!(
        rpc::encode_request(2, rpc::list_custom_subsystems()),
        hex("08 02 a2 06 02 0a 00")
    );
}

#[test]
fn get_profiles_request_on_subsystem_0() {
    // Subsystem index 0 is the proto3 default, so it is omitted on the wire.
    let req = rpc::custom_call(0, rpc::ble::get_profiles());
    assert_eq!(
        rpc::encode_request(3, req),
        hex("08 03 a2 06 06 12 04 12 02 0a 00")
    );
}

#[test]
fn framed_get_device_info() {
    let wire = framing::encode(&rpc::encode_request(1, rpc::get_device_info()));
    assert_eq!(wire, hex("ab 08 01 1a 02 08 01 ad"));
}

#[test]
fn device_info_get_device_info_payload() {
    // zmk.device_info.Request { get_device_info (1): {} }
    assert_eq!(rpc::device_info::get_device_info(), hex("0a 00"));
}

#[test]
fn device_info_call_on_subsystem_2() {
    // On the #46 firmware the subsystems sort as cormoran_ble = 0,
    // cormoran_rsr = 1, zmk__device_info = 2 (but resolve by identifier).
    let req = rpc::custom_call(2, rpc::device_info::get_device_info());
    assert_eq!(
        rpc::encode_request(4, req),
        hex("08 04 a2 06 08 12 06 08 02 12 02 0a 00")
    );
}

// cormoran_ble write payloads (zmk.ble_management.Request), worked out by
// hand from the vendored proto's field numbers.

#[test]
fn switch_profile_payload() {
    // switch_profile (3): { index (1): 2 }
    assert_eq!(rpc::ble::switch_profile(2), hex("1a 02 08 02"));
    // Index 0 is the proto3 default, so the inner message is empty.
    assert_eq!(rpc::ble::switch_profile(0), hex("1a 00"));
}

#[test]
fn set_profile_name_payload() {
    // set_profile_name (2): { index (1): 1, name (2): "Mac" }
    assert_eq!(
        rpc::ble::set_profile_name(1, "Mac"),
        hex("12 07 08 01 12 03 4d 61 63")
    );
    // Clearing the name sends just the index.
    assert_eq!(rpc::ble::set_profile_name(1, ""), hex("12 02 08 01"));
}

#[test]
fn unpair_profile_payload() {
    // unpair_profile (4): { index (1): 1 }
    assert_eq!(rpc::ble::unpair_profile(1), hex("22 02 08 01"));
}

#[test]
fn output_priority_payloads() {
    use sofleii::proto::ble_management::OutputPriority;
    // set_output_priority (7): { priority (1): BLE = 1 }
    assert_eq!(
        rpc::ble::set_output_priority(OutputPriority::Ble),
        hex("3a 02 08 01")
    );
    // USB = 0 is the default: an empty inner message, which nanopb decodes
    // as OUTPUT_PRIORITY_USB.
    assert_eq!(
        rpc::ble::set_output_priority(OutputPriority::Usb),
        hex("3a 00")
    );
    // get_output_priority (8): {}
    assert_eq!(rpc::ble::get_output_priority(), hex("42 00"));
}

#[test]
fn switch_profile_call_on_subsystem_0() {
    let req = rpc::custom_call(0, rpc::ble::switch_profile(2));
    assert_eq!(
        rpc::encode_request(4, req),
        hex("08 04 a2 06 08 12 06 12 04 1a 02 08 02")
    );
}

// get_split_info, captured live on 2026-10-19 over USB from the left half
// running the ble-management fork's fix (rdlu/zmk-module-ble-management
// 5ea5b4f).

#[test]
fn get_split_info_request_on_subsystem_0() {
    // get_split_info (5): {}
    assert_eq!(rpc::ble::get_split_info(), hex("2a 00"));
    let req = rpc::custom_call(0, rpc::ble::get_split_info());
    assert_eq!(
        rpc::encode_request(7, req),
        hex("08 07 a2 06 06 12 04 12 02 2a 00")
    );
}

/// The `cormoran_ble` reply inside a captured `zmk.studio.RequestResponse`
/// (the `Response` frame's `request_response`, unframed), mapped to a
/// [`Split`].
fn split_reply(bytes: &str) -> Split {
    use prost::Message;
    use sofleii::proto::{ble_management::response::ResponseType, custom, studio};
    let rr = studio::RequestResponse::decode(hex(bytes).as_slice()).unwrap();
    assert_eq!(rr.request_id, 7);
    let Some(studio::request_response::Subsystem::Custom(custom::Response {
        response_type: Some(custom::response::ResponseType::Call(c)),
    })) = rr.subsystem
    else {
        panic!("not a custom call reply: {rr:?}");
    };
    assert_eq!(c.subsystem_index, 0);
    let ResponseType::GetSplitInfo(r) = rpc::ble::decode(&c.payload).unwrap() else {
        panic!("not get_split_info");
    };
    split::from_response(r)
}

#[test]
fn get_split_info_reply_right_half_on() {
    // info { is_split: 1, is_central: 1, peripheral_connected: 1 }
    let s = split_reply("08 07 a2 06 0e 12 0c 12 0a 32 08 0a 06 08 01 10 01 18 01");
    assert_eq!(
        s,
        Split {
            known: true,
            connected: Some(true),
            is_split: Some(true),
            is_central: Some(true),
        }
    );
}

#[test]
fn get_split_info_reply_right_half_off() {
    // peripheral_connected false is the proto3 default, so it is omitted.
    let s = split_reply("08 07 a2 06 0c 12 0a 12 08 32 06 0a 04 08 01 10 01");
    assert!(s.known && s.disconnected());
    assert_eq!(s.connected, Some(false));
}

#[test]
fn get_split_info_reply_without_info_is_unknown() {
    // Older firmware (cormoran's module before the fix): get_split_info (6)
    // is an empty message, no `info` at all.
    let s = split_reply("08 07 a2 06 06 12 04 12 02 32 00");
    assert_eq!(s, Split::unknown());
    assert!(!s.disconnected());
}
