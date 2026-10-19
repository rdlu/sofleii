//! Opening a Studio RPC link the way `--transport`/`--device`/`--bt-address`
//! say, for every front end: [`transport::plan`] picks the transport, then
//! `auto` probes USB briefly and falls back to BLE when USB is deaf (Studio
//! only answers on the selected output endpoint).
//!
//! The CLI connects once per process ([`connect`]). The TUI's session
//! thread reconnects for every lease, so [`Connector`] keeps the outcome
//! (USB deaf, so BLE) between calls until [`Connector::invalidate`] or a
//! failed connect drops it. Everything that touches the system (serial
//! ports, BlueZ) goes through [`Opener`], so tests swap in mock transports.

use std::time::Duration;

use crate::bluez::Bluez;
use crate::client::{Client, DEFAULT_TIMEOUT};
use crate::commands::battery;
use crate::gatt::{Query, find_device};
use crate::transport::{
    self, Plan, Preference, Transport, TransportInfo, TransportKind, ble::BleTransport, serial,
    serial::SerialTransport,
};
use crate::{Error, Result};

/// `status` reads the batteries with at most this timeout, to stay snappy.
pub const STATUS_BATTERY_TIMEOUT: Duration = Duration::from_millis(1000);
/// With `--transport auto`, USB gets this long to answer before sofleii
/// falls back to BLE (Studio only answers on the selected output endpoint).
pub const AUTO_USB_PROBE: Duration = Duration::from_millis(1000);

/// A client over whichever transport [`Connector`] opened. `Send`, so it can
/// move into a worker thread.
pub type DynClient = Client<Box<dyn Transport>>;

/// What to connect to: the CLI's global flags, without clap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `--transport`.
    pub transport: Preference,
    /// `--device`: a serial port path (implies USB).
    pub device: Option<String>,
    /// `--bt-address`: the keyboard's BLE address (implies BLE).
    pub bt_address: Option<String>,
    /// `--timeout`: each response (and each D-Bus call) gets this long.
    pub timeout: Duration,
}

