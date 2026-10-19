//! Endpoint handover: the rule shared by the writes that can move Studio RPC
//! to the other transport (`output`, and `profile switch`/`unpair` in some
//! states).
//!
//! The firmware runs the handler, which changes the endpoint and raises
//! `zmk_endpoint_changed` synchronously; `studio_rpc_listener_cb` then calls
//! `refresh_selected_transport` before `send_response`, so the reply already
//! goes out on the *new* transport (`zmk/app/src/studio/rpc.c`,
//! `zmk/app/src/endpoints.c`). On the link sofleii sent the request on, that
//! is silence. So when a write may hand over, a reply timeout is expected,
//! and the new state is read back over the other transport if one is there.

use crate::client::Client;
use crate::transport::{Transport, TransportKind};
use crate::{Error, Result};

/// The other transport kind.
pub fn other(kind: TransportKind) -> TransportKind {
    match kind {
        TransportKind::Usb => TransportKind::Ble,
        TransportKind::Ble => TransportKind::Usb,
    }
}

/// Sends a write; `Ok(None)` is the expected silence after a handover
/// (only when `may_hand_over`), anything else is the reply payload or the
/// error.
pub fn send<T: Transport>(
    client: &mut Client<T>,
    what: &str,
    index: u32,
    payload: Vec<u8>,
    may_hand_over: bool,
) -> Result<Option<Vec<u8>>> {
    match client.custom_call(what, index, payload) {
        Ok(p) => Ok(Some(p)),
        Err(Error::Timeout { .. }) if may_hand_over => Ok(None),
        Err(e) => Err(e),
    }
}

/// A read-back over the other transport, after a silent handover.
pub struct Readback<R> {
    /// What the read returned, `None` if the transport or the read failed.
    pub value: Option<R>,
    /// The transport it was read over.
    pub over: Option<TransportKind>,
    /// Why there's no value.
    pub why_not: Option<String>,
}

/// Opens the other transport with `open` and runs `read` over it. Never
/// fails: an unavailable transport or a failed read is `why_not`.
pub fn read_other<U: Transport, R>(
    open: impl FnOnce() -> Result<Client<U>>,
    read: impl FnOnce(&mut Client<U>) -> Result<R>,
) -> Readback<R> {
    let mut c = match open() {
        Ok(c) => c,
        Err(e) => {
            return Readback {
                value: None,
                over: None,
                why_not: Some(e.to_string()),
            };
        }
    };
    let over = Some(c.transport_info().kind);
    match read(&mut c) {
        Ok(v) => Readback {
            value: Some(v),
            over,
            why_not: None,
        },
        Err(e) => Readback {
            value: None,
            over,
            why_not: Some(e.to_string()),
        },
    }
}

/// `success: false` in a write's reply, as an RPC error (exit 4).
pub fn refused(what: &str) -> Error {
    Error::Rpc(format!(
        "{what}: the keyboard reported failure (success=false)"
    ))
}

/// A read-back that contradicts the write that was just acknowledged.
pub fn mismatch(what: &str, detail: String) -> Error {
    Error::Protocol(format!("{what} was acknowledged, but {detail}"))
}
