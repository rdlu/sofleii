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
