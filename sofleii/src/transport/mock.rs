//! Test-only transport that replays canned reads and records what was sent.

use std::collections::VecDeque;
use std::time::Duration;

use prost::Message;

use super::{Transport, TransportInfo, TransportKind};
use crate::client::ResponseSubsystem;
use crate::framing::{self, Decoder};
use crate::proto::{ble_management, custom, studio};
use crate::{Error, Result};

pub struct Mock {
    pub reads: VecDeque<Vec<u8>>,
    pub sent: Vec<Vec<u8>>,
    /// What [`Transport::info`] reports (USB unless [`Mock::over`] says).
    pub kind: TransportKind,
    /// Answers, in order, to whatever request ids arrive ([`Mock::echoing`]).
    pub echo: VecDeque<ResponseSubsystem>,
    /// What every `recv` fails with, if set ([`Mock::failing`]).
    pub fail: Option<fn() -> Error>,
}

impl Mock {
    pub fn new(reads: VecDeque<Vec<u8>>) -> Self {
        Self {
            reads,
            sent: vec![],
            kind: TransportKind::Usb,
            echo: VecDeque::new(),
            fail: None,
        }
    }

    /// Answers each request with the next of `responses`, under the id it
    /// was sent with (for clients with a seeded first id).
    pub fn echoing(responses: Vec<ResponseSubsystem>) -> Self {
        let mut m = Self::new(VecDeque::new());
        m.echo = responses.into();
        m
    }

    /// A mock whose every `recv` fails with `fail()`.
    pub fn failing(fail: fn() -> Error) -> Self {
        let mut m = Self::new(VecDeque::new());
        m.fail = Some(fail);
        m
    }

    /// The same mock, reporting `kind` as its transport.
    pub fn over(mut self, kind: TransportKind) -> Self {
        self.kind = kind;
        self
    }

    /// One framed read per response, answering request ids 1, 2, 3… in order.
    pub fn replying(responses: Vec<ResponseSubsystem>) -> Self {
        Self::new(
            responses
                .into_iter()
                .enumerate()
                .map(|(i, r)| framing::encode(&response(i as u32 + 1, r)))
                .collect(),
        )
    }
}

impl Transport for Mock {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.sent.push(bytes.to_vec());
        for frame in Decoder::new().feed(bytes) {
            let Ok(req) = studio::Request::decode(frame.as_slice()) else {
                continue;
            };
            if let Some(r) = self.echo.pop_front() {
                self.reads
                    .push_back(framing::encode(&response(req.request_id, r)));
            }
        }
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        if let Some(fail) = self.fail {
            return Err(fail());
        }
        match self.reads.pop_front() {
            Some(mut r) => {
                // A read longer than the caller's buffer comes back in pieces,
                // like a real serial port.
                if r.len() > buf.len() {
                    let rest = r.split_off(buf.len());
                    self.reads.push_front(rest);
                }
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
            kind: self.kind,
            device: "mock".into(),
        }
    }
}

/// An unframed `zmk.studio.Response` answering `request_id`.
pub fn response(request_id: u32, sub: ResponseSubsystem) -> Vec<u8> {
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

/// A custom-subsystem call reply carrying `payload`.
pub fn custom_reply(subsystem_index: u32, payload: Vec<u8>) -> ResponseSubsystem {
    ResponseSubsystem::Custom(custom::Response {
        response_type: Some(custom::response::ResponseType::Call(custom::CallResponse {
            subsystem_index,
            payload,
        })),
    })
}

/// A custom-subsystem call reply carrying a `cormoran_ble` response.
pub fn ble_reply(
    subsystem_index: u32,
    r: ble_management::response::ResponseType,
) -> ResponseSubsystem {
    custom_reply(
        subsystem_index,
        ble_management::Response {
            response_type: Some(r),
        }
        .encode_to_vec(),
    )
}
