//! The battery and presence thread, on its own BlueZ connection (never
//! Studio RPC, so it never competes with the link for the USB port):
//!
//! - both batteries every [`Config::battery_every`], through
//!   [`commands::battery`](crate::commands::battery) over GATT, at once
//!   when the BLE connection comes up, and at once plus
//!   [`Config::battery_after_split`] later when the split link changes
//!   ([`Msg::SplitEdge`], from the link);
//! - USB presence every [`Config::usb_every`]: the serial discover (sysfs,
//!   no radio);
//! - BLE `Device1.Connected` every [`Config::ble_every`], and with it this
//!   host's adapter addresses ([`Event::Adapters`], when they change).
//!
//! Presence changes go to the front end ([`Event::Presence`]) and to the
//! link, which retries (or replans) at once.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::{Config, ErrorView, Event, LogLevel, Outcome, Presence, Stamped, Ticket, link};
use crate::Result;
use crate::bluez::Bluez;
use crate::commands::battery::{self, Reading};
use crate::gatt::{Query, find_device};
use crate::transport::serial;

/// What the thread reads; [`SystemProbe`] is the real one.
pub trait Probe: Send {
    /// The keyboard's serial port exists.
    fn usb_present(&mut self) -> bool;
    /// The keyboard is connected to BlueZ.
    fn ble_connected(&mut self) -> bool;
    /// Both halves over GATT.
    fn battery(&mut self) -> Result<Reading>;
    /// This host's Bluetooth adapter addresses, as of the last
    /// [`ble_connected`](Self::ble_connected); empty if unknown.
    fn adapters(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// sysfs for USB, the system D-Bus for BLE. The BlueZ connection is opened
/// on first use and reopened after a failure.
pub struct SystemProbe {
    query: Query,
    timeout: Duration,
    bluez: Option<Bluez>,
    /// From the last tree read.
    adapters: Vec<String>,
}

impl SystemProbe {
    pub fn new(bt_address: Option<String>, timeout: Duration) -> Self {
        Self {
            query: Query {
                address: bt_address,
            },
            timeout,
            bluez: None,
            adapters: Vec::new(),
        }
    }

    fn with_bluez<R>(&mut self, f: impl FnOnce(&mut Bluez, &Query) -> Result<R>) -> Result<R> {
        let bluez = match &mut self.bluez {
            Some(b) => b,
            None => self.bluez.insert(Bluez::connect(self.timeout)?),
        };
        let r = f(bluez, &self.query);
        if matches!(r, Err(crate::Error::Bluez(_))) {
            self.bluez = None;
        }
        r
    }
}

impl Probe for SystemProbe {
    fn usb_present(&mut self) -> bool {
        !serial::discover().is_empty()
    }

    fn ble_connected(&mut self) -> bool {
        let r = self.with_bluez(|b, q| {
            let tree = b.managed_tree()?;
            let found = find_device(&tree, q, &mut Vec::new()).is_ok();
            Ok((found, tree.adapter_addresses))
        });
        match r {
            Ok((found, adapters)) => {
                self.adapters = adapters;
                found
            }
            Err(_) => false,
        }
    }

    fn adapters(&mut self) -> Vec<String> {
        self.adapters.clone()
    }

    fn battery(&mut self) -> Result<Reading> {
        self.with_bluez(|b, q| battery::run(b, q))
    }
}

pub(crate) enum Msg {
    /// Read the batteries now; answer the ticket if there is one.
    Refresh(Option<Ticket>),
    /// The split link changed: read now, and once more after
    /// [`Config::battery_after_split`], when the central has fetched a
    /// rejoined right half's level.
    SplitEdge,
    Quit,
}

pub(crate) fn spawn(
    cfg: Config,
    probe: Box<dyn Probe>,
    events: Sender<Event>,
    link: Sender<link::Msg>,
    rx: Receiver<Msg>,
) {
    std::thread::Builder::new()
        .name("sofleii-watch".into())
        .spawn(move || run(cfg, probe, events, link, rx))
        .expect("spawning the watch thread");
}

fn run(
    cfg: Config,
    mut probe: Box<dyn Probe>,
    events: Sender<Event>,
    link: Sender<link::Msg>,
    rx: Receiver<Msg>,
) {
    let now = Instant::now();
    let (mut usb_due, mut ble_due, mut battery_due) = (now, now, now);
    let mut presence: Option<Presence> = None;
    let mut seen = Presence::default();
    let mut adapters: Option<Vec<String>> = None;
    // The follow-up read after a split edge.
    let mut follow_up: Option<Instant> = None;
    loop {
        let now = Instant::now();
        let mut changed = false;
        if now >= usb_due {
            usb_due = now + cfg.usb_every;
            let usb = probe.usb_present();
            changed |= usb != seen.usb;
            seen.usb = usb;
        }
        if now >= ble_due {
            ble_due = now + cfg.ble_every;
            let ble = probe.ble_connected();
            if ble && !seen.ble {
                // Just connected: read the batteries rather than in a minute.
                battery_due = now;
            }
            changed |= ble != seen.ble;
            seen.ble = ble;
            let now_adapters = probe.adapters();
            if adapters.as_ref() != Some(&now_adapters) {
                adapters = Some(now_adapters.clone());
                if events.send(Event::Adapters(now_adapters)).is_err() {
                    return;
                }
            }
        }
        if changed || presence.is_none() {
            presence = Some(seen);
            let _ = link.send(link::Msg::Presence(seen));
            if events.send(Event::Presence(seen)).is_err() {
                return;
            }
        }
        if follow_up.is_some_and(|t| now >= t) {
            follow_up = None;
            battery_due = now;
        }
        if now >= battery_due {
            battery_due = now + cfg.battery_every;
            if read_battery(&mut *probe, &events).is_none() {
                return;
            }
        }
        let mut wake = usb_due.min(ble_due).min(battery_due);
        if let Some(t) = follow_up {
            wake = wake.min(t);
        }
        match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(Msg::Refresh(ticket)) => {
                battery_due = Instant::now() + cfg.battery_every;
                let Some(r) = read_battery(&mut *probe, &events) else {
                    return;
                };
                if let Some(ticket) = ticket {
                    let _ = events.send(Event::Done {
                        ticket,
                        result: r.map(|()| Outcome::Ok),
                    });
                }
            }
            Ok(Msg::SplitEdge) => {
                battery_due = Instant::now();
                follow_up = Some(Instant::now() + cfg.battery_after_split);
            }
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Reads and reports both batteries; `None` once the front end is gone.
fn read_battery(probe: &mut dyn Probe, events: &Sender<Event>) -> Option<Result<(), ErrorView>> {
    let r = probe.battery().map_err(|e| e.view());
    let out = r.as_ref().map(drop).map_err(Clone::clone);
    if let Ok(reading) = &r {
        for w in &reading.warnings {
            let _ = events.send(Event::log(LogLevel::Warn, w.clone()));
        }
    }
    events
        .send(Event::Battery(Stamped::now(r)))
        .ok()
        .map(|()| out)
}
