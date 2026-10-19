//! The battery and presence thread, on its own BlueZ connection (never
//! Studio RPC, so it never competes with the link for the USB port):
//!
//! - both batteries every [`Config::battery_every`], through
//!   [`commands::battery`](crate::commands::battery) over GATT, and at once
//!   when the BLE connection comes up;
//! - USB presence every [`Config::usb_every`]: the serial discover (sysfs,
//!   no radio);
//! - BLE `Device1.Connected` every [`Config::ble_every`].
//!
//! Presence changes go to the front end ([`Event::Presence`]) and to the
//! link, which retries (or replans) at once.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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
}

/// sysfs for USB, the system D-Bus for BLE. The BlueZ connection is opened
/// on first use and reopened after a failure.
pub struct SystemProbe {
    query: Query,
    timeout: Duration,
    bluez: Option<Bluez>,
}

impl SystemProbe {
    pub fn new(bt_address: Option<String>, timeout: Duration) -> Self {
        Self {
            query: Query {
                address: bt_address,
            },
            timeout,
            bluez: None,
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
        self.with_bluez(|b, q| {
            let tree = b.managed_tree()?;
            Ok(find_device(&tree, q, &mut Vec::new()).is_ok())
        })
        .unwrap_or(false)
    }

    fn battery(&mut self) -> Result<Reading> {
        self.with_bluez(|b, q| battery::run(b, q))
    }
}

pub(crate) enum Msg {
    /// Read the batteries now; answer the ticket if there is one.
    Refresh(Option<Ticket>),
    Quit,
}

pub(crate) fn spawn(
    cfg: Config,
    probe: Box<dyn Probe>,
    events: Sender<Event>,
    link: Sender<link::Msg>,
) -> Sender<Msg> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("sofleii-watch".into())
        .spawn(move || run(cfg, probe, events, link, rx))
        .expect("spawning the watch thread");
    tx
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
        }
        if changed || presence.is_none() {
            presence = Some(seen);
            let _ = link.send(link::Msg::Presence(seen));
            if events.send(Event::Presence(seen)).is_err() {
                return;
            }
        }
        if now >= battery_due {
            battery_due = now + cfg.battery_every;
            if read_battery(&mut *probe, &events).is_none() {
                return;
            }
        }
        let wake = usb_due.min(ble_due).min(battery_due);
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
