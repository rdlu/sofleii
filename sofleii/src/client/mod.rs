//! Studio RPC client: assigns request ids, frames requests, and matches each
//! response to its request. Notifications that arrive meanwhile are kept in
//! [`Client::notifications`] instead of being mistaken for replies.
//!
//! Request ids start at a per-process value ([`seed_id`]), not 1: two
//! clients on one BLE link (the TUI and waybar's `sofleii status`) see each
//! other's replies, and the ids are what keeps them apart.

pub mod rpc;

use std::time::{Duration, Instant, SystemTime};

use crate::framing::{self, Decoder};
use crate::proto::{core, custom, meta, studio};
use crate::transport::{Transport, TransportInfo};
use crate::{Error, Result};

pub use rpc::{RequestSubsystem, ResponseSubsystem};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Request ids stay in `1..=MAX_REQUEST_ID`: `request_id` is a proto
/// `uint32` whose 0 is the default (indistinguishable from unset), and 31
/// bits stay clear of the BLE probe's `u32::MAX` and of any signed handling
/// on the firmware side.
pub const MAX_REQUEST_ID: u32 = i32::MAX as u32;

pub struct Client<T: Transport> {
    transport: T,
    decoder: Decoder,
    next_id: u32,
    timeout: Duration,
    /// The transport failed in a way it doesn't recover from; see
    /// [`Client::is_dead`].
    dead: bool,
    notifications: Vec<studio::Notification>,
    /// Decoded frames not consumed yet (several can arrive in one read).
    pending: std::collections::VecDeque<Vec<u8>>,
}

impl<T: Transport> Client<T> {
    /// A client whose first request id comes from [`seed_id`].
    pub fn new(transport: T, timeout: Duration) -> Self {
        Self::with_first_id(transport, timeout, seed_id())
    }

    /// A client whose first request id is `first_id` (clamped into
    /// `1..=MAX_REQUEST_ID`), for tests and canned replies.
    pub fn with_first_id(transport: T, timeout: Duration, first_id: u32) -> Self {
        Self {
            transport,
            decoder: Decoder::new(),
            next_id: first_id.clamp(1, MAX_REQUEST_ID),
            timeout,
            dead: false,
            notifications: Vec::new(),
            pending: Default::default(),
        }
    }

    /// Changes the per-response timeout (e.g. a short probe first).
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn transport_info(&self) -> TransportInfo {
        self.transport.info()
    }

    /// Whether the transport is gone (the port unplugged, an I/O error, the
    /// BLE notify channel closed, BlueZ saying it's not connected): the
    /// link is **lost** and has to be reopened. A timeout leaves this
    /// false: the link is fine but **deaf** (Studio answers on the other
    /// endpoint), so it's the transport choice that needs another look.
    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// The transport, for tests that check what was sent.
    #[cfg(test)]
    pub(crate) fn transport_ref(&self) -> &T {
        &self.transport
    }

    /// Notifications received while waiting for responses, oldest first.
    pub fn notifications(&self) -> &[studio::Notification] {
        &self.notifications
    }

