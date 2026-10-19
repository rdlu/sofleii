//! `sofleii output usb|ble`: sets the output priority through the
//! `cormoran_ble` custom subsystem (`set_output_priority`), then reads it
//! back.
//!
//! Setting the priority to the *other* transport hands Studio RPC over
//! whenever that transport is ready (USB: enumerated by a host; BLE: the
//! active profile connected): the firmware switches before replying, so
//! the reply goes out on the new transport and this one goes silent (see
//! [`crate::commands::handover`]). That timeout counts as success when the
//! other transport reads back the new priority, and as `ok` with
//! `verified: false` when there's no other transport to ask.

use serde::Serialize;

use crate::client::{Client, rpc::ble};
use crate::commands::handover;
use crate::commands::profiles;
use crate::proto::ble_management::{OutputPriority, response::ResponseType};
use crate::transport::{Transport, TransportKind};
use crate::{Error, Result};

/// The result of `output`, for `--json` (`action` is `output`).
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub action: &'static str,
    /// The priority that was asked for: `"usb"` or `"ble"`.
    pub output_priority: &'static str,
    pub previous_output_priority: Option<&'static str>,
    /// Always `true`: a failed write is an error object instead.
    pub ok: bool,
    /// Whether the keyboard's reply arrived on this transport.
    pub reply: bool,
    /// Whether a read-back confirmed the new priority.
    pub verified: bool,
    /// The transport the read-back went over.
    pub verified_over: Option<TransportKind>,
    /// Where Studio RPC answers now, when known.
    pub studio_transport: Option<TransportKind>,
    pub note: Option<String>,
}

fn name(t: TransportKind) -> &'static str {
    match t {
        TransportKind::Usb => "usb",
        TransportKind::Ble => "ble",
    }
}

