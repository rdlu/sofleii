//! Captures the keymap reads from the real keyboard into
//! `tests/fixtures/keymap_live.hex`, which `tests/golden.rs` decodes.
//! Read-only: `get_keymap`, `get_physical_layouts`, `check_unsaved_changes`,
//! `list_all_behaviors` and `get_behavior_details` for each behaviour;
//! nothing is written.
//!
//! ```bash
//! SOFLEII_CAPTURE=1 cargo test --test keymap_capture -- --ignored --nocapture
//! ```
//!
//! Without `SOFLEII_CAPTURE=1` it only prints what it would write. Each line
//! is `<label> <hex>`: `request:<name>` lines hold the unframed
//! `zmk.studio.Request` sent (request ids from 1), `<name>` lines one
//! unframed `zmk.studio.Response` exactly as it arrived (the frame bytes,
//! not a re-encoding).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sofleii::client::Client;
use sofleii::framing::Decoder;
use sofleii::transport::serial::SerialTransport;
use sofleii::transport::{Transport, TransportInfo};

/// Passes everything through, keeping a copy of the bytes sent and received.
struct Recorder<T> {
    inner: T,
    sent: Arc<Mutex<Vec<u8>>>,
    got: Arc<Mutex<Vec<u8>>>,
}

impl<T: Transport> Transport for Recorder<T> {
    fn send(&mut self, bytes: &[u8]) -> sofleii::Result<()> {
        self.sent.lock().unwrap().extend_from_slice(bytes);
        self.inner.send(bytes)
    }
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> sofleii::Result<usize> {
        let n = self.inner.recv(buf, timeout)?;
        self.got.lock().unwrap().extend_from_slice(&buf[..n]);
        Ok(n)
    }
    fn info(&self) -> TransportInfo {
        self.inner.info()
    }
}

#[test]
#[ignore = "needs the keyboard over USB"]
fn capture_keymap_reads() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let got = Arc::new(Mutex::new(Vec::new()));
    let t = Recorder {
        inner: SerialTransport::open(None).expect("open the keyboard's serial port"),
        sent: sent.clone(),
        got: got.clone(),
    };
    // A fixed first id, so the fixture's request ids are predictable.
    let mut c = Client::with_first_id(t, Duration::from_secs(3), 1);
    let mut out = String::new();
    let take = |label: &str, out: &mut String| {
        let hex = |f: &[u8]| -> String { f.iter().map(|b| format!("{b:02x}")).collect() };
        let req = std::mem::take(&mut *sent.lock().unwrap());
        for frame in Decoder::new().feed(&req) {
            let _ = writeln!(out, "request:{label} {}", hex(&frame));
        }
        let bytes = std::mem::take(&mut *got.lock().unwrap());
        for frame in Decoder::new().feed(&bytes) {
            let _ = writeln!(out, "{label} {}", hex(&frame));
        }
    };
    c.get_keymap().unwrap();
    take("get_keymap", &mut out);
    c.get_physical_layouts().unwrap();
    take("get_physical_layouts", &mut out);
    c.check_unsaved_changes().unwrap();
    take("check_unsaved_changes", &mut out);
    let ids = c.list_all_behaviors().unwrap();
    take("list_all_behaviors", &mut out);
    for id in ids {
        c.get_behavior_details(id).unwrap();
        take("get_behavior_details", &mut out);
    }
    if std::env::var_os("SOFLEII_CAPTURE").is_some() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/keymap_live.hex"
        );
        std::fs::write(path, &out).unwrap();
        println!("wrote {path}");
    } else {
        print!("{out}");
    }
}
