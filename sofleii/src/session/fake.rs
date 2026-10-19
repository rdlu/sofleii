//! Test doubles for the session: a fake keyboard behind a [`Transport`]
//! that answers whatever it is asked (unlike `transport::mock`, which
//! replays a fixed script), an [`Opener`] for the real
//! [`connect::Connector`], and a [`Probe`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use prost::Message;

use super::Probe;
use crate::client::ResponseSubsystem;
use crate::commands::battery::Reading;
use crate::commands::info::tests::{device_info_reply, sample_response, subsystem};
use crate::connect::{self, Opener};
use crate::framing::{self, Decoder};
use crate::proto::ble_management::{self as bm, request::RequestType as Ble};
use crate::proto::{core, custom, device_info as di, studio};
use crate::transport::mock::{ble_reply, response};
use crate::transport::{Transport, TransportInfo, TransportKind};
use crate::{Error, Result};

/// How one opened transport behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Answers this many requests (the connect probe, say), then goes quiet.
    DeafAfter(usize),
    /// Every send and receive fails.
    Dead,
}

/// What one USB open does before the default (opening, if `kb.usb`).
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// Fails with an I/O error.
    Io,
}

/// The keyboard's state and what happened to it.
#[derive(Debug)]
pub struct Kb {
    pub active: u32,
    pub priority: bm::OutputPriority,
    /// The right half is on the split link; `None` answers
    /// `get_split_info` with no `info`, as older firmware does.
    pub split: Option<bool>,
    /// Per opened transport, in order; then [`Mode::Normal`].
    pub modes: VecDeque<Mode>,
    /// Per connection attempt, in order; then a successful open.
    pub steps: VecDeque<Step>,
    /// The serial port exists (presence, and whether USB opens).
    pub usb: bool,
    /// This host has the keyboard connected over BLE (whether BLE opens).
    pub ble: bool,
    /// USB opens tried.
    pub attempts: usize,
    pub opened: usize,
    pub dropped: usize,
    /// Fresh plans: the connector only probes presence without a cached
    /// plan, so this counts invalidations (and failed connects).
    pub plans: usize,
    /// Every RPC request answered or swallowed.
    pub requests: usize,
    /// Battery reads through [`FakeProbe`].
    pub battery_reads: usize,
}

impl Default for Kb {
    fn default() -> Self {
        Self {
            active: 0,
            priority: bm::OutputPriority::Usb,
            split: Some(true),
            modes: VecDeque::new(),
            steps: VecDeque::new(),
            usb: true,
            ble: false,
            attempts: 0,
            opened: 0,
            dropped: 0,
            plans: 0,
            requests: 0,
            battery_reads: 0,
        }
    }
}

