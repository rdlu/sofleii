//! BLE transport: ZMK Studio RPC over GATT, through BlueZ on the system
//! D-Bus (zbus through [`crate::bluez::call`], no tokio).
//!
//! It reuses the connection the keyboard already has to this host (bonded,
//! connected as HID) and never pairs, connects or disconnects. The host
//! writes framed requests to the RPC characteristic and the firmware answers
//! with indications carrying framed bytes (`zmk/app/src/studio/
//! gatt_rpc_transport.c`, at most 27 bytes each), so a frame can span
//! several indications and one can end a frame and start the next. That is
//! the same byte stream as over USB: the streaming decoder and the client's
//! request-id matching work unchanged on top of [`BleTransport`].
//!
//! - **Receiving:** `AcquireNotify` (a `SOCK_SEQPACKET` fd, one indication
//!   per read, read with `SO_RCVTIMEO`); if BlueZ refuses it, `StartNotify`
//!   plus `PropertiesChanged` signals on `Value`. Closing the fd, or
//!   `StopNotify`, on drop unsubscribes.
//! - **Sending:** `AcquireWrite` only works on `write-without-response`
//!   characteristics, which Studio's isn't, so writes are
//!   `WriteValue(bytes, {"type": "request"})`, split to the ATT MTU.
//! - **Firmware limit:** Studio only listens on the transport of the
//!   *selected output endpoint* (`refresh_selected_transport` in
//!   `studio/rpc.c`). While USB is connected and output priority is USB,
//!   BLE writes are accepted and silently dropped, so requests time out;
//!   [`BLE_TIMEOUT_HINT`] says so. Responses also go only to the active BLE
//!   profile's connection.
//!
//! BlueZ can list the Studio service twice: a stale copy from an older
//! firmware's GATT layout survives in its cache next to the live one. See
//! [`candidates`] for how the live one is picked.

use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::os::unix::net::UnixDatagram;
use std::pin::Pin;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use prost::Message;
use zbus::export::futures_core::Stream;
use zbus::{Connection, MessageStream, OwnedFd, OwnedVariant, Variant};

use super::{Transport, TransportInfo, TransportKind};
use crate::bluez::{Bluez, call, call_error};
use crate::framing::{self, Decoder};
use crate::gatt::{DeviceObj, Query, Tree, find_device};
use crate::proto::{core, studio};
use crate::{Error, Result};

/// ZMK Studio GATT service.
pub const STUDIO_SERVICE_UUID: &str = "00000000-0196-6107-c967-c5cfb1c2482a";
/// Studio RPC characteristic (read, write, indicate).
pub const STUDIO_RPC_CHAR_UUID: &str = "00000001-0196-6107-c967-c5cfb1c2482a";

const GATT_CHAR: &str = "org.bluez.GattCharacteristic1";
/// ATT default MTU, when BlueZ doesn't say.
const DEFAULT_MTU: u16 = 23;
/// How long each candidate service gets to answer a probe, when there is
/// more than one to choose from.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(1000);
/// Request id for probes; the client never reaches it.
const PROBE_ID: u32 = u32::MAX;

/// Why a BLE request most likely timed out.
pub const BLE_TIMEOUT_HINT: &str = "over BLE, ZMK Studio only answers on the selected output: \
     while USB is connected with output priority usb, or when this host isn't the active BLE \
     profile, BLE requests are ignored; try --transport usb";

/// A Studio RPC characteristic under the keyboard, as [`candidates`] ranks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcChar {
    /// Object path of the characteristic.
    pub path: String,
    pub service_handle: u16,
    pub handle: u16,
    pub flags: Vec<String>,
    /// The service's handle range holds another service's characteristic:
    /// a leftover in BlueZ's GATT cache, not a live service.
    pub stale: bool,
}

impl RpcChar {
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
    fn usable(&self) -> bool {
        (self.has("write") || self.has("write-without-response"))
            && (self.has("indicate") || self.has("notify"))
    }
}

