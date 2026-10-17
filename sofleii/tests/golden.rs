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
