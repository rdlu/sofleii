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
fn waybar_is_status_only() {
    let (code, stdout) = sofleii(&["profiles", "--format", "waybar"]);
    assert_eq!(code, 3);
    assert!(stdout.is_empty());
}