/// The device's usable Studio RPC characteristics, best first.
///
/// Usable means writable plus indicate or notify. A service is **stale**
/// when a characteristic of *another* service sits inside its handle range
/// (from its handle up to the next service's): a live GATT database can't
/// interleave services, but BlueZ's cache can keep an old service whose
/// handles a newer firmware reused. On the owner's keyboard the copy at
/// handle 0x2f is stale (HID's control point, 0x33, is inside it; the
/// firmware gained a third HID report since that layout was cached) and the
/// one at 0x35 is live. Order: not stale first, then by handle.
pub fn candidates(tree: &Tree, device: &DeviceObj) -> Vec<RpcChar> {
    let services: Vec<_> = tree
        .services
        .iter()
        .filter(|s| s.device == device.path)
        .collect();
    let mut out: Vec<RpcChar> = Vec::new();
    for s in services
        .iter()
        .filter(|s| s.uuid.eq_ignore_ascii_case(STUDIO_SERVICE_UUID))
    {
        let end = services
            .iter()
            .map(|o| o.handle)
            .filter(|&h| h > s.handle)
            .min()
            .unwrap_or(u16::MAX);
        let others = |c: &&crate::gatt::CharacteristicObj| {
            c.service != s.path && services.iter().any(|o| o.path == c.service)
        };
        let stale = tree
            .characteristics
            .iter()
            .filter(others)
            .any(|c| c.handle > s.handle && c.handle < end);
        out.extend(
            tree.characteristics
                .iter()
                .filter(|c| {
                    c.service == s.path && c.uuid.eq_ignore_ascii_case(STUDIO_RPC_CHAR_UUID)
                })
                .map(|c| RpcChar {
                    path: c.path.clone(),
                    service_handle: s.handle,
                    handle: c.handle,
                    flags: c.flags.clone(),
                    stale,
                })
                .filter(RpcChar::usable),
        );
    }
    out.sort_by_key(|c| (c.stale, c.handle));
    out
}

