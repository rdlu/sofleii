//! What this binary was built from and for: its own version and the
//! firmware line it speaks to, i.e. the vendored proto pins
//! (`proto/README.md`, read by `build.rs`). `sofleii --version` prints
//! [`LONG`]; `-V` just the version.

/// `zmk-studio-messages` (cormoran's fork, imported by ZMK `main+dya`).
pub const PROTO_STUDIO: &str = env!("SOFLEII_PROTO_STUDIO");
/// `zmk-module-ble-management`'s proto.
pub const PROTO_BLE_MANAGEMENT: &str = env!("SOFLEII_PROTO_BLE_MANAGEMENT");
/// `zmk-feature-device-info`'s proto.
pub const PROTO_DEVICE_INFO: &str = env!("SOFLEII_PROTO_DEVICE_INFO");

/// `sofleii --version` after the binary name.
pub const LONG: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\nfirmware line: ZMK main+dya (cormoran's Studio RPC fork), vendored protos:",
    "\n  zmk-studio-messages        ",
    env!("SOFLEII_PROTO_STUDIO"),
    "\n  zmk-module-ble-management  ",
    env!("SOFLEII_PROTO_BLE_MANAGEMENT"),
    "\n  zmk-feature-device-info    ",
    env!("SOFLEII_PROTO_DEVICE_INFO"),
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_come_from_the_proto_readme() {
        let readme = include_str!("../proto/README.md");
        for pin in [PROTO_STUDIO, PROTO_BLE_MANAGEMENT, PROTO_DEVICE_INFO] {
            assert_eq!(pin.len(), 40, "{pin}");
            assert!(readme.contains(&format!("`{pin}`")), "{pin}");
        }
        assert!(LONG.starts_with(concat!(env!("CARGO_PKG_VERSION"), "\n")));
        assert!(LONG.contains(PROTO_DEVICE_INFO));
    }
}
