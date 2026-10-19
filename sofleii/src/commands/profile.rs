//! `sofleii profile switch|rename|unpair <n>`: BLE profile writes through
//! the `cormoran_ble` custom subsystem. Profiles are 0-based, as `profiles`
//! lists them (and as `&bt BT_SEL n` counts). Each action reads the
//! profiles first (range check, the "before" state), and reads them back
//! after. Confirming `unpair` is the front end's job, between [`prepare`]
//! and [`unpair`].

use serde::Serialize;

use crate::client::{Client, rpc::ble};
use crate::commands::handover::{self, Readback};
use crate::commands::profiles::{self, Profile, Profiles};
use crate::proto::ble_management::response::ResponseType;
use crate::transport::{Transport, TransportKind};
use crate::{Error, Result};

/// The result of a profile write, for `--json` (`action` is
/// `profile_switch`, `profile_rename` or `profile_unpair`).
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub action: &'static str,
    pub profile: u32,
    /// `rename`: the new name, `null` when it was cleared.
    pub name: Option<String>,
    /// Always `true`: a failed write is an error object instead.
    pub ok: bool,
    /// Whether a request was sent (`unpair` on an empty slot sends none).
    pub sent: bool,
    /// Whether the keyboard's reply arrived on this transport.
    pub reply: bool,
    /// Whether a read-back confirmed the new state.
    pub verified: bool,
    /// The transport the read-back went over.
    pub verified_over: Option<TransportKind>,
    /// The active profile before the write.
    pub previous_active_profile: Option<u32>,
    /// The active profile after, from the read-back.
    pub active_profile: Option<u32>,
    /// The target slot after, from the read-back.
    pub slot: Option<Profile>,
    /// Where Studio RPC answers now, when known.
    pub studio_transport: Option<TransportKind>,
    pub note: Option<String>,
}

/// A checked target: the subsystem index, the profiles before the write
/// and the profile index (in range).
#[derive(Debug, Clone)]
pub struct Target {
    pub subsystem: u32,
    pub before: Profiles,
    pub profile: u32,
    /// The transport the request goes over.
    pub kind: TransportKind,
}

impl Target {
    /// The target slot as it was before the write.
    pub fn slot(&self) -> Option<&Profile> {
        self.before
            .profiles
            .iter()
            .find(|p| p.index == self.profile)
    }
}

/// Resolves `cormoran_ble` by identifier, reads the profiles and checks
/// that `profile` is in range (else exit 3).
pub fn prepare<T: Transport>(client: &mut Client<T>, profile: u32) -> Result<Target> {
    let subsystem = client.resolve_subsystem(ble::IDENTIFIER)?;
    let before = profiles::fetch(client, subsystem)?;
    if profile >= before.max_profiles {
        return Err(Error::Usage(format!(
            "profile {profile} is out of range: this keyboard has profiles 0-{} \
             (0-based, as `sofleii profiles` lists them)",
            before.max_profiles.saturating_sub(1)
        )));
    }
    Ok(Target {
        subsystem,
        before,
        profile,
        kind: client.transport_info().kind,
    })
}

