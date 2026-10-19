//! Generates the Studio RPC, ble_management and device_info message types
//! with prost-build, and exports the vendored proto pins (from
//! `proto/README.md`'s table) for `sofleii --version`.
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
    println!("cargo:rerun-if-changed=proto/README.md");
    proto_pins()?;
    // studio.proto imports its siblings by bare name ("core.proto"), so the
    // studio directory itself is an include root.
    prost_build::Config::new().compile_protos(
        &protos,
        &["proto/studio", "proto/ble_management", "proto/device_info"],
    )
}

/// `SOFLEII_PROTO_{STUDIO,BLE_MANAGEMENT,DEVICE_INFO}`: the Commit column
/// of each row in `proto/README.md` (the one place a bump records them).
/// A missing row or a malformed SHA fails the build, so the table and the
/// version string can't drift apart.
fn proto_pins() -> std::io::Result<()> {
    let readme = std::fs::read_to_string("proto/README.md")?;
    for (dir, var) in [
        ("studio/", "SOFLEII_PROTO_STUDIO"),
        ("ble_management/", "SOFLEII_PROTO_BLE_MANAGEMENT"),
        ("device_info/", "SOFLEII_PROTO_DEVICE_INFO"),
    ] {
        let row = readme
            .lines()
            .find(|l| l.starts_with(&format!("| `{dir}`")))
            .unwrap_or_else(|| panic!("proto/README.md: no table row for `{dir}`"));
        // | Directory | Upstream | Commit | License |
        let commit = row
            .split('|')
            .nth(3)
            .map(|c| c.trim().trim_matches('`'))
            .unwrap_or("");
        assert!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "proto/README.md: the Commit column for `{dir}` is not a 40-hex SHA: {commit:?}"
        );
        println!("cargo:rustc-env={var}={commit}");
    }
    Ok(())
}