/// Sets the output priority to `target`. `open_other` opens the other
/// transport, for the read-back after a silent handover.
pub fn set<T: Transport, U: Transport>(
    client: &mut Client<T>,
    target: TransportKind,
    open_other: impl FnOnce() -> Result<Client<U>>,
) -> Result<Outcome> {
    let kind = client.transport_info().kind;
    let index = client.resolve_subsystem(ble::IDENTIFIER)?;
    let previous = profiles::output_priority(client, index)?;
    let wanted = name(target);
    let mut o = Outcome {
        action: "output",
        output_priority: wanted,
        previous_output_priority: previous,
        ok: true,
        reply: false,
        verified: false,
        verified_over: None,
        studio_transport: None,
        note: None,
    };
    let priority = match target {
        TransportKind::Usb => OutputPriority::Usb,
        TransportKind::Ble => OutputPriority::Ble,
    };
    let may_hand_over = target != kind;
    let sent = handover::send(
        client,
        "set_output_priority",
        index,
        ble::set_output_priority(priority),
        may_hand_over,
    )?;
    let read_back = match sent {
        Some(reply) => {
            match ble::decode(&reply)? {
                ResponseType::SetOutputPriority(r) if r.success => {}
                ResponseType::SetOutputPriority(_) => {
                    return Err(handover::refused("set_output_priority"));
                }
                _ => {
                    return Err(Error::Protocol(
                        "set_output_priority: unexpected response type".into(),
                    ));
                }
            }
            o.reply = true;
            o.studio_transport = Some(kind);
            if may_hand_over {
                // The reply came back here, so the firmware kept this
                // transport: the other one isn't ready.
                let why = match target {
                    TransportKind::Ble => "the active BLE profile isn't connected",
                    TransportKind::Usb => "USB isn't connected to a host",
                };
                o.note = Some(format!(
                    "priority is {wanted}, but {why}, so keystrokes and Studio stay on {kind}"
                ));
            }
            match profiles::output_priority(client, index) {
                Ok(p) => {
                    o.verified_over = Some(kind);
                    Some(p)
                }
                Err(e) => {
                    o.note = Some(format!(
                        "not verified: reading the priority back failed: {e}"
                    ));
                    None
                }
            }
        }
        None => {
            let r = handover::read_other(open_other, |c| {
                let i = c.resolve_subsystem(ble::IDENTIFIER)?;
                profiles::output_priority(c, i)
            });
            o.verified_over = r.over;
            o.studio_transport = Some(target);
            let moved = format!(
                "no reply over {kind} (expected: the firmware hands Studio over to {wanted} \
                 before replying); further RPC needs --transport {wanted} (or auto)"
            );
            match r.value {
                Some(p) => {
                    o.note = Some(format!("{moved}; read back over {wanted}"));
                    Some(p)
                }
                None => {
                    o.note = Some(format!(
                        "{moved}; not verified, {wanted} unavailable: {}",
                        r.why_not.unwrap_or_default()
                    ));
                    None
                }
            }
        }
    };
    if let Some(p) = read_back {
        if p != Some(wanted) {
            return Err(handover::mismatch(
                "set_output_priority",
                format!("the priority reads back as {p:?}"),
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
    use crate::proto::{ble_management as bm, custom};
    use crate::transport::mock::{Mock, ble_reply};
    use std::time::Duration;

    use bm::response::ResponseType as R;

    fn list() -> ResponseSubsystem {
        ResponseSubsystem::Custom(custom::Response {
            response_type: Some(custom::response::ResponseType::ListCustomSubsystems(
                custom::ListCustomSubsystemResponse {
                    subsystems: vec![custom::CustomSubsystemInfo {
                        index: 0,
                        identifier: ble::IDENTIFIER.into(),
                        ui_url: vec![],
                    }],
                },
            )),
        })
    }

    fn priority(p: OutputPriority) -> ResponseSubsystem {
        ble_reply(
            0,
            R::GetOutputPriority(bm::GetOutputPriorityResponse { priority: p as i32 }),
        )
    }

    fn set_ok(success: bool) -> ResponseSubsystem {
        ble_reply(
            0,
            R::SetOutputPriority(bm::SetOutputPriorityResponse { success }),
        )
    }

    fn client(replies: Vec<ResponseSubsystem>, kind: TransportKind) -> Client<Mock> {
        Client::with_first_id(
            Mock::replying(replies).over(kind),
            Duration::from_millis(100),
            1,
        )
    }

    fn no_other() -> Result<Client<Mock>> {
        Err(Error::NotFound("no BLE in this test".into()))
    }

    #[test]
    fn ble_over_usb_silence_verified_over_ble() {
        let mut c = client(
            vec![list(), priority(OutputPriority::Usb)],
            TransportKind::Usb,
        );
        let other = || {
            Ok(client(
                vec![list(), priority(OutputPriority::Ble)],
                TransportKind::Ble,
            ))
        };
        let o = set(&mut c, TransportKind::Ble, other).unwrap();
        assert!(o.ok && !o.reply && o.verified);
        assert_eq!(o.previous_output_priority, Some("usb"));
        assert_eq!(o.verified_over, Some(TransportKind::Ble));
        assert_eq!(o.studio_transport, Some(TransportKind::Ble));
        assert!(o.note.unwrap().contains("--transport ble"));
    }

    #[test]
    fn ble_over_usb_silence_without_ble_is_ok_unverified() {
        let mut c = client(
            vec![list(), priority(OutputPriority::Usb)],
            TransportKind::Usb,
        );
        let o = set(&mut c, TransportKind::Ble, no_other).unwrap();
        assert!(o.ok && !o.reply && !o.verified);
        assert_eq!(o.verified_over, None);
        assert!(o.note.unwrap().contains("not verified"));
    }

    #[test]
    fn ble_over_usb_other_disagrees_is_protocol_error() {
        let mut c = client(
            vec![list(), priority(OutputPriority::Usb)],
            TransportKind::Usb,
        );
        let other = || {
            Ok(client(
                vec![list(), priority(OutputPriority::Usb)],
                TransportKind::Ble,
            ))
        };
        let err = set(&mut c, TransportKind::Ble, other).unwrap_err();
        assert_eq!(err.kind(), "protocol");
    }

    #[test]
    fn ble_over_usb_reply_means_ble_not_ready() {
        let mut c = client(
            vec![
                list(),
                priority(OutputPriority::Usb),
                set_ok(true),
                priority(OutputPriority::Ble),
            ],
            TransportKind::Usb,
        );
        let o = set(&mut c, TransportKind::Ble, no_other).unwrap();
        assert!(o.reply && o.verified);
        assert_eq!(o.studio_transport, Some(TransportKind::Usb));
        assert!(o.note.unwrap().contains("isn't connected"));
    }

    #[test]
    fn usb_over_usb_silence_is_a_real_timeout() {
        let mut c = client(
            vec![list(), priority(OutputPriority::Ble)],
            TransportKind::Usb,
        );
        let err = set(&mut c, TransportKind::Usb, no_other).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err}");
        assert_eq!(err.exit_code(), crate::error::exit::TRANSPORT);
    }

    #[test]
    fn usb_over_ble_silence_is_a_handover() {
        let mut c = client(
            vec![list(), priority(OutputPriority::Ble)],
            TransportKind::Ble,
        );
        let other = || {
            Ok(client(
                vec![list(), priority(OutputPriority::Usb)],
                TransportKind::Usb,
            ))
        };
        let o = set(&mut c, TransportKind::Usb, other).unwrap();
        assert!(!o.reply && o.verified);
        assert_eq!(o.studio_transport, Some(TransportKind::Usb));
    }

    #[test]
    fn refused_is_rpc_error() {
        let mut c = client(
            vec![list(), priority(OutputPriority::Usb), set_ok(false)],
            TransportKind::Usb,
        );
        let err = set(&mut c, TransportKind::Usb, no_other).unwrap_err();
        assert_eq!(err.kind(), "rpc");
    }
}
