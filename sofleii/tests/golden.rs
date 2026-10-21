//! Golden request bytes from #44, captured from the v3.0.0 firmware, plus
//! live `get_split_info` requests and replies, and the keymap reads (#76).

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

// Keymap reads (#76), captured live on 2026-10-21 over USB from the left
// half (firmware build 0858966 = v3.2.0's firmware; MYGAME, 10 layers) by
// `tests/keymap_capture.rs`, read-only, with the `.keymap` it matches frozen
// as `tests/fixtures/keymap_live.keymap`:
// `tests/fixtures/keymap_live.hex` holds the requests sent and the replies,
// unframed.

const KEYMAP_LIVE: &str = include_str!("fixtures/keymap_live.hex");

/// The captured frames labelled `label`, in order.
fn captured(label: &str) -> Vec<Vec<u8>> {
    KEYMAP_LIVE
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(n, _)| *n == label)
        .map(|(_, h)| {
            (0..h.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn keymap_requests_match_what_was_sent() {
    // keymap (5): { get_keymap (1): true }
    assert_eq!(
        rpc::encode_request(1, rpc::get_keymap()),
        hex("08 01 2a 02 08 01")
    );
    assert_eq!(captured("request:get_keymap"), [hex("08 01 2a 02 08 01")]);
    // keymap (5): { get_physical_layouts (6): true }
    assert_eq!(
        vec![rpc::encode_request(2, rpc::get_physical_layouts())],
        captured("request:get_physical_layouts")
    );
    // keymap (5): { check_unsaved_changes (3): true }
    assert_eq!(
        vec![rpc::encode_request(3, rpc::check_unsaved_changes())],
        captured("request:check_unsaved_changes")
    );
    // behaviors (4): { list_all_behaviors (1): true }
    assert_eq!(
        rpc::encode_request(4, rpc::list_all_behaviors()),
        hex("08 04 22 02 08 01")
    );
    assert_eq!(
        vec![rpc::encode_request(4, rpc::list_all_behaviors())],
        captured("request:list_all_behaviors")
    );
    // behaviors (4): { get_behavior_details (2): { behavior_id (1): 1 } }
    let details = captured("request:get_behavior_details");
    assert_eq!(details[0], hex("08 05 22 04 12 02 08 01"));
    for (i, sent) in details.iter().enumerate() {
        let id = i as u32 + 1;
        assert_eq!(
            *sent,
            rpc::encode_request(5 + i as u32, rpc::get_behavior_details(id))
        );
    }
}

/// A captured reply's subsystem payload.
fn reply(frame: &[u8]) -> sofleii::client::ResponseSubsystem {
    use sofleii::proto::studio::response::Type;
    match rpc::decode_response(frame).unwrap() {
        Type::RequestResponse(r) => r.subsystem.unwrap(),
        Type::Notification(n) => panic!("notification {n:?}"),
    }
}

#[test]
fn keymap_reply_decodes() {
    use sofleii::client::ResponseSubsystem;
    use sofleii::keymap::{self, Behavior, Labels, dts};
    use sofleii::proto::{behaviors as pb, keymap as kpb};

    let ResponseSubsystem::Keymap(kpb::Response {
        response_type: Some(kpb::response::ResponseType::GetKeymap(km)),
    }) = reply(&captured("get_keymap")[0])
    else {
        panic!("not get_keymap");
    };
    let names: Vec<&str> = km.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "BASE", "NAV", "CODE", "MEDIA", "SYS|NUM", "GAME?", "MINECRFT", "GAME", "NOMANSKY",
            "MYGAME"
        ]
    );
    assert!(km.layers.iter().all(|l| l.bindings.len() == 64));
    assert_eq!(km.max_layer_name_length, 20);

    let ResponseSubsystem::Keymap(kpb::Response {
        response_type: Some(kpb::response::ResponseType::GetPhysicalLayouts(pl)),
    }) = reply(&captured("get_physical_layouts")[0])
    else {
        panic!("not get_physical_layouts");
    };
    assert_eq!(
        pl.layouts[pl.active_layout_index as usize].name,
        "Eyelash Sofle"
    );
    let keys = &pl.layouts[0].keys;
    assert_eq!(keys.len(), 64);
    // The joystick centre (58) sits at 7.5u, 1u; the right inner thumb (59)
    // is rotated -24°.
    assert_eq!((keys[58].x, keys[58].y), (750, 100));
    assert_eq!(keys[59].r, -2400);

    // The .keymap frozen with the capture, not the live one.
    let file = dts::parse(include_str!("fixtures/keymap_live.keymap")).unwrap();
    let labels = Labels::with_file(&file);
    let behaviors: std::collections::HashMap<u32, Behavior> = captured("get_behavior_details")
        .iter()
        .map(|f| match reply(f) {
            ResponseSubsystem::Behaviors(pb::Response {
                response_type: Some(pb::response::ResponseType::GetBehaviorDetails(d)),
            }) => {
                let b = Behavior::from_details(d, &labels);
                (b.id, b)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    let label = |display: &str| {
        behaviors
            .values()
            .find(|b| b.display_name == display)
            .and_then(|b| b.label.clone())
    };
    assert_eq!(label("Key Press").as_deref(), Some("kp"));
    assert_eq!(label("home_row_mod_left").as_deref(), Some("hml"));
    assert_eq!(label("ralt_mod_tap").as_deref(), Some("ralt_mt"));
    assert_eq!(label("PIPE_OP").as_deref(), Some("M_PIPE"));
    assert_eq!(label("z_so_off").as_deref(), Some("soft_off"));
    assert_eq!(label("mouse_move").as_deref(), Some("mmv"));
    assert_eq!(label("rsr_vol").as_deref(), Some("rsr_vol"));
    // Every behaviour on the firmware gets a label.
    assert!(behaviors.values().all(|b| b.label.is_some()));

    let ctx = keymap::live_context(&km.layers, file.move_val, file.scrl_val);
    let layers = keymap::decode_layers(km.layers, &behaviors, &ctx);
    let zmk = |l: usize, p: usize| layers[l].bindings[p].zmk.clone();
    assert_eq!(zmk(0, 28), "&hml LALT S");
    assert_eq!(zmk(0, 34), "&hmr_idx RSHFT J");
    assert_eq!(zmk(0, 62), "&ralt_mt RALT F12");
    assert_eq!(zmk(0, 60), "&mo 2");
    assert_eq!(zmk(1, 14), "&kp LG(I)");
    assert_eq!(zmk(3, 6), "&kp C_VOL_UP");
    assert_eq!(zmk(4, 19), "&mmv MOVE_DOWN");
    assert_eq!(zmk(4, 32), "&mmv MOVE_LEFT");
    assert_eq!(zmk(4, 26), "&none");
    assert_eq!(zmk(5, 19), "&GAME_GEN_TO");
    assert_eq!(zmk(5, 58), "&to 0");

    // And the file agrees with the keyboard it was flashed to.
    let d = keymap::diff::diff(&layers, &behaviors, &file);
    assert!(!d.drift, "{d:#?}");
}