impl Kb {
    /// Where Studio answers, as the firmware picks it: the priority's
    /// transport when it's ready, else the other one.
    pub fn selected(&self) -> TransportKind {
        match self.priority {
            bm::OutputPriority::Ble if self.ble => TransportKind::Ble,
            _ if self.usb => TransportKind::Usb,
            _ => TransportKind::Ble,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Shared(Arc<Mutex<Kb>>);

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, Kb> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct FakeTransport {
    kb: Shared,
    mode: Mode,
    answered: usize,
    kind: TransportKind,
    decoder: Decoder,
    out: VecDeque<Vec<u8>>,
}

fn dead() -> Error {
    Error::io(
        "fake transport",
        std::io::Error::from(std::io::ErrorKind::BrokenPipe),
    )
}

impl Transport for FakeTransport {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        if self.mode == Mode::Dead {
            return Err(dead());
        }
        for frame in self.decoder.feed(bytes) {
            let req = studio::Request::decode(frame.as_slice()).expect("a studio request");
            let mut kb = self.kb.lock();
            kb.requests += 1;
            if let Mode::DeafAfter(n) = self.mode
                && self.answered >= n
            {
                continue;
            }
            // Studio only listens on the selected endpoint, and a write
            // that moves it replies on the new one: silence here.
            if kb.selected() != self.kind {
                continue;
            }
            self.answered += 1;
            let r = req.subsystem.and_then(|s| answer(&mut kb, s));
            if let Some(r) = r
                && kb.selected() == self.kind
            {
                self.out
                    .push_back(framing::encode(&response(req.request_id, r)));
            }
        }
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        if self.mode == Mode::Dead {
            return Err(dead());
        }
        match self.out.pop_front() {
            Some(mut r) => {
                if r.len() > buf.len() {
                    let rest = r.split_off(buf.len());
                    self.out.push_front(rest);
                }
                buf[..r.len()].copy_from_slice(&r);
                Ok(r.len())
            }
            None => {
                std::thread::sleep(timeout.min(Duration::from_millis(1)));
                Ok(0)
            }
        }
    }

    fn info(&self) -> TransportInfo {
        TransportInfo {
            kind: self.kind,
            device: "fake".into(),
        }
    }
}

impl Drop for FakeTransport {
    fn drop(&mut self) {
        self.kb.lock().dropped += 1;
    }
}

fn answer(kb: &mut Kb, sub: crate::client::RequestSubsystem) -> Option<ResponseSubsystem> {
    use crate::client::RequestSubsystem as Req;
    use core::request::RequestType as C;
    use core::response::ResponseType as CR;
    use custom::request::RequestType as X;
    let core_reply = |r| {
        Some(ResponseSubsystem::Core(core::Response {
            response_type: Some(r),
        }))
    };
    match sub {
        Req::Core(core::Request {
            request_type: Some(C::GetDeviceInfo(_)),
        }) => core_reply(CR::GetDeviceInfo(core::GetDeviceInfoResponse {
            name: "Eyelash Sofle".into(),
            serial_number: vec![0xAB, 0xCD],
        })),
        Req::Core(core::Request {
            request_type: Some(C::GetLockState(_)),
        }) => core_reply(CR::GetLockState(
            core::LockState::ZmkStudioCoreLockStateUnlocked as i32,
        )),
        Req::Custom(custom::Request {
            request_type: Some(X::ListCustomSubsystems(_)),
        }) => Some(ResponseSubsystem::Custom(custom::Response {
            response_type: Some(custom::response::ResponseType::ListCustomSubsystems(
                custom::ListCustomSubsystemResponse {
                    subsystems: vec![
                        subsystem(0, "cormoran_ble"),
                        subsystem(1, "zmk__device_info"),
                    ],
                },
            )),
        })),
        Req::Custom(custom::Request {
            request_type: Some(X::Call(c)),
        }) if c.subsystem_index == 1 => Some(device_info_reply(
            1,
            di::response::ResponseType::DeviceInfo(sample_response()),
        )),
        Req::Custom(custom::Request {
            request_type: Some(X::Call(c)),
        }) if c.subsystem_index == 0 => {
            let req = bm::Request::decode(c.payload.as_slice())
                .ok()?
                .request_type?;
            use bm::response::ResponseType as R;
            let r = match req {
                Ble::GetProfiles(_) => R::GetProfiles(profiles(kb.active)),
                Ble::GetSplitInfo(_) => R::GetSplitInfo(bm::GetSplitInfoResponse {
                    info: kb.split.map(|c| bm::SplitInfo {
                        is_split: true,
                        is_central: true,
                        peripheral_connected: c,
                        central_bonded: false,
                    }),
                }),
                Ble::GetOutputPriority(_) => R::GetOutputPriority(bm::GetOutputPriorityResponse {
                    priority: kb.priority as i32,
                }),
                Ble::SwitchProfile(s) => {
                    kb.active = s.index;
                    R::SwitchProfile(bm::SwitchProfileResponse { success: true })
                }
                Ble::SetOutputPriority(s) => {
                    kb.priority = bm::OutputPriority::try_from(s.priority).ok()?;
                    R::SetOutputPriority(bm::SetOutputPriorityResponse { success: true })
                }
                _ => return None,
            };
            Some(ble_reply(0, r))
        }
        _ => None,
    }
}

/// Two bonded, connected slots.
fn profiles(active: u32) -> bm::GetProfilesResponse {
    let slot = |index: u32, addr: &str| bm::ProfileInfo {
        index,
        name: String::new(),
        address: addr.into(),
        is_connected: true,
        is_open: false,
        is_active: index == active,
    };
    bm::GetProfilesResponse {
        max_profiles: 2,
        profiles: vec![slot(0, "84:9E:56:52:6A:D2"), slot(1, "AA:BB:CC:DD:EE:FF")],
    }
}

/// The system side of [`connect::Connector`] over `kb`: USB is present when
/// `kb.usb` says, BLE when `kb.ble` does; opening USB follows `kb.steps`
/// and `kb.modes`.
#[derive(Clone)]
pub struct FakeOpener(pub Shared);

impl Opener for FakeOpener {
    fn usb_present(&self) -> bool {
        let mut kb = self.0.lock();
        kb.plans += 1;
        kb.usb
    }

    fn ble_connected(&self, _: Option<&str>, _: Duration) -> std::result::Result<(), String> {
        if self.0.lock().ble {
            Ok(())
        } else {
            Err("fake: no BLE".into())
        }
    }

    fn open_usb(&self, _: Option<&str>) -> Result<Box<dyn Transport>> {
        let mut kb = self.0.lock();
        kb.attempts += 1;
        match kb.steps.pop_front() {
            Some(Step::Io) => return Err(dead()),
            None if !kb.usb => return Err(Error::NotFound("fake: unplugged".into())),
            None => {}
        }
        kb.opened += 1;
        let mode = kb.modes.pop_front().unwrap_or(Mode::Normal);
        Ok(Box::new(FakeTransport {
            kb: self.0.clone(),
            mode,
            answered: 0,
            kind: TransportKind::Usb,
            decoder: Decoder::new(),
            out: VecDeque::new(),
        }))
    }

    /// Only when `kb.ble`; never deaf by [`Mode`] (those are USB's).
    fn open_ble(&self, _: Option<&str>, _: Duration) -> Result<Box<dyn Transport>> {
        if !self.0.lock().ble {
            return Err(Error::NotFound("fake: no BLE".into()));
        }
        Ok(Box::new(FakeTransport {
            kb: self.0.clone(),
            mode: Mode::Normal,
            answered: 0,
            kind: TransportKind::Ble,
            decoder: Decoder::new(),
            out: VecDeque::new(),
        }))
    }
}

/// The real connector (`auto`, plan cache, USB probe) over [`FakeOpener`],
/// with a short timeout so deaf links fail fast.
pub fn connector(kb: &Shared) -> connect::Connector<FakeOpener> {
    connect::Connector::with_opener(
        connect::Config {
            timeout: Duration::from_millis(30),
            ..connect::Config::default()
        },
        FakeOpener(kb.clone()),
    )
}

/// Presence from `kb.usb` and a flag; the batteries are never readable
/// (each try counts in `kb.battery_reads`).
#[derive(Clone)]
pub struct FakeProbe {
    pub kb: Shared,
    pub ble: Arc<AtomicBool>,
}

impl FakeProbe {
    pub fn new(kb: &Shared) -> Self {
        Self {
            kb: kb.clone(),
            ble: Arc::default(),
        }
    }
}

impl Probe for FakeProbe {
    fn usb_present(&mut self) -> bool {
        self.kb.lock().usb
    }
    fn ble_connected(&mut self) -> bool {
        self.ble.load(Ordering::SeqCst)
    }
    fn battery(&mut self) -> Result<Reading> {
        self.kb.lock().battery_reads += 1;
        Err(Error::NotFound("fake: no BLE".into()))
    }
}

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "sofleii-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