/// One open subscription to an RPC characteristic.
pub trait Link {
    /// Writes one chunk of at most [`Link::max_write`] bytes.
    fn write(&mut self, chunk: &[u8]) -> Result<()>;
    /// The next notification or indication payload; `None` if none arrived
    /// within `timeout`.
    fn next(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>>;
    /// Largest write (ATT MTU - 3).
    fn max_write(&self) -> usize;
}

/// Opens one of `cands` (from [`candidates`]).
///
/// With exactly one non-stale candidate, that one, unprobed. Otherwise each
/// in order is opened and probed with a read-only `get_device_info` for up
/// to `probe_timeout`, and the first to answer wins.
pub fn open_best<L: Link>(
    cands: &[RpcChar],
    probe_timeout: Duration,
    mut open: impl FnMut(&RpcChar) -> Result<L>,
) -> Result<(L, RpcChar)> {
    let live: Vec<&RpcChar> = cands.iter().filter(|c| !c.stale).collect();
    if let [only] = live.as_slice() {
        return Ok((open(only)?, (*only).clone()));
    }
    let mut last_err = None;
    for c in cands {
        match open(c) {
            Ok(mut link) => match probe(&mut link, probe_timeout) {
                Ok(true) => return Ok((link, c.clone())),
                Ok(false) => {}
                Err(e) => last_err = Some(e),
            },
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Timeout {
        what: format!(
            "a probe on any of {} Studio GATT services ({})",
            cands.len(),
            cands
                .iter()
                .map(|c| format!("0x{:04x}", c.service_handle))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        after: probe_timeout,
        hint: Some(BLE_TIMEOUT_HINT.into()),
    }))
}

/// Sends a `get_device_info` (read-only) and waits for its reply.
fn probe<L: Link>(link: &mut L, timeout: Duration) -> Result<bool> {
    let req = studio::Request {
        request_id: PROBE_ID,
        subsystem: Some(studio::request::Subsystem::Core(core::Request {
            request_type: Some(core::request::RequestType::GetDeviceInfo(true)),
        })),
    };
    let frame = framing::encode(&req.encode_to_vec());
    for chunk in frame.chunks(link.max_write().max(1)) {
        link.write(chunk)?;
    }
    let deadline = Instant::now() + timeout;
    let mut dec = Decoder::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        let Some(bytes) = link.next(deadline - now)? else {
            continue;
        };
        for f in dec.feed(&bytes) {
            if let Ok(studio::Response {
                r#type: Some(studio::response::Type::RequestResponse(r)),
            }) = studio::Response::decode(f.as_slice())
                && r.request_id == PROBE_ID
            {
                return Ok(true);
            }
        }
    }
}

/// [`Transport`] over a [`Link`]: a byte stream made of indications.
pub struct BleTransport<L: Link> {
    link: L,
    address: String,
    /// The rest of an indication larger than the caller's buffer.
    pending: VecDeque<u8>,
}

impl<L: Link> BleTransport<L> {
    pub fn new(link: L, address: impl Into<String>) -> Self {
        Self {
            link,
            address: address.into(),
            pending: VecDeque::new(),
        }
    }
}

impl BleTransport<DbusLink> {
    /// Finds the connected keyboard (`address`, or the BlueZ device named
    /// "Eyelash Sofle"), picks its live Studio RPC characteristic and
    /// subscribes to it. Every D-Bus call times out after `timeout`.
    pub fn connect(address: Option<&str>, timeout: Duration) -> Result<Self> {
        let bluez = Bluez::connect(timeout)?;
        let tree = bluez.managed_tree()?;
        let mut warnings = Vec::new();
        let device = find_device(
            &tree,
            &Query {
                address: address.map(str::to_string),
            },
            &mut warnings,
        )?
        .clone();
        for w in warnings {
            eprintln!("sofleii: warning: {w}");
        }
        let cands = candidates(&tree, &device);
        if cands.is_empty() {
            let why = if device.services_resolved {
                "the firmware has no Studio over BLE (built without CONFIG_ZMK_STUDIO?)"
            } else {
                "BlueZ hasn't resolved its GATT services yet (ServicesResolved=false)"
            };
            return Err(Error::NotFound(format!(
                "{} has no usable ZMK Studio GATT service: {why}",
                device.address
            )));
        }
        let conn = bluez.connection().clone();
        let (link, _picked) = open_best(&cands, timeout.min(PROBE_TIMEOUT), |c| {
            DbusLink::open(&conn, c)
        })?;
        Ok(Self::new(link, device.address))
    }
}

impl<L: Link> Transport for BleTransport<L> {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        for chunk in bytes.chunks(self.link.max_write().max(1)) {
            self.link.write(chunk)?;
        }
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        if self.pending.is_empty() {
            match self.link.next(timeout)? {
                Some(v) => self.pending.extend(v),
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        for (dst, src) in buf.iter_mut().zip(self.pending.drain(..n)) {
            *dst = src;
        }
        Ok(n)
    }

    fn info(&self) -> TransportInfo {
        TransportInfo {
            kind: TransportKind::Ble,
            device: self.address.clone(),
        }
    }

    fn timeout_hint(&self) -> Option<String> {
        Some(BLE_TIMEOUT_HINT.into())
    }
}

enum Rx {
    /// From `AcquireNotify`: one indication per datagram.
    Fd(UnixDatagram),
    /// From `StartNotify`: `Value` changes, forwarded by a reader thread.
    Signals(mpsc::Receiver<Vec<u8>>),
}

enum Tx {
    /// From `AcquireWrite`.
    Fd(UnixDatagram),
    /// `WriteValue` with this `type` option.
    Value(&'static str),
}

/// The BlueZ D-Bus [`Link`].
pub struct DbusLink {
    conn: Connection,
    path: String,
    rx: Rx,
    tx: Tx,
    mtu: u16,
}

impl DbusLink {
    /// Subscribes to `c`. `SOFLEII_BLE_NOTIFY=signals` skips `AcquireNotify`
    /// (to exercise the fallback).
    pub fn open(conn: &Connection, c: &RpcChar) -> Result<Self> {
        let force_signals =
            std::env::var("SOFLEII_BLE_NOTIFY").is_ok_and(|v| v.eq_ignore_ascii_case("signals"));
        let acquired = if force_signals {
            None
        } else {
            acquire(conn, &c.path, "AcquireNotify").ok()
        };
        let (rx, mut mtu) = match acquired {
            Some((sock, mtu)) => (Rx::Fd(sock), mtu),
            None => (
                Rx::Signals(start_notify(conn, &c.path)?),
                mtu_of(conn, &c.path),
            ),
        };
        let tx = if c.has("write-without-response") && !c.has("write") {
            match acquire(conn, &c.path, "AcquireWrite") {
                Ok((sock, m)) => {
                    mtu = mtu.min(m);
                    Tx::Fd(sock)
                }
                Err(_) => Tx::Value("command"),
            }
        } else {
            Tx::Value("request")
        };
        Ok(Self {
            conn: conn.clone(),
            path: c.path.clone(),
            rx,
            tx,
            mtu,
        })
    }
}

impl Link for DbusLink {
    fn write(&mut self, chunk: &[u8]) -> Result<()> {
        match &self.tx {
            Tx::Fd(sock) => sock
                .send(chunk)
                .map(drop)
                .map_err(|e| Error::io("writing to the BLE RPC characteristic", e)),
            Tx::Value(kind) => {
                let opts: HashMap<&str, Variant> = HashMap::from([("type", Variant::from(*kind))]);
                call(
                    &self.conn,
                    &self.path,
                    GATT_CHAR,
                    "WriteValue",
                    &(chunk, opts),
                )
                .map(drop)
                .map_err(|e| call_error("WriteValue", e))
            }
        }
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let timeout = timeout.max(Duration::from_millis(1));
        match &self.rx {
            Rx::Fd(sock) => {
                sock.set_read_timeout(Some(timeout))
                    .map_err(|e| Error::io("configuring the BLE notify socket", e))?;
                let mut buf = vec![0u8; usize::from(self.mtu).max(512)];
                match sock.recv(&mut buf) {
                    Ok(0) => Err(Error::NotFound(
                        "BLE notify channel closed (keyboard disconnected?)".into(),
                    )),
                    Ok(n) => {
                        buf.truncate(n);
                        Ok(Some(buf))
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        Ok(None)
                    }
                    Err(e) => Err(Error::io("reading BLE indications", e)),
                }
            }
            Rx::Signals(rx) => match rx.recv_timeout(timeout) {
                Ok(v) => Ok(Some(v)),
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Bluez(
                    "the D-Bus signal stream for BLE indications ended".into(),
                )),
            },
        }
    }