impl Default for Config {
    /// `auto`, no pinned device or address, the CLI's default 3 s timeout.
    fn default() -> Self {
        Self {
            transport: Preference::Auto,
            device: None,
            bt_address: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// An open link.
pub struct Connected {
    pub client: DynClient,
    /// What it connected over (the same as `client.transport_info()`).
    pub info: TransportInfo,
}

/// The system calls a connect makes, so tests can replace them.
pub trait Opener {
    /// Whether a serial port with the keyboard's product string exists.
    fn usb_present(&self) -> bool;
    /// Whether BlueZ has the keyboard connected, or why not.
    fn ble_connected(
        &self,
        address: Option<&str>,
        timeout: Duration,
    ) -> std::result::Result<(), String>;
    /// Opens the serial port (`device`, or the discovered one).
    fn open_usb(&self, device: Option<&str>) -> Result<Box<dyn Transport>>;
    /// Subscribes to the keyboard's Studio GATT service.
    fn open_ble(&self, address: Option<&str>, timeout: Duration) -> Result<Box<dyn Transport>>;
}

/// The real [`Opener`]: serial ports and BlueZ.
#[derive(Debug, Clone, Copy, Default)]
pub struct System;

impl Opener for System {
    fn usb_present(&self) -> bool {
        !serial::discover().is_empty()
    }
    fn ble_connected(
        &self,
        address: Option<&str>,
        timeout: Duration,
    ) -> std::result::Result<(), String> {
        ble_connected(address, timeout)
    }
    fn open_usb(&self, device: Option<&str>) -> Result<Box<dyn Transport>> {
        Ok(Box::new(SerialTransport::open(device)?))
    }
    fn open_ble(&self, address: Option<&str>, timeout: Duration) -> Result<Box<dyn Transport>> {
        Ok(Box::new(BleTransport::connect(address, timeout)?))
    }
}

/// Connects again and again with the same [`Config`], remembering the
/// [`Plan`] that worked (and that USB was deaf) in between.
pub struct Connector<O: Opener = System> {
    config: Config,
    opener: O,
    plan: Option<Plan>,
}

impl Connector {
    pub fn new(config: Config) -> Self {
        Self::with_opener(config, System)
    }
}

impl<O: Opener> Connector<O> {
    pub fn with_opener(config: Config, opener: O) -> Self {
        Self {
            config,
            opener,
            plan: None,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The remembered plan, if the last connect worked and nothing
    /// invalidated it since. After an `auto` fallback it is BLE.
    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Forgets the remembered plan, so the next [`Connector::connect`]
    /// looks at what's present again (after a timeout on a healthy link:
    /// the output endpoint probably moved).
    pub fn invalidate(&mut self) {
        self.plan = None;
    }

    /// Opens the link: the remembered plan, or a fresh one from the config
    /// and what's present. With `auto` and USB picked only because the port
    /// exists, USB gets [`AUTO_USB_PROBE`] to answer a `get_device_info`;
    /// if it times out and BLE is connected, BLE is used (and remembered).
    /// Any error forgets the plan.
    ///
    /// Errors as in [`transport::plan`] (usage 3, not found 2), plus the
    /// transports' own (exit 4 for I/O, a busy port, the probe's timeout
    /// when BLE isn't there to fall back to).
    pub fn connect(&mut self) -> Result<Connected> {
        let r = self.open();
        if r.is_err() {
            self.plan = None;
        }
        r
    }

    fn open(&mut self) -> Result<Connected> {
        let timeout = self.config.timeout;
        let chosen = match &self.plan {
            Some(p) => p.clone(),
            None => {
                let c = &self.config;
                let bt_address = c.bt_address.as_deref();
                transport::plan(
                    c.transport,
                    c.device.as_deref(),
                    bt_address,
                    || self.opener.usb_present(),
                    || {
                        self.opener
                            .ble_connected(bt_address, timeout.min(DEFAULT_TIMEOUT))
                    },
                )?
            }
        };
        let client = match &chosen {
            Plan::Ble { address } => self.ble(address.as_deref())?,
            Plan::Usb { device, auto } => {
                let mut client = Client::new(self.opener.open_usb(device.as_deref())?, timeout);
                if *auto {
                    // Studio only listens on the selected output endpoint:
                    // with output priority BLE and the profile connected,
                    // USB is plugged in but deaf. Probe it briefly, then
                    // fall back to BLE.
                    client.set_timeout(timeout.min(AUTO_USB_PROBE));
                    match client.get_device_info() {
                        Ok(_) => client.set_timeout(timeout),
                        Err(e @ Error::Timeout { .. }) => {
                            if self
                                .opener
                                .ble_connected(None, timeout.min(DEFAULT_TIMEOUT))
                                .is_err()
                            {
                                return Err(e);
                            }
                            log::info!(
                                "note: USB ({}) didn't answer (Studio only answers on the \
                                 selected output, BLE here?); using BLE",
                                client.transport_info().device
                            );
                            let client = self.ble(None)?;
                            self.plan = Some(Plan::Ble { address: None });
                            return Ok(connected(client));
                        }
                        Err(e) => return Err(e),
                    }
                }
                client
            }
        };
        self.plan = Some(chosen);
        Ok(connected(client))
    }

    fn ble(&self, address: Option<&str>) -> Result<DynClient> {
        let timeout = self.config.timeout;
        Ok(Client::new(
            self.opener.open_ble(address, timeout)?,
            timeout,
        ))
    }

    /// Opens the transport that isn't `current`, to read back after a write
    /// handed Studio over to it. `device`/`bt_address` only pin the
    /// transport they name. Leaves the remembered plan alone.
    pub fn open_other(&self, current: TransportKind) -> Result<DynClient> {
        let timeout = self.config.timeout;
        let t = match current {
            TransportKind::Usb => {
                let address = self.config.bt_address.as_deref();
                self.opener
                    .ble_connected(address, timeout.min(DEFAULT_TIMEOUT))
                    .map_err(Error::NotFound)?;
                self.opener.open_ble(address, timeout)?
            }
            TransportKind::Ble => {
                if !self.opener.usb_present() {
                    return Err(Error::NotFound("no \"Eyelash Sofle\" serial port".into()));
                }
                self.opener.open_usb(None)?
            }
        };
        Ok(Client::new(t, timeout))
    }
}

fn connected(client: DynClient) -> Connected {
    let info = client.transport_info();
    Connected { client, info }
}

/// Connects once (the CLI): a fresh [`Connector`] over the real system.
pub fn connect(config: &Config) -> Result<Connected> {
    Connector::new(config.clone()).connect()
}

/// Whether the keyboard is connected to BlueZ, or why not.
pub fn ble_connected(
    bt_address: Option<&str>,
    timeout: Duration,
) -> std::result::Result<(), String> {
    let reason = |e: Error| match e {
        Error::NotFound(m) => m,
        e => e.to_string(),
    };
    let bluez = Bluez::connect(timeout).map_err(reason)?;
    let tree = bluez.managed_tree().map_err(reason)?;
    let query = Query {
        address: bt_address.map(str::to_string),
    };
    find_device(&tree, &query, &mut Vec::new())
        .map(drop)
        .map_err(reason)
}

/// Reads both batteries over BlueZ; its warnings go to the log.
pub fn read_battery(bt_address: Option<&str>, timeout: Duration) -> Result<battery::Reading> {
    let mut bluez = Bluez::connect(timeout)?;
    let r = battery::run(
        &mut bluez,
        &battery::Query {
            address: bt_address.map(str::to_string),
        },
    )?;
    for w in &r.warnings {
        log::warn!("{w}");
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ResponseSubsystem;
    use crate::error::exit;
    use crate::proto::core;
    use crate::transport::mock::Mock;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    /// What's "present", and a log of the system calls made.
    #[derive(Default)]
    struct Fake {
        usb: Cell<bool>,
        /// USB answers the probe (else it's deaf).
        usb_answers: Cell<bool>,
        ble: Cell<bool>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Fake {
        fn new(usb: bool, usb_answers: bool, ble: bool) -> Self {
            let f = Fake::default();
            f.usb.set(usb);
            f.usb_answers.set(usb_answers);
            f.ble.set(ble);
            f
        }
        fn log(&self, call: &'static str) {
            self.calls.borrow_mut().push(call);
        }
        fn take(&self) -> Vec<&'static str> {
            std::mem::take(&mut self.calls.borrow_mut())
        }
    }

    fn device_info() -> ResponseSubsystem {
        ResponseSubsystem::Core(core::Response {
            response_type: Some(core::response::ResponseType::GetDeviceInfo(
                core::GetDeviceInfoResponse {
                    name: "Eyelash Sofle".into(),
                    serial_number: vec![],
                },
            )),
        })
    }

    impl Opener for &Fake {
        fn usb_present(&self) -> bool {
            self.log("usb_present");
            self.usb.get()
        }
        fn ble_connected(&self, _: Option<&str>, _: Duration) -> std::result::Result<(), String> {
            self.log("ble_connected");
            if self.ble.get() {
                Ok(())
            } else {
                Err("not connected".into())
            }
        }
        fn open_usb(&self, _: Option<&str>) -> Result<Box<dyn Transport>> {
            self.log("open_usb");
            if !self.usb.get() {
                return Err(Error::NotFound("no serial port".into()));
            }
            Ok(Box::new(if self.usb_answers.get() {
                Mock::echoing(vec![device_info()])
            } else {
                Mock::new(VecDeque::new())
            }))
        }
        fn open_ble(&self, _: Option<&str>, _: Duration) -> Result<Box<dyn Transport>> {
            self.log("open_ble");
            if !self.ble.get() {
                return Err(Error::NotFound("not connected over BLE".into()));
            }
            Ok(Box::new(
                Mock::new(VecDeque::new()).over(TransportKind::Ble),
            ))
        }
    }

    fn config(transport: Preference) -> Config {
        Config {
            transport,
            timeout: Duration::from_millis(30),
            ..Config::default()
        }
    }

    #[test]
    fn auto_falls_back_to_ble_when_usb_is_deaf() {
        let fake = Fake::new(true, false, true);
        let mut c = Connector::with_opener(config(Preference::Auto), &fake);
        let got = c.connect().unwrap();
        assert_eq!(got.info.kind, TransportKind::Ble);
        assert_eq!(got.client.transport_info().kind, TransportKind::Ble);
        assert_eq!(
            fake.take(),
            ["usb_present", "open_usb", "ble_connected", "open_ble"]
        );
        assert_eq!(c.plan(), Some(&Plan::Ble { address: None }));
    }

    #[test]
    fn auto_keeps_usb_when_it_answers() {
        let fake = Fake::new(true, true, true);
        let mut c = Connector::with_opener(config(Preference::Auto), &fake);
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Usb);
        assert_eq!(fake.take(), ["usb_present", "open_usb"]);
        assert_eq!(
            c.plan(),
            Some(&Plan::Usb {
                device: None,
                auto: true
            })
        );
    }

    #[test]
    fn deaf_usb_without_ble_is_the_timeout() {
        let fake = Fake::new(true, false, false);
        let mut c = Connector::with_opener(config(Preference::Auto), &fake);
        let e = c.connect().err().unwrap();
        assert!(matches!(e, Error::Timeout { .. }), "{e}");
        assert_eq!(e.exit_code(), exit::TRANSPORT);
        assert_eq!(c.plan(), None);
    }

    #[test]
    fn the_plan_is_cached_until_invalidated() {
        let fake = Fake::new(true, false, true);
        let mut c = Connector::with_opener(config(Preference::Auto), &fake);
        c.connect().unwrap();
        fake.take();
        // USB is remembered as deaf: straight to BLE, no probe.
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Ble);
        assert_eq!(fake.take(), ["open_ble"]);
        // The endpoint moved back to USB; invalidate replans and probes.
        fake.usb_answers.set(true);
        c.invalidate();
        assert_eq!(c.plan(), None);
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Usb);
        assert_eq!(fake.take(), ["usb_present", "open_usb"]);
    }

    #[test]
    fn a_failed_connect_forgets_the_plan() {
        let fake = Fake::new(false, false, true);
        let mut c = Connector::with_opener(config(Preference::Auto), &fake);
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Ble);
        assert_eq!(c.plan(), Some(&Plan::Ble { address: None }));
        fake.ble.set(false);
        let e = c.connect().err().unwrap();
        assert_eq!(e.exit_code(), exit::NOT_FOUND);
        assert_eq!(c.plan(), None);
        // Next time it looks again, and finds USB.
        fake.usb.set(true);
        fake.usb_answers.set(true);
        fake.take();
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Usb);
        assert_eq!(fake.take(), ["usb_present", "open_usb"]);
    }

    #[test]
    fn explicit_transport_never_probes() {
        let fake = Fake::new(true, false, true);
        let mut c = Connector::with_opener(config(Preference::Usb), &fake);
        // A deaf USB is still USB when asked for: no probe, no fallback.
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Usb);
        assert_eq!(fake.take(), ["open_usb"]);
        let mut c = Connector::with_opener(config(Preference::Ble), &fake);
        assert_eq!(c.connect().unwrap().info.kind, TransportKind::Ble);
        assert_eq!(fake.take(), ["open_ble"]);
    }

