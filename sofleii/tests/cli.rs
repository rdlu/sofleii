//! End-to-end CLI behaviour that needs no keyboard: error paths and exit codes.

use std::process::Command;

fn sofleii(args: &[&str]) -> (u8, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sofleii"))
        .args(args)
        .output()
        .expect("run sofleii");
    let code = out.status.code().expect("exit code") as u8;
    (code, String::from_utf8(out.stdout).expect("utf-8 stdout"))
}

#[test]
fn waybar_not_found_prints_valid_json_and_exits_2() {
    let (code, stdout) = sofleii(&[
        "--device",
        "/dev/nonexistent",
        "status",
        "--format",
        "waybar",
    ]);
    assert_eq!(code, 2);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["class"], "disconnected");
    assert_eq!(v["alt"], "disconnected");
    assert!(v["text"].is_string() && v["tooltip"].is_string());
}

#[test]
fn json_not_found_is_error_object() {
    let (code, stdout) = sofleii(&["status", "--json", "--device", "/dev/nonexistent"]);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["error"]["kind"], "not_found");
}

#[test]
fn waybar_is_status_and_battery_only() {
    let (code, stdout) = sofleii(&["profiles", "--format", "waybar"]);
    assert_eq!(code, 3);
    assert!(stdout.is_empty());
}

/// Runs with the system bus pointed at nothing: "BlueZ unavailable".
fn sofleii_no_dbus(args: &[&str]) -> (u8, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sofleii"))
        .args(args)
        .env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            "unix:path=/nonexistent/sofleii-test",
        )
        .output()
        .expect("run sofleii");
    let code = out.status.code().expect("exit code") as u8;
    (code, String::from_utf8(out.stdout).expect("utf-8 stdout"))
}

#[test]
fn battery_waybar_without_bluez_prints_valid_json_and_exits_2() {
    let (code, stdout) = sofleii_no_dbus(&["battery", "--format", "waybar"]);
    assert_eq!(code, 2);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["class"], "disconnected");
    assert!(v["tooltip"].as_str().unwrap().contains("D-Bus"));
}

#[test]
fn battery_json_without_bluez_is_not_found() {
    let (code, stdout) = sofleii_no_dbus(&["battery", "--json", "--warn-below", "50"]);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["error"]["kind"], "not_found");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("BlueZ unavailable"), "{msg}");
}

#[test]
fn battery_bad_flags_are_usage_errors() {
    assert_eq!(sofleii(&["battery", "--bt-address", "zz"]).0, 3);
    assert_eq!(sofleii(&["battery", "--warn-below", "101"]).0, 3);
    assert_eq!(sofleii(&["battery", "--json", "--format", "waybar"]).0, 3);
}

#[test]
fn contradictory_transport_flags_are_usage_errors() {
    let mac = "DD:1F:23:76:6E:71";
    for args in [
        &["info", "--device", "/dev/nonexistent", "--bt-address", mac][..],
        &["info", "--transport", "usb", "--bt-address", mac][..],
        &[
            "status",
            "--json",
            "--transport",
            "ble",
            "--device",
            "/dev/nonexistent",
        ][..],
    ] {
        let (code, stdout) = sofleii(args);
        assert_eq!(code, 3, "{args:?}");
        if args.contains(&"--json") {
            let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            assert_eq!(v["error"]["kind"], "usage");
        }
    }
}

#[test]
fn ble_without_bluez_is_not_found() {
    let (code, stdout) = sofleii_no_dbus(&["info", "--transport", "ble", "--json"]);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["error"]["kind"], "not_found");
}

#[test]
fn update_check_not_found_exits_2_before_github() {
    let (code, stdout) = sofleii(&["--device", "/dev/nonexistent", "update", "check", "--json"]);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["error"]["kind"], "not_found");
}

#[test]
fn update_check_rejects_a_malformed_repo() {
    for repo in ["sofleii", "rdlu/", "a/b/c", "rdlu/so fleii"] {
        let (code, stdout) = sofleii(&["update", "check", "--repo", repo]);
        assert_eq!(code, 3, "{repo}");
        assert!(stdout.is_empty());
    }
}