    fn max_write(&self) -> usize {
        usize::from(self.mtu.max(DEFAULT_MTU) - 3)
    }
}

impl Drop for DbusLink {
    fn drop(&mut self) {
        // The fds unsubscribe when they close; a StartNotify session needs
        // StopNotify (BlueZ would also drop it when this process exits).
        if let Rx::Signals(_) = self.rx {
            let _ = call(&self.conn, &self.path, GATT_CHAR, "StopNotify", &());
        }
    }
}

/// `AcquireNotify` / `AcquireWrite`: a blocking socket and the MTU.
fn acquire(conn: &Connection, path: &str, method: &str) -> Result<(UnixDatagram, u16)> {
    let opts: HashMap<&str, Variant> = HashMap::new();
    let reply = call(conn, path, GATT_CHAR, method, &(opts,)).map_err(|e| call_error(method, e))?;
    let (fd, mtu): (OwnedFd, u16) = reply
        .body()
        .deserialize()
        .map_err(|e| Error::Bluez(format!("{method}: bad reply ({e})")))?;
    let sock = UnixDatagram::from(std::os::fd::OwnedFd::from(fd));
    // BlueZ creates the socketpair non-blocking; reads use SO_RCVTIMEO.
    sock.set_nonblocking(false)
        .map_err(|e| Error::io(format!("{method} socket"), e))?;
    Ok((sock, mtu))
}

/// `StartNotify`, with a thread forwarding `Value` changes. The match rule
/// is registered before subscribing so the first indication isn't missed.
fn start_notify(conn: &Connection, path: &str) -> Result<mpsc::Receiver<Vec<u8>>> {
    let bad = |e: zbus::Error| Error::Bluez(format!("PropertiesChanged match on {path}: {e}"));
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")
        .member("PropertiesChanged")
        .path(path.to_string())
        .build()
        .map_err(bad)?;
    let mut stream = zbus::block_on(MessageStream::for_match_rule(
        rule,
        conn,
        NonZeroUsize::new(256),
    ))
    .map_err(bad)?;
    let (tx, rx) = mpsc::channel();
    // The thread blocks in `block_on` per message; between calls, zbus's
    // helper thread keeps the connection's I/O going.
    std::thread::spawn(move || {
        let next = |s: &mut MessageStream| {
            zbus::block_on(std::future::poll_fn(|cx| Pin::new(&mut *s).poll_next(cx)))
        };
        while let Some(msg) = next(&mut stream) {
            let Ok(msg) = msg else { continue };
            type Changed = (String, HashMap<String, OwnedVariant>, Vec<String>);
            let Ok((iface, changed, _)) = msg.body().deserialize::<Changed>() else {
                continue;
            };
            if iface != GATT_CHAR {
                continue;
            }
            if let Some(Variant::Array(a)) = changed.get("Value").map(|v| &**v) {
                let bytes = a
                    .iter()
                    .filter_map(|v| match v {
                        Variant::U8(b) => Some(*b),
                        _ => None,
                    })
                    .collect();
                if tx.send(bytes).is_err() {
                    break;
                }
            }
        }
    });
    call(conn, path, GATT_CHAR, "StartNotify", &()).map_err(|e| call_error("StartNotify", e))?;
    Ok(rx)
}

fn mtu_of(conn: &Connection, path: &str) -> u16 {
    call(
        conn,
        path,
        "org.freedesktop.DBus.Properties",
        "Get",
        &(GATT_CHAR, "MTU"),
    )
    .ok()
    .and_then(|r| r.body().deserialize::<OwnedVariant>().ok())
    .and_then(|v| u16::try_from(v).ok())
    .unwrap_or(DEFAULT_MTU)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Client, ResponseSubsystem};
    use crate::gatt::{CharacteristicObj, ServiceObj};
    use crate::transport::mock::response;