    #[test]
    fn contradicting_flags_are_usage_errors() {
        let fake = Fake::new(true, true, true);
        let cfg = Config {
            device: Some("/dev/ttyACM1".into()),
            bt_address: Some("DD:1F:23:76:6E:71".into()),
            ..config(Preference::Auto)
        };
        let e = Connector::with_opener(cfg, &fake).connect().err().unwrap();
        assert_eq!(e.exit_code(), exit::USAGE);
        assert!(fake.take().is_empty());
    }

    #[test]
    fn open_other_opens_the_other_kind() {
        let fake = Fake::new(true, true, true);
        let c = Connector::with_opener(config(Preference::Auto), &fake);
        let other = c.open_other(TransportKind::Usb).unwrap();
        assert_eq!(other.transport_info().kind, TransportKind::Ble);
        let other = c.open_other(TransportKind::Ble).unwrap();
        assert_eq!(other.transport_info().kind, TransportKind::Usb);
        assert_eq!(c.plan(), None, "open_other leaves the plan alone");

        let fake = Fake::new(false, false, false);
        let c = Connector::with_opener(config(Preference::Auto), &fake);
        let e = c.open_other(TransportKind::Usb).err().unwrap();
        assert_eq!(e.to_string(), "keyboard not found: not connected");
        let e = c.open_other(TransportKind::Ble).err().unwrap();
        assert_eq!(
            e.to_string(),
            "keyboard not found: no \"Eyelash Sofle\" serial port"
        );
    }

    #[test]
    fn a_connection_can_move_to_a_worker_thread() {
        fn send<T: Send>() {}
        send::<Connector>();
        send::<Connected>();
        send::<DynClient>();
    }
}