    pub fn take_notifications(&mut self) -> Vec<studio::Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        // Wrap to 1, skipping 0 (see MAX_REQUEST_ID).
        self.next_id = if id >= MAX_REQUEST_ID { 1 } else { id + 1 };
        id
    }

    /// Passes a transport result through, marking the client dead if the
    /// error means the link is gone.
    fn check<R>(&mut self, r: Result<R>) -> Result<R> {
        if let Err(e) = &r
            && link_lost(e)
        {
            self.dead = true;
        }
        r
    }

    /// Sends one request and waits for the response carrying its id.
    /// Meta errors (`simple_error`, `no_response`) become [`Error::Rpc`].
    pub fn request(
        &mut self,
        what: &str,
        subsystem: RequestSubsystem,
    ) -> Result<ResponseSubsystem> {
        let id = self.alloc_id();
        let frame = framing::encode(&rpc::encode_request(id, subsystem));
        let sent = self.transport.send(&frame);
        self.check(sent)?;

        let deadline = Instant::now() + self.timeout;
        let mut buf = [0u8; 512];
        loop {
            while let Some(frame) = self.pending.pop_front() {
                match rpc::decode_response(&frame)? {
                    studio::response::Type::Notification(n) => self.notifications.push(n),
                    studio::response::Type::RequestResponse(r) if r.request_id == id => {
                        return match r.subsystem {
                            Some(ResponseSubsystem::Meta(m)) => Err(meta_error(what, m)),
                            Some(s) => Ok(s),
                            None => Err(Error::Protocol(format!("empty response to {what}"))),
                        };
                    }
                    // A late reply to an earlier (timed-out) request: drop it.
                    studio::response::Type::RequestResponse(r) => {
                        log::debug!("ignoring stale response id {}", r.request_id);
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout {
                    what: what.into(),
                    after: self.timeout,
                    hint: self.transport.timeout_hint(),
                });
            }
            let got = self.transport.recv(&mut buf, deadline - now);
            let n = self.check(got)?;
            self.pending.extend(self.decoder.feed(&buf[..n]));
        }
    }

    pub fn get_device_info(&mut self) -> Result<core::GetDeviceInfoResponse> {
        use core::response::ResponseType;
        match self.request("get_device_info", rpc::get_device_info())? {
            ResponseSubsystem::Core(core::Response {
                response_type: Some(ResponseType::GetDeviceInfo(info)),
            }) => Ok(info),
            other => Err(unexpected("get_device_info", &other)),
        }
    }

    pub fn get_lock_state(&mut self) -> Result<core::LockState> {
        use core::response::ResponseType;
        match self.request("get_lock_state", rpc::get_lock_state())? {
            ResponseSubsystem::Core(core::Response {
                response_type: Some(ResponseType::GetLockState(s)),
            }) => core::LockState::try_from(s)
                .map_err(|_| Error::Protocol(format!("unknown lock state {s}"))),
            other => Err(unexpected("get_lock_state", &other)),
        }
    }

    pub fn list_custom_subsystems(&mut self) -> Result<Vec<custom::CustomSubsystemInfo>> {
        use custom::response::ResponseType;
        match self.request("list_custom_subsystems", rpc::list_custom_subsystems())? {
            ResponseSubsystem::Custom(custom::Response {
                response_type: Some(ResponseType::ListCustomSubsystems(l)),
            }) => Ok(l.subsystems),
            other => Err(unexpected("list_custom_subsystems", &other)),
        }
    }

    /// Looks up a custom subsystem's index by identifier. Indices change
    /// between builds, so never hardcode them.
    pub fn resolve_subsystem(&mut self, identifier: &str) -> Result<u32> {
        self.list_custom_subsystems()?
            .into_iter()
            .find(|s| s.identifier == identifier)
            .map(|s| s.index)
            .ok_or_else(|| {
                Error::Rpc(format!(
                    "custom subsystem \"{identifier}\" is not in this firmware"
                ))
            })
    }

    /// Calls a custom subsystem and returns its response payload.
    pub fn custom_call(&mut self, what: &str, index: u32, payload: Vec<u8>) -> Result<Vec<u8>> {
        use custom::response::ResponseType;
        match self.request(what, rpc::custom_call(index, payload))? {
            ResponseSubsystem::Custom(custom::Response {
                response_type: Some(ResponseType::Call(c)),
            }) => {
                if c.subsystem_index != index {
                    return Err(Error::Protocol(format!(
                        "{what}: reply from subsystem {} instead of {index}",
                        c.subsystem_index
                    )));
                }
                Ok(c.payload)
            }
            other => Err(unexpected(what, &other)),
        }
    }
}

/// A first request id that differs between processes: the pid mixed with
/// the clock (no random-number crate), in `1..=MAX_REQUEST_ID`.
pub fn seed_id() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = u64::from(std::process::id()).rotate_left(32) ^ nanos;
    // splitmix64's finalizer, so nearby pids and times spread out.
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x % u64::from(MAX_REQUEST_ID)) as u32 + 1
}

