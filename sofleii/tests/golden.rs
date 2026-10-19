//! Golden request bytes from #44, captured from the v3.0.0 firmware.

use sofleii::client::rpc;
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