/// Rejects a name the firmware can't take, before anything is sent: more
/// than [`ble::NAME_MAX_BYTES`] UTF-8 bytes, or control characters (a NUL
/// would cut it short in the firmware's C string).
pub fn validate_name(name: &str) -> Result<()> {
    if name.len() > ble::NAME_MAX_BYTES {
        return Err(Error::Usage(format!(
            "profile name is {} bytes; the firmware takes at most {} (UTF-8)",
            name.len(),
            ble::NAME_MAX_BYTES
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::Usage(
            "profile name must not contain control characters".into(),
        ));
    }
    Ok(())
}

fn outcome(action: &'static str, t: &Target) -> Outcome {
    Outcome {
        action,
        profile: t.profile,
        name: None,
        ok: true,
        sent: true,
        reply: false,
        verified: false,
        verified_over: None,
        previous_active_profile: t.before.active_profile,
        active_profile: None,
        slot: None,
        studio_transport: None,
        note: None,
    }
}

/// Fills the after-state from a read-back.
fn after(o: &mut Outcome, p: &Profiles) {
    o.active_profile = p.active_profile;
    o.slot = p.profiles.iter().find(|s| s.index == o.profile).cloned();
}

/// Reads the profiles back over this transport; a failure is a note, not
/// an error (the write itself was acknowledged).
fn read_back<T: Transport>(
    client: &mut Client<T>,
    t: &Target,
    o: &mut Outcome,
) -> Option<Profiles> {
    match profiles::fetch(client, t.subsystem) {
        Ok(p) => {
            after(o, &p);
            o.verified_over = Some(t.kind);
            Some(p)
        }
        Err(e) => {
            o.note = Some(format!(
                "not verified: reading the profiles back failed: {e}"
            ));
            None
        }
    }
}

/// Reads the profiles back over the other transport after a silent
/// handover, and says what happened in `note`.
fn read_back_other<U: Transport>(
    open: impl FnOnce() -> Result<Client<U>>,
    t: &Target,
    o: &mut Outcome,
    why: &str,
) -> Option<Profiles> {
    let other = handover::other(t.kind);
    let r: Readback<Profiles> = handover::read_other(open, |c| {
        let i = c.resolve_subsystem(ble::IDENTIFIER)?;
        profiles::fetch(c, i)
    });
    o.verified_over = r.over;
    match r.value {
        Some(p) => {
            after(o, &p);
            o.studio_transport = r.over;
            o.note = Some(format!(
                "no reply over {} ({why}); read back over {other}",
                t.kind
            ));
            Some(p)
        }
        None => {
            o.note = Some(format!(
                "no reply over {} ({why}); not verified, {other} unavailable: {}",
                t.kind,
                r.why_not.unwrap_or_default()
            ));
            None
        }
    }
}

/// Checks a write's `success` flag in its typed reply.
fn check_success(what: &str, payload: &[u8]) -> Result<()> {
    let ok = match ble::decode(payload)? {
        ResponseType::SwitchProfile(r) => r.success,
        ResponseType::SetProfileName(r) => r.success,
        ResponseType::UnpairProfile(r) => r.success,
        _ => {
            return Err(Error::Protocol(format!("{what}: unexpected response type")));
        }
    };
    if ok {
        Ok(())
    } else {
        Err(handover::refused(what))
    }
}

/// Makes `t.profile` the active profile.
///
/// Hands over (a reply timeout is expected) when the profile changes and
/// the reply would follow it: over BLE, replies only go to the active
/// profile's connection; over USB with priority `ble`, USB is only selected
/// because the old profile isn't connected, and a connected new one takes
/// over. `open_other` then opens the other transport to read back.
pub fn switch<T: Transport, U: Transport>(
    client: &mut Client<T>,
    t: &Target,
    open_other: impl FnOnce() -> Result<Client<U>>,
) -> Result<Outcome> {
    let mut o = outcome("profile_switch", t);
    let changes = t.before.active_profile != Some(t.profile);
    let may_hand_over =
        changes && (t.kind == TransportKind::Ble || t.before.output_priority == Some("ble"));
    let payload = ble::switch_profile(t.profile);
    let after_state = match handover::send(
        client,
        "switch_profile",
        t.subsystem,
        payload,
        may_hand_over,
    )? {
        Some(reply) => {
            check_success("switch_profile", &reply)?;
            o.reply = true;
            o.studio_transport = Some(t.kind);
            read_back(client, t, &mut o)
        }
        None => {
            let why = match t.kind {
                TransportKind::Ble => {
                    "expected: over BLE, Studio answers only on the active profile's \
                     connection, which is no longer this host's"
                }
                TransportKind::Usb => {
                    "expected: with priority ble, the new profile's BLE connection \
                     took over from USB"
                }
            };
            read_back_other(open_other, t, &mut o, why)
        }
    };
    if let Some(p) = after_state {
        if p.active_profile != Some(t.profile) {
            return Err(handover::mismatch(
                "switch_profile",
                format!("the active profile reads back as {:?}", p.active_profile),
            ));
        }
        o.verified = true;
    }
    Ok(o)
}

/// Names the bonded `t.profile` (`""` clears the name). The firmware keys
/// names by the bond's address, so an empty slot is a usage error (exit 3).
pub fn rename<T: Transport>(client: &mut Client<T>, t: &Target, name: &str) -> Result<Outcome> {
    validate_name(name)?;
    if !t.slot().is_some_and(|s| s.bonded) {
        return Err(Error::Usage(format!(
            "profile {} is empty: the firmware names a bond by its address, so pair a host \
             on it first",
            t.profile
        )));
    }
    let mut o = outcome("profile_rename", t);
    o.name = (!name.is_empty()).then(|| name.to_string());
    let reply = client.custom_call(
        "set_profile_name",
        t.subsystem,
        ble::set_profile_name(t.profile, name),
    )?;
    check_success("set_profile_name", &reply)?;
    o.reply = true;
    o.studio_transport = Some(t.kind);
    if let Some(p) = read_back(client, t, &mut o) {
        let got = p
            .profiles
            .iter()
            .find(|s| s.index == t.profile)
            .and_then(|s| s.name.clone());
        if got != o.name {
            return Err(handover::mismatch(
                "set_profile_name",
                format!("the name reads back as {got:?}"),
            ));
        }
        o.verified = true;
    }
    Ok(o)
}

/// Removes `t.profile`'s bond. An empty slot is a no-op (nothing sent).
///
/// The firmware selects the profile, clears its bond and re-selects the
/// active one, so the reply comes back normally, except when unpairing the
/// active profile over BLE: that drops this very connection, and the
/// profiles are read back over `open_other` instead.
pub fn unpair<T: Transport, U: Transport>(
    client: &mut Client<T>,
    t: &Target,
    open_other: impl FnOnce() -> Result<Client<U>>,
) -> Result<Outcome> {
    let mut o = outcome("profile_unpair", t);
    if !t.slot().is_some_and(|s| s.bonded) {
        o.sent = false;
        o.verified = true;
        o.active_profile = t.before.active_profile;
        o.slot = t.slot().cloned();
        o.note = Some(format!(
            "profile {} is not bonded; nothing to do",
            t.profile
        ));
        return Ok(o);
    }
    let may_hand_over = t.kind == TransportKind::Ble && t.before.active_profile == Some(t.profile);
    let payload = ble::unpair_profile(t.profile);
    let after_state = match handover::send(
        client,
        "unpair_profile",
        t.subsystem,
        payload,
        may_hand_over,
    )? {
        Some(reply) => {
            check_success("unpair_profile", &reply)?;
            o.reply = true;
            o.studio_transport = Some(t.kind);
            read_back(client, t, &mut o)
        }
        None => read_back_other(
            open_other,
            t,
            &mut o,
            "expected: unpairing the active profile drops the BLE connection sofleii was using",
        ),
    };
    if let Some(p) = after_state {
        if p.profiles.iter().any(|s| s.index == t.profile && s.bonded) {
            return Err(handover::mismatch(
                "unpair_profile",
                format!("profile {} still reads back as bonded", t.profile),
            ));
        }
        o.verified = true;
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ResponseSubsystem;
    use crate::error::exit;
    use crate::proto::{ble_management as bm, custom};
    use crate::transport::mock::{Mock, ble_reply};
    use std::time::Duration;

    use bm::response::ResponseType as R;

    fn list(ble: u32) -> ResponseSubsystem {
        ResponseSubsystem::Custom(custom::Response {
            response_type: Some(custom::response::ResponseType::ListCustomSubsystems(
                custom::ListCustomSubsystemResponse {
                    subsystems: vec![custom::CustomSubsystemInfo {
                        index: ble,
                        identifier: ble::IDENTIFIER.into(),
                        ui_url: vec![],
                    }],
                },
            )),
        })
    }

    /// Two slots: 0 bonded + connected + `active == 0`, 1 empty (or bonded
    /// with a name).
    fn slots(active: u32, one_bonded: bool, one_name: &str) -> ResponseSubsystem {
        let slot = |i: u32, open: bool, addr: &str, name: &str| bm::ProfileInfo {
            index: i,
            name: name.into(),
            address: addr.into(),
            is_connected: !open,
            is_open: open,
            is_active: i == active,
        };
        ble_reply(
            0,
            R::GetProfiles(bm::GetProfilesResponse {
                max_profiles: 2,
                profiles: vec![
                    slot(0, false, "84:9E:56:52:6A:D2", ""),
                    if one_bonded {
                        slot(1, false, "AA:BB:CC:DD:EE:FF", one_name)
                    } else {
                        slot(1, true, "", "")
                    },
                ],
            }),
        )
    }

    fn priority(p: bm::OutputPriority) -> ResponseSubsystem {
        ble_reply(
            0,
            R::GetOutputPriority(bm::GetOutputPriorityResponse { priority: p as i32 }),
        )
    }

    /// `prepare`'s three replies: list, get_profiles, get_output_priority.
    fn before(active: u32, one_bonded: bool, p: bm::OutputPriority) -> Vec<ResponseSubsystem> {
        vec![list(0), slots(active, one_bonded, ""), priority(p)]
    }

    fn client(replies: Vec<ResponseSubsystem>, kind: TransportKind) -> Client<Mock> {
        Client::with_first_id(
            Mock::replying(replies).over(kind),
            Duration::from_millis(100),
            1,
        )
    }

    fn no_other() -> Result<Client<Mock>> {
        Err(Error::NotFound("no other transport in this test".into()))
    }

    /// The `cormoran_ble` payload of the `i`th request sent.
    fn payload_at(c: &Client<Mock>, i: usize) -> Vec<u8> {
        use crate::proto::studio;
        use prost::Message;
        let frame = &c.transport_ref().sent[i];
        let req = studio::Request::decode(&frame[1..frame.len() - 1]).unwrap();
        match req.subsystem {
            Some(studio::request::Subsystem::Custom(custom::Request {
                request_type: Some(custom::request::RequestType::Call(call)),
            })) => call.payload,
            other => panic!("not a custom call: {other:?}"),
        }
    }

    #[test]
    fn out_of_range_is_usage() {
        let mut c = client(
            before(0, false, bm::OutputPriority::Usb),
            TransportKind::Usb,
        );
        let err = prepare(&mut c, 2).unwrap_err();
        assert_eq!(err.exit_code(), exit::USAGE, "{err}");
        assert!(err.to_string().contains("0-1"), "{err}");
        assert!(
            prepare(
                &mut client(
                    before(0, false, bm::OutputPriority::Usb),
                    TransportKind::Usb
                ),
                1
            )
            .is_ok()
        );
    }

    #[test]
    fn name_validation() {
        assert!(validate_name("").is_ok());
        assert!(validate_name(&"x".repeat(31)).is_ok());
        let err = validate_name(&"x".repeat(32)).unwrap_err();
        assert_eq!(err.exit_code(), exit::USAGE);
        // Bytes, not chars: 16 two-byte characters are 32 bytes.
        assert!(validate_name(&"é".repeat(16)).is_err());
        assert!(validate_name(&"é".repeat(15)).is_ok());
        assert!(validate_name("a\0b").is_err());
    }

    #[test]
    fn switch_over_usb_reads_back() {
        let mut replies = before(0, true, bm::OutputPriority::Usb);
        replies.push(ble_reply(
            0,
            R::SwitchProfile(bm::SwitchProfileResponse { success: true }),
        ));
        replies.push(slots(1, true, ""));
        replies.push(priority(bm::OutputPriority::Usb));
        let mut c = client(replies, TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let o = switch(&mut c, &t, no_other).unwrap();
        assert!(o.reply && o.verified);
        assert_eq!(o.previous_active_profile, Some(0));
        assert_eq!(o.active_profile, Some(1));
        assert_eq!(o.studio_transport, Some(TransportKind::Usb));
        assert_eq!(c.transport_ref().sent.len(), 6);
        assert_eq!(payload_at(&c, 3), ble::switch_profile(1));
    }

    #[test]
    fn switch_refused_is_rpc_error() {
        let mut replies = before(0, true, bm::OutputPriority::Usb);
        replies.push(ble_reply(
            0,
            R::SwitchProfile(bm::SwitchProfileResponse { success: false }),
        ));
        let mut c = client(replies, TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let err = switch(&mut c, &t, no_other).unwrap_err();
        assert_eq!(err.exit_code(), exit::TRANSPORT);
        assert_eq!(err.kind(), "rpc");
    }

    #[test]
    fn switch_error_reply_is_rpc_error() {
        let mut replies = before(0, true, bm::OutputPriority::Usb);
        replies.push(ble_reply(
            0,
            R::Error(bm::ErrorResponse {
                message: "Failed to process request: -22".into(),
            }),
        ));
        let mut c = client(replies, TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let err = switch(&mut c, &t, no_other).unwrap_err();
        assert_eq!(err.kind(), "rpc");
        assert_eq!(err.exit_code(), exit::TRANSPORT);
    }

    #[test]
    fn switch_over_ble_silence_is_a_handover_verified_over_usb() {
        // Over BLE the reply follows the new profile: silence.
        let mut c = client(before(0, true, bm::OutputPriority::Ble), TransportKind::Ble);
        let t = prepare(&mut c, 1).unwrap();
        let other = || {
            Ok(client(
                vec![
                    list(0),
                    slots(1, true, ""),
                    priority(bm::OutputPriority::Ble),
                ],
                TransportKind::Usb,
            ))
        };
        let o = switch(&mut c, &t, other).unwrap();
        assert!(o.ok && !o.reply && o.verified);
        assert_eq!(o.verified_over, Some(TransportKind::Usb));
        assert_eq!(o.studio_transport, Some(TransportKind::Usb));
        assert_eq!(o.active_profile, Some(1));
        assert!(o.note.unwrap().contains("read back over usb"));
    }

    #[test]
    fn switch_over_ble_without_other_transport_is_ok_unverified() {
        let mut c = client(before(0, true, bm::OutputPriority::Ble), TransportKind::Ble);
        let t = prepare(&mut c, 1).unwrap();
        let o = switch(&mut c, &t, no_other).unwrap();
        assert!(o.ok && !o.reply && !o.verified);
        assert_eq!(o.active_profile, None);
        assert!(o.note.unwrap().contains("not verified"));
    }

    #[test]
    fn switch_over_usb_with_priority_usb_silence_is_an_error() {
        // USB stays selected, so silence is a real timeout (exit 4).
        let mut c = client(before(0, true, bm::OutputPriority::Usb), TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let err = switch(&mut c, &t, no_other).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err}");
    }

    #[test]
    fn switch_to_the_active_profile_never_hands_over() {
        let mut c = client(before(0, true, bm::OutputPriority::Ble), TransportKind::Ble);
        let t = prepare(&mut c, 0).unwrap();
        assert!(matches!(
            switch(&mut c, &t, no_other),
            Err(Error::Timeout { .. })
        ));
    }

    #[test]
    fn switch_read_back_mismatch_is_protocol_error() {
        let mut replies = before(0, true, bm::OutputPriority::Usb);
        replies.push(ble_reply(
            0,
            R::SwitchProfile(bm::SwitchProfileResponse { success: true }),
        ));
        replies.push(slots(0, true, ""));
        replies.push(priority(bm::OutputPriority::Usb));
        let mut c = client(replies, TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let err = switch(&mut c, &t, no_other).unwrap_err();
        assert_eq!(err.kind(), "protocol");
    }

    #[test]
    fn rename_sends_name_and_verifies() {
        let mut replies = before(0, true, bm::OutputPriority::Usb);
        replies.push(ble_reply(
            0,
            R::SetProfileName(bm::SetProfileNameResponse { success: true }),
        ));
        replies.push(slots(0, true, "Mac"));
        replies.push(priority(bm::OutputPriority::Usb));
        let mut c = client(replies, TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let o = rename(&mut c, &t, "Mac").unwrap();
        assert!(o.verified);
        assert_eq!(o.name.as_deref(), Some("Mac"));
        assert_eq!(o.slot.unwrap().name.as_deref(), Some("Mac"));
        assert_eq!(c.transport_ref().sent.len(), 6);
        assert_eq!(payload_at(&c, 3), ble::set_profile_name(1, "Mac"));
    }

    #[test]
    fn rename_empty_slot_is_usage_and_sends_nothing() {
        let mut c = client(
            before(0, false, bm::OutputPriority::Usb),
            TransportKind::Usb,
        );
        let t = prepare(&mut c, 1).unwrap();
        let err = rename(&mut c, &t, "Mac").unwrap_err();
        assert_eq!(err.exit_code(), exit::USAGE);
        assert_eq!(c.transport_ref().sent.len(), 3);
    }

    #[test]
    fn rename_too_long_sends_nothing() {
        let mut c = client(before(0, true, bm::OutputPriority::Usb), TransportKind::Usb);
        let t = prepare(&mut c, 1).unwrap();
        let err = rename(&mut c, &t, &"x".repeat(32)).unwrap_err();
        assert_eq!(err.exit_code(), exit::USAGE);
        assert_eq!(c.transport_ref().sent.len(), 3);
    }

    #[test]
    fn unpair_empty_slot_is_a_no_op() {
        let mut c = client(
            before(0, false, bm::OutputPriority::Usb),
            TransportKind::Usb,
        );
        let t = prepare(&mut c, 1).unwrap();
        let o = unpair(&mut c, &t, no_other).unwrap();
        assert!(!o.sent && o.verified);
        assert_eq!(c.transport_ref().sent.len(), 3);
    }

    #[test]
    fn unpair_bonded_slot_verifies_it_is_empty() {
        let mut replies = before(0, true, bm::OutputPriority::Usb);
        replies.push(ble_reply(
            0,
            R::UnpairProfile(bm::UnpairProfileResponse { success: true }),
        ));
        replies.push(slots(0, false, ""));
        replies.push(priority(bm::OutputPriority::Usb));
        let mut c = client(replies, TransportKind::Ble);
        let t = prepare(&mut c, 1).unwrap();
        let o = unpair(&mut c, &t, no_other).unwrap();
        assert!(o.sent && o.reply && o.verified);
        assert!(!o.slot.unwrap().bonded);
        assert_eq!(payload_at(&c, 3), ble::unpair_profile(1));
    }

    #[test]
    fn unpair_active_over_ble_silence_is_a_handover() {
        let mut c = client(before(0, true, bm::OutputPriority::Ble), TransportKind::Ble);
        let t = prepare(&mut c, 0).unwrap();
        let o = unpair(&mut c, &t, no_other).unwrap();
        assert!(o.ok && !o.reply && !o.verified);
    }
}
