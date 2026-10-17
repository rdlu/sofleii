//! Studio RPC client: assigns request ids, frames requests, and matches each
//! response to its request. Notifications that arrive meanwhile are kept in
//! [`Client::notifications`] instead of being mistaken for replies.

pub mod rpc;

use std::time::{Duration, Instant};

use crate::framing::{self, Decoder};
use crate::proto::{core, custom, meta, studio};
use crate::transport::{Transport, TransportInfo};
use crate::{Error, Result};

pub use rpc::{RequestSubsystem, ResponseSubsystem};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Client<T: Transport> {
    transport: T,
    decoder: Decoder,
    next_id: u32,
    timeout: Duration,
    notifications: Vec<studio::Notification>,
    /// Decoded frames not consumed yet (several can arrive in one read).
    pending: std::collections::VecDeque<Vec<u8>>,
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T, timeout: Duration) -> Self {
        Self {
            transport,
            decoder: Decoder::new(),
            next_id: 1,
            timeout,
            notifications: Vec::new(),
            pending: Default::default(),
        }
    }

    pub fn transport_info(&self) -> TransportInfo {
        self.transport.info()
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
        // Skip 0 on wrap: it's the proto3 default and indistinguishable from unset.
        self.next_id = self.next_id.checked_add(1).unwrap_or(1);
        id
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
        self.transport.send(&frame)?;

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
                        eprintln!("sofleii: ignoring stale response id {}", r.request_id);
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout {
                    what: what.into(),
                    after: self.timeout,
                });
            }
            let n = self.transport.recv(&mut buf, deadline - now)?;
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
    use crate::transport::TransportKind;
    use prost::Message;
    use std::collections::VecDeque;

    /// Replays canned reads; records what was sent.
    struct Mock {
        reads: VecDeque<Vec<u8>>,
        sent: Vec<Vec<u8>>,
    }

    impl Transport for Mock {
        fn send(&mut self, bytes: &[u8]) -> Result<()> {
            self.sent.push(bytes.to_vec());
            Ok(())
        }
        fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
            match self.reads.pop_front() {
                Some(r) => {
                    buf[..r.len()].copy_from_slice(&r);
                    Ok(r.len())
                }
                None => {
                    std::thread::sleep(timeout.min(Duration::from_millis(5)));
                    Ok(0)
                }
            }
        }
        fn info(&self) -> TransportInfo {
            TransportInfo {
                kind: TransportKind::Usb,
                device: "mock".into(),
            }
        }
    }

    fn response(request_id: u32, sub: ResponseSubsystem) -> Vec<u8> {
        studio::Response {
            r#type: Some(studio::response::Type::RequestResponse(
                studio::RequestResponse {
                    request_id,
                    subsystem: Some(sub),
                },
            )),
        }
        .encode_to_vec()
    }

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
        let mut c = Client::new(
            Mock {
                reads,
                sent: vec![],
            },
            Duration::from_secs(1),
        );
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
        let mut c = Client::new(
            Mock {
                reads: VecDeque::new(),
                sent: vec![],
            },
            Duration::from_millis(50),
        );
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
        let mut c = Client::new(
            Mock {
                reads,
                sent: vec![],
            },
            Duration::from_secs(1),
        );
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
        let mut c = Client::new(
            Mock {
                reads,
                sent: vec![],
            },
            Duration::from_secs(1),
        );
        assert_eq!(c.get_device_info().unwrap().name, "a");
        assert_eq!(c.get_device_info().unwrap().name, "b");
        assert_eq!(c.transport.sent[1][2], 2);
    }
}