    const DEV: &str = "/org/bluez/hci0/dev_DD_1F_23_76_6E_71";

    /// Delivers scripted indications; records writes. With `answer_probe`,
    /// any write containing the probe id queues a reply.
    #[derive(Debug, Default)]
    struct FakeLink {
        script: VecDeque<Vec<u8>>,
        written: Vec<Vec<u8>>,
        answer_probe: bool,
        max_write: usize,
    }

    impl Link for FakeLink {
        fn write(&mut self, chunk: &[u8]) -> Result<()> {
            assert!(chunk.len() <= self.max_write, "chunk over MTU");
            self.written.push(chunk.to_vec());
            if self.answer_probe && chunk.last() == Some(&framing::EOF) {
                self.script
                    .push_back(framing::encode(&response(PROBE_ID, device_info("probe"))));
            }
            Ok(())
        }
        fn next(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
            match self.script.pop_front() {
                Some(v) => Ok(Some(v)),
                None => {
                    std::thread::sleep(timeout.min(Duration::from_millis(2)));
                    Ok(None)
                }
            }
        }
        fn max_write(&self) -> usize {
            self.max_write
        }
    }

    fn link(script: Vec<Vec<u8>>) -> FakeLink {
        FakeLink {
            script: script.into(),
            max_write: 20,
            ..Default::default()
        }
    }

    fn device_info(name: &str) -> ResponseSubsystem {
        ResponseSubsystem::Core(core::Response {
            response_type: Some(core::response::ResponseType::GetDeviceInfo(
                core::GetDeviceInfoResponse {
                    name: name.into(),
                    serial_number: vec![0x94, 0x70, 0xAB, 0xAD],
                },
            )),
        })
    }

