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
fn no_subcommand_when_piped_prints_usage_and_exits_0() {
    // Piped stdout is not a TTY, so no TUI.
    let (code, stdout) = sofleii(&[]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage: sofleii"), "{stdout}");
}

#[test]
fn output_flags_without_a_subcommand_are_usage_errors() {
    for args in [
        &["--json"][..],
        &["--format", "json"],
        &["--format", "waybar"],
    ] {
        let (code, stdout) = sofleii(args);
        assert_eq!(code, 3, "{args:?}");
        assert!(stdout.is_empty(), "{args:?}: {stdout}");
    }
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
fn update_check_firmware_flag_still_needs_the_keyboard_first() {
    // --firmware (and --refresh) still read the keyboard before GitHub.
    for extra in [
        &["--firmware"][..],
        &["--firmware", "--refresh"],
        &["--refresh"],
    ] {
        let mut args = vec!["--device", "/dev/nonexistent", "update", "check", "--json"];
        args.extend_from_slice(extra);
        let (code, stdout) = sofleii(&args);
        assert_eq!(code, 2, "{extra:?}");
        let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(v["error"]["kind"], "not_found", "{extra:?}");
    }
}

#[test]
fn update_check_firmware_and_cli_conflict() {
    let (code, stdout) = sofleii(&["update", "check", "--firmware", "--cli"]);
    assert_eq!(code, 3);
    assert!(stdout.is_empty());
}

#[test]
fn version_names_the_firmware_line() {
    let (code, stdout) = sofleii(&["--version"]);
    assert_eq!(code, 0);
    let mut lines = stdout.lines();
    assert_eq!(
        lines.next(),
        Some(concat!("sofleii ", env!("CARGO_PKG_VERSION")))
    );
    assert!(stdout.contains("firmware line: ZMK main+dya"), "{stdout}");
    for m in [
        "zmk-studio-messages",
        "zmk-module-ble-management",
        "zmk-feature-device-info",
    ] {
        assert!(stdout.contains(m), "{stdout}");
    }
    // -V stays one line.
    let (code, stdout) = sofleii(&["-V"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim_end(),
        concat!("sofleii ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn update_check_rejects_a_malformed_repo() {
    for repo in ["sofleii", "rdlu/", "a/b/c", "rdlu/so fleii"] {
        let (code, stdout) = sofleii(&["update", "check", "--repo", repo]);
        assert_eq!(code, 3, "{repo}");
        assert!(stdout.is_empty());
    }
}

#[test]
fn unpair_without_tty_or_yes_is_usage_before_connecting() {
    let out = Command::new(env!("CARGO_BIN_EXE_sofleii"))
        .args(["profile", "unpair", "1", "--json"])
        .args(["--device", "/dev/nonexistent"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run sofleii");
    assert_eq!(out.status.code(), Some(3));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "usage");
    assert!(v["error"]["message"].as_str().unwrap().contains("--yes"));
}

#[test]
fn unpair_with_yes_gets_as_far_as_connecting() {
    let (code, _) = sofleii(&[
        "profile",
        "unpair",
        "1",
        "--yes",
        "--device",
        "/dev/nonexistent",
    ]);
    assert_eq!(code, 2);
}

#[test]
fn rename_too_long_is_usage_before_connecting() {
    let long = "x".repeat(32);
    let (code, stdout) = sofleii(&[
        "profile",
        "rename",
        "1",
        &long,
        "--json",
        "--device",
        "/dev/nonexistent",
    ]);
    assert_eq!(code, 3);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v["error"]["message"].as_str().unwrap().contains("31"));
    // 31 bytes is fine: it gets as far as the (missing) keyboard.
    let ok = "x".repeat(31);
    let args = [
        "profile",
        "rename",
        "1",
        &ok,
        "--device",
        "/dev/nonexistent",
    ];
    assert_eq!(sofleii(&args).0, 2);
}

#[test]
fn bad_write_arguments_are_usage_errors() {
    assert_eq!(sofleii(&["profile", "switch", "-1"]).0, 3);
    assert_eq!(sofleii(&["profile", "switch", "x"]).0, 3);
    assert_eq!(sofleii(&["output", "wifi"]).0, 3);
    assert_eq!(sofleii(&["output", "ble", "--format", "waybar"]).0, 3);
    // forget_split_bond is deliberately not exposed.
    assert_eq!(sofleii(&["profile", "forget-split-bond"]).0, 3);
}
