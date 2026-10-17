//! Generates the Studio RPC, ble_management and device_info message types
//! with prost-build.
//! Uses the system `protoc` (pacman `protobuf`), or `$PROTOC` if set.

fn main() -> std::io::Result<()> {
    let protos = [
        "proto/studio/studio.proto",
        "proto/ble_management/ble_management.proto",
        "proto/device_info/device_info.proto",
    ];
    for p in &protos {
        println!("cargo:rerun-if-changed={p}");
    }
    println!("cargo:rerun-if-changed=proto");
    // studio.proto imports its siblings by bare name ("core.proto"), so the
    // studio directory itself is an include root.
    prost_build::Config::new().compile_protos(
        &protos,
        &["proto/studio", "proto/ble_management", "proto/device_info"],
    )
}