    fn notification() -> Vec<u8> {
        framing::encode(
            &studio::Response {
                r#type: Some(studio::response::Type::Notification(studio::Notification {
                    subsystem: Some(studio::notification::Subsystem::Core(core::Notification {
                        notification_type: Some(
                            core::notification::NotificationType::LockStateChanged(1),
                        ),
                    })),
                })),
            }
            .encode_to_vec(),
        )
    }

    fn client(l: FakeLink) -> Client<BleTransport<FakeLink>> {
        Client::new(
            BleTransport::new(l, "DD:1F:23:76:6E:71"),
            Duration::from_millis(500),
        )
    }

    #[test]
    fn frame_split_across_indications() {
        // ZMK sends at most 27 bytes per indication.
        let wire = framing::encode(&response(1, device_info("Eyelash Sofle")));
        let script = wire.chunks(7).map(<[u8]>::to_vec).collect();
        let mut c = client(link(script));
        assert_eq!(c.get_device_info().unwrap().name, "Eyelash Sofle");
    }

    #[test]
    fn several_frames_in_one_indication_with_interleaved_notification() {
        // One indication: the end of a notification frame, a whole reply to
        // request 1 and the start of the reply to request 2.
        let mut wire = notification();
        wire.extend(framing::encode(&response(1, device_info("a"))));
        wire.extend(notification());
        wire.extend(framing::encode(&response(2, device_info("b"))));
        let first = wire.len() / 3;
        let second = wire.len() * 3 / 4;
        let script = vec![
            wire[..first].to_vec(),
            wire[first..second].to_vec(),
            wire[second..].to_vec(),
        ];
        let mut c = client(link(script));
        assert_eq!(c.get_device_info().unwrap().name, "a");
        assert_eq!(c.get_device_info().unwrap().name, "b");
        assert_eq!(c.notifications().len(), 2);
    }

    #[test]
    fn indication_bigger_than_the_read_buffer() {
        // The client reads 512 bytes at a time; the rest waits in `pending`.
        let mut wire = Vec::new();
        for _ in 0..100 {
            wire.extend(notification());
        }
        wire.extend(framing::encode(&response(1, device_info("big"))));
        assert!(wire.len() > 512);
        let mut c = client(link(vec![wire]));
        assert_eq!(c.get_device_info().unwrap().name, "big");
        assert_eq!(c.notifications().len(), 100);
    }

    #[test]
    fn writes_are_split_to_the_mtu() {
        let mut t = BleTransport::new(link(vec![]), "x");
        let frame = framing::encode(&[7u8; 45]);
        t.send(&frame).unwrap();
        assert_eq!(t.link.written.len(), 3);
        assert_eq!(t.link.written.concat(), frame);
    }

    #[test]
    fn silence_times_out_with_the_endpoint_hint() {
        let mut c = Client::new(
            BleTransport::new(link(vec![]), "x"),
            Duration::from_millis(30),
        );
        let e = c.get_device_info().unwrap_err();
        assert_eq!(e.exit_code(), crate::error::exit::TRANSPORT);
        assert!(e.to_string().contains("selected output"), "{e}");
    }

    /// The owner's BlueZ tree (2026-10-17), trimmed: HID at 0x22 whose
    /// control point (0x33) sits inside the stale Studio copy at 0x2f, and
    /// the live Studio service at 0x35.
    fn owner_tree() -> (Tree, DeviceObj) {
        let dev = DeviceObj {
            path: DEV.into(),
            address: "DD:1F:23:76:6E:71".into(),
            name: Some("Eyelash Sofle".into()),
            alias: Some("Eyelash Sofle".into()),
            connected: true,
            services_resolved: true,
        };
        let mut t = Tree::default();
        t.adapters_powered.push(true);
        t.devices.push(dev.clone());
        let mut svc = |h: u16, uuid: &str| {
            t.services.push(ServiceObj {
                path: format!("{DEV}/service{h:04x}"),
                device: DEV.into(),
                uuid: uuid.into(),
                handle: h,
            });
        };
        svc(0x10, "0000180f-0000-1000-8000-00805f9b34fb");
        svc(0x22, "00001812-0000-1000-8000-00805f9b34fb");
        svc(0x2f, STUDIO_SERVICE_UUID);
        svc(0x35, STUDIO_SERVICE_UUID);
        let mut ch = |svc: u16, h: u16, uuid: &str, flags: &[&str]| {
            t.characteristics.push(CharacteristicObj {
                path: format!("{DEV}/service{svc:04x}/char{h:04x}"),
                service: format!("{DEV}/service{svc:04x}"),
                uuid: uuid.into(),
                handle: h,
                flags: flags.iter().map(|f| f.to_string()).collect(),
                value: vec![],
            });
        };
        let rwi = ["read", "write", "indicate"];
        ch(
            0x10,
            0x11,
            "00002a19-0000-1000-8000-00805f9b34fb",
            &["read", "notify"],
        );
        ch(
            0x22,
            0x27,
            "00002a4d-0000-1000-8000-00805f9b34fb",
            &["read", "notify"],
        );
        ch(
            0x22,
            0x2b,
            "00002a4d-0000-1000-8000-00805f9b34fb",
            &["read", "notify"],
        );
        ch(
            0x22,
            0x33,
            "00002a4c-0000-1000-8000-00805f9b34fb",
            &["write-without-response"],
        );
        ch(0x2f, 0x30, STUDIO_RPC_CHAR_UUID, &rwi);
        ch(0x35, 0x36, STUDIO_RPC_CHAR_UUID, &rwi);
        (t, dev)
    }

    #[test]
    fn stale_duplicate_service_is_ranked_last_and_not_probed() {
        let (t, dev) = owner_tree();
        let c = candidates(&t, &dev);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].service_handle, c[0].stale), (0x35, false));
        assert_eq!((c[1].service_handle, c[1].stale), (0x2f, true));
        let mut opened = Vec::new();
        let (l, picked) = open_best(&c, Duration::from_millis(20), |c| {
            opened.push(c.handle);
            Ok(link(vec![]))
        })
        .unwrap();
        assert_eq!(picked.handle, 0x36);
        assert_eq!(opened, vec![0x36]);
        assert!(l.written.is_empty(), "a single live candidate isn't probed");
    }

    #[test]
    fn unusable_flags_are_skipped() {
        let (mut t, dev) = owner_tree();
        for c in &mut t.characteristics {
            if c.handle == 0x36 {
                c.flags = vec!["read".into()];
            }
        }
        let c = candidates(&t, &dev);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].handle, 0x30);
        // Only the stale copy is left: it gets probed rather than trusted.
        let e = open_best(&c, Duration::from_millis(10), |_| Ok(link(vec![]))).unwrap_err();
        assert!(matches!(e, Error::Timeout { .. }), "{e}");
    }

    #[test]
    fn two_live_services_are_probed_in_handle_order() {
        let (mut t, dev) = owner_tree();
        // Drop HID's control point: nothing marks 0x2f stale any more.
        t.characteristics.retain(|c| c.handle != 0x33);
        let c = candidates(&t, &dev);
        assert!(c.iter().all(|c| !c.stale));
        assert_eq!(c[0].service_handle, 0x2f);
        let mut opened = Vec::new();
        let (l, picked) = open_best(&c, Duration::from_millis(20), |c| {
            opened.push(c.handle);
            let mut l = link(vec![]);
            // Only the second (0x35) answers.
            l.answer_probe = c.handle == 0x36;
            Ok(l)
        })
        .unwrap();
        assert_eq!(opened, vec![0x30, 0x36]);
        assert_eq!(picked.handle, 0x36);
        // The probe is a framed get_device_info with the probe id.
        let sent = l.written.concat();
        let mut d = Decoder::new();
        let req = studio::Request::decode(d.feed(&sent)[0].as_slice()).unwrap();
        assert_eq!(req.request_id, PROBE_ID);
    }

    #[test]
    fn other_devices_services_are_ignored() {
        let (mut t, dev) = owner_tree();
        t.services.iter_mut().for_each(|s| {
            if s.handle == 0x35 {
                s.device = "/org/bluez/hci0/dev_11_22_33_44_55_66".into();
            }
        });
        let c = candidates(&t, &dev);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].service_handle, 0x2f);
    }
}