/// Whether a transport error means the link is gone. On an open transport
/// any I/O error does (a CDC-ACM port that hits EIO, ENODEV or a hangup
/// doesn't come back, and serialport drops the errno for some of them;
/// timeouts never get here, they're `Ok(0)`), and so does "not found": that
/// is how the transports report the peer vanishing mid-session (the BLE
/// notify fd hitting EOF, BlueZ's `NotConnected`).
fn link_lost(e: &Error) -> bool {
    matches!(e, Error::Io { .. } | Error::NotFound(_))
}

fn meta_error(what: &str, m: meta::Response) -> Error {
    match m.response_type {
        Some(meta::response::ResponseType::SimpleError(code)) => {
            let name = meta::ErrorConditions::try_from(code)
                .map(|c| c.as_str_name().to_ascii_lowercase())
                .unwrap_or_else(|_| format!("error {code}"));
            Error::Rpc(format!("{what}: {name}"))
        }
        Some(meta::response::ResponseType::NoResponse(_)) => {
            Error::Rpc(format!("{what}: the keyboard sent no response"))
        }
        None => Error::Rpc(format!("{what}: empty meta response")),
    }
}

fn unexpected(what: &str, got: &ResponseSubsystem) -> Error {
    let kind = match got {
        ResponseSubsystem::Meta(_) => "meta",
        ResponseSubsystem::Core(_) => "core",
        ResponseSubsystem::Behaviors(_) => "behaviors",
        ResponseSubsystem::Keymap(_) => "keymap",
        ResponseSubsystem::Custom(_) => "custom",
    };
    Error::Protocol(format!("unexpected {kind} response to {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::{Mock, response};
    use prost::Message;
    use std::collections::VecDeque;

    fn device_info(name: &str) -> ResponseSubsystem {
        ResponseSubsystem::Core(core::Response {
            response_type: Some(core::response::ResponseType::GetDeviceInfo(
                core::GetDeviceInfoResponse {
                    name: name.into(),
                    serial_number: vec![0xAB, 0xAD],
                },
            )),
        })
    }

    #[test]
    fn matches_by_id_skipping_stale_and_notifications() {
        let notif = studio::Response {
            r#type: Some(studio::response::Type::Notification(studio::Notification {
                subsystem: Some(studio::notification::Subsystem::Core(core::Notification {
                    notification_type: Some(
                        core::notification::NotificationType::LockStateChanged(1),
                    ),
                })),
            })),
        }
        .encode_to_vec();
        let mut wire = framing::encode(&notif);
        wire.extend(framing::encode(&response(99, device_info("stale"))));
        wire.extend(framing::encode(&response(1, device_info("Eyelash Sofle"))));
        // Deliver it in awkward 3-byte chunks.
        let reads = wire.chunks(3).map(<[u8]>::to_vec).collect();
        let mut c = Client::with_first_id(Mock::new(reads), Duration::from_secs(1), 1);
        let info = c.get_device_info().unwrap();
        assert_eq!(info.name, "Eyelash Sofle");
        assert_eq!(info.serial_number, vec![0xAB, 0xAD]);
        assert_eq!(c.notifications().len(), 1);
        assert_eq!(
            c.transport.sent,
            vec![vec![0xAB, 0x08, 0x01, 0x1a, 0x02, 0x08, 0x01, 0xAD]]
        );
    }

    #[test]
    fn silence_times_out_quickly() {
        let mut c = Client::with_first_id(Mock::new(VecDeque::new()), Duration::from_millis(50), 1);
        let start = Instant::now();
        let err = c.get_device_info().unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err}");
        assert_eq!(err.exit_code(), crate::error::exit::TRANSPORT);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn meta_error_becomes_rpc_error() {
        let meta = ResponseSubsystem::Meta(meta::Response {
            response_type: Some(meta::response::ResponseType::SimpleError(
                meta::ErrorConditions::RpcNotFound as i32,
            )),
        });
        let reads = VecDeque::from([framing::encode(&response(1, meta))]);
        let mut c = Client::with_first_id(Mock::new(reads), Duration::from_secs(1), 1);
        let err = c.get_device_info().unwrap_err();
        assert!(
            matches!(&err, Error::Rpc(m) if m.contains("rpc_not_found")),
            "{err}"
        );
    }

    #[test]
    fn ids_increment() {
        let reads = VecDeque::from([
            framing::encode(&response(1, device_info("a"))),
            framing::encode(&response(2, device_info("b"))),
        ]);
        let mut c = Client::with_first_id(Mock::new(reads), Duration::from_secs(1), 1);
        assert_eq!(c.get_device_info().unwrap().name, "a");
        assert_eq!(c.get_device_info().unwrap().name, "b");
        assert_eq!(c.transport.sent[1][2], 2);
    }

    fn sent_id(c: &Client<Mock>, i: usize) -> u32 {
        let frame = Decoder::new().feed(&c.transport.sent[i]).remove(0);
        studio::Request::decode(frame.as_slice())
            .unwrap()
            .request_id
    }

    #[test]
    fn seeded_ids_are_in_range_and_matched() {
        for _ in 0..1000 {
            let id = seed_id();
            assert!((1..=MAX_REQUEST_ID).contains(&id), "{id}");
        }
        let mut c = Client::new(
            Mock::echoing(vec![device_info("a"), device_info("b")]),
            Duration::from_secs(1),
        );
        assert_eq!(c.get_device_info().unwrap().name, "a");
        assert_eq!(c.get_device_info().unwrap().name, "b");
        let first = sent_id(&c, 0);
        assert_ne!(first, 0);
        assert_eq!(
            sent_id(&c, 1),
            if first == MAX_REQUEST_ID {
                1
            } else {
                first + 1
            }
        );
    }

    #[test]
    fn pinned_ids_are_deterministic_and_wrap_to_1() {
        let reads = VecDeque::from([
            framing::encode(&response(MAX_REQUEST_ID, device_info("last"))),
            framing::encode(&response(1, device_info("wrapped"))),
        ]);
        let mut c = Client::with_first_id(Mock::new(reads), Duration::from_secs(1), u32::MAX);
        assert_eq!(c.get_device_info().unwrap().name, "last");
        assert_eq!(c.get_device_info().unwrap().name, "wrapped");
        assert_eq!((sent_id(&c, 0), sent_id(&c, 1)), (MAX_REQUEST_ID, 1));
        // 0 is the proto3 default: pinning it starts at 1.
        let mut c = Client::with_first_id(Mock::new(VecDeque::new()), Duration::ZERO, 0);
        let _ = c.get_device_info();
        assert_eq!(sent_id(&c, 0), 1);
    }

    #[test]
    fn transport_failures_mark_the_client_dead_and_timeouts_dont() {
        let mut c = Client::new(Mock::new(VecDeque::new()), Duration::from_millis(20));
        assert!(matches!(c.get_device_info(), Err(Error::Timeout { .. })));
        assert!(!c.is_dead(), "a timeout is deaf, not lost");

        let lost: [fn() -> Error; 4] = [
            || {
                Error::io(
                    "reading from /dev/ttyACM1",
                    std::io::Error::from_raw_os_error(5),
                )
            },
            || {
                Error::io(
                    "reading from /dev/ttyACM1",
                    std::io::Error::from_raw_os_error(19),
                )
            },
            || Error::io("reading", std::io::ErrorKind::NotConnected.into()),
            || Error::NotFound("BLE notify channel closed (keyboard disconnected?)".into()),
        ];
        for fail in lost {
            let mut c = Client::new(Mock::failing(fail), Duration::from_secs(1));
            let e = c.get_device_info().unwrap_err();
            assert!(c.is_dead(), "{e}");
        }

        // A reply that doesn't decode is the keyboard's fault, not the link's.
        let reads = VecDeque::from([framing::encode(&[0xff, 0xff])]);
        let mut c = Client::with_first_id(Mock::new(reads), Duration::from_secs(1), 1);
        assert!(matches!(c.get_device_info(), Err(Error::Protocol(_))));
        assert!(!c.is_dead());
    }
}
