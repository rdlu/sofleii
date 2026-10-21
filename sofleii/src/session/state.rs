//! The shared state the front ends render, folded from the event stream by
//! a pure reducer ([`Snapshot::apply`]); [`Snapshot::tick`] marks values
//! stale as time passes. The TUI derives its view from this, and a GUI can
//! keep the same struct on its side of the event stream.
//!
//! A failed read keeps the last good value (its error is shown next to it),
//! and a value goes stale once it is 3x older than its refresh interval.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use super::{Config, ErrorView, Event, LinkState, LogLevel, Presence, Reads, UpdateReport, ser};
use crate::commands::battery::Reading;
use crate::commands::info::{Build, Device, Info};
use crate::commands::profiles::Profiles;
use crate::commands::split::Split;

/// Who the active BLE profile is connected to ([`Snapshot::active_ble`]).
/// It decides where output priority BLE sends keystrokes and Studio: the
/// firmware selects BLE only while the active profile is connected, and
/// falls back to USB otherwise (`get_selected_transport` in
/// `zmk/app/src/endpoints.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveBle {
    /// Not connected to any host: with priority BLE the keyboard stays on
    /// USB until that host connects.
    NotConnected,
    /// Connected to this host: BLE RPC reaches it from here.
    ThisHost,
    /// Connected to another host: Studio over BLE answers there, out of
    /// this host's reach.
    OtherHost,
    /// Connected, but to whom can't be told.
    Unknown,
}

/// A value goes stale after this many refresh intervals without a good read.
pub const STALE_AFTER: u32 = 3;

/// One polled value: the last good read and the latest error.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field<T> {
    /// The last good value.
    pub value: Option<T>,
    /// When `value` was read.
    #[serde(rename = "at_ms", serialize_with = "ser::opt_unix_ms")]
    pub at: Option<SystemTime>,
    /// The latest read's error; cleared by a good read.
    pub error: Option<ErrorView>,
    /// `value` is older than [`STALE_AFTER`] intervals.
    pub stale: bool,
}

impl<T> Default for Field<T> {
    fn default() -> Self {
        Self {
            value: None,
            at: None,
            error: None,
            stale: false,
        }
    }
}

impl<T: Clone> Field<T> {
    fn apply(&mut self, r: &Reads<T>) {
        match &r.value {
            Ok(v) => {
                self.value = Some(v.clone());
                self.at = Some(r.at);
                self.error = None;
                self.stale = false;
            }
            Err(e) => self.error = Some(e.clone()),
        }
    }
}

impl<T> Field<T> {
    /// How old `value` is at `now` (zero if the clock went back).
    pub fn age(&self, now: SystemTime) -> Option<Duration> {
        self.at
            .map(|at| now.duration_since(at).unwrap_or(Duration::ZERO))
    }

    fn tick(&mut self, now: SystemTime, every: Option<Duration>) {
        self.stale = match (self.age(now), every) {
            (Some(age), Some(every)) => age > every.saturating_mul(STALE_AFTER),
            _ => false,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    #[serde(rename = "at_ms", serialize_with = "ser::unix_ms")]
    pub at: SystemTime,
    pub level: LogLevel,
    pub msg: String,
}

/// The refresh intervals staleness is judged by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Intervals {
    poll: Duration,
    poll_unfocused: Duration,
    battery: Duration,
    update: Duration,
    split_settle: Duration,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub link: LinkState,
    /// `None` until the first presence probe.
    pub presence: Option<Presence>,
    /// Read once per (re)connect; never stale by time.
    pub info: Field<Info>,
    /// Profiles and output priority.
    pub status: Field<Profiles>,
    /// The split link, polled with `status`.
    pub split: Field<Split>,
    /// When the split link last went from disconnected to connected, while
    /// the right half hasn't read non-zero since: see
    /// [`right_settling`](Self::right_settling).
    #[serde(rename = "rejoined_at_ms", serialize_with = "ser::opt_unix_ms")]
    pub rejoined_at: Option<SystemTime>,
    pub battery: Field<Reading>,
    /// This host's Bluetooth adapter addresses; empty until known.
    pub adapters: Vec<String>,
    pub update: Field<UpdateReport>,
    /// Oldest first, at most `log_capacity` lines.
    pub log: VecDeque<LogEntry>,
    /// Mirrors the last [`Request::Focus`](super::Request::Focus) the front
    /// end sent: the status interval (and so its staleness) depends on it.
    pub focused: bool,
    #[serde(skip)]
    intervals: Intervals,
    #[serde(skip)]
    log_capacity: usize,
}

impl Snapshot {
    /// Nothing known yet: the "Searching…" state.
    pub fn new(cfg: &Config) -> Self {
        Self {
            link: LinkState::Searching,
            presence: None,
            info: Field::default(),
            status: Field::default(),
            split: Field::default(),
            rejoined_at: None,
            battery: Field::default(),
            adapters: Vec::new(),
            update: Field::default(),
            log: VecDeque::new(),
            focused: true,
            intervals: Intervals {
                poll: cfg.poll,
                poll_unfocused: cfg.poll_unfocused,
                battery: cfg.battery_every,
                update: cfg.update_every,
                split_settle: cfg.split_settle,
            },
            log_capacity: cfg.log_capacity.max(1),
        }
    }

    /// Folds one event in. Pure: no clock, no I/O.
    pub fn apply(&mut self, e: &Event) {
        match e {
            Event::Link(s) => self.link = s.clone(),
            Event::Presence(p) => self.presence = Some(*p),
            Event::Status(r) => self.status.apply(r),
            Event::Split(r) => {
                let was_down = self.split.value.is_some_and(|s| s.disconnected());
                self.split.apply(r);
                match &r.value {
                    Ok(s) if was_down && s.connected == Some(true) => self.rejoined_at = Some(r.at),
                    Ok(s) if s.connected != Some(true) => self.rejoined_at = None,
                    _ => {}
                }
            }
            Event::Info(r) => self.info.apply(r),
            Event::Battery(r) => {
                self.battery.apply(r);
                // A real level ends the settling window early.
                if let Ok(b) = &r.value
                    && b.right.percent.is_some_and(|p| p > 0)
                {
                    self.rejoined_at = None;
                }
            }
            Event::Adapters(a) => self.adapters = a.clone(),
            Event::Update(r) => self.update.apply(r),
            Event::Log { at, level, msg } => {
                if self.log.len() >= self.log_capacity {
                    self.log.pop_front();
                }
                self.log.push_back(LogEntry {
                    at: *at,
                    level: *level,
                    msg: msg.clone(),
                });
            }
            // The front end matches tickets itself.
            Event::Done { .. } => {}
        }
    }

    /// Re-marks staleness for `now`; call it on the front end's tick.
    pub fn tick(&mut self, now: SystemTime) {
        let i = self.intervals;
        let poll = if self.focused {
            i.poll
        } else {
            i.poll_unfocused
        };
        self.status.tick(now, Some(poll));
        self.split.tick(now, Some(poll));
        self.info.tick(now, None);
        self.battery.tick(now, Some(i.battery));
        self.update.tick(now, Some(i.update));
    }

    /// The right half reads 0% within [`Config::split_settle`] of rejoining
    /// the split link: most likely the central's proxy before it has fetched
    /// the real level, so a front end shows "reading…". A 0% that lasts
    /// past the window is shown as read (a flat battery is real).
    pub fn right_settling(&self, now: SystemTime) -> bool {
        let Some(at) = self.rejoined_at else {
            return false;
        };
        let within = now.duration_since(at).unwrap_or(Duration::ZERO) < self.intervals.split_settle;
        let zero = self
            .battery
            .value
            .as_ref()
            .is_some_and(|b| b.right.percent == Some(0));
        within && zero
    }

    /// Who the active BLE profile is connected to, from the profiles and
    /// this host's adapter addresses. With the addresses known, the active
    /// slot's peer address is compared with them; without, a connected
    /// active profile is another host's only when this host has no BLE
    /// link to the keyboard at all (presence), else unknown. `None` until
    /// the profiles are read.
    pub fn active_ble(&self) -> Option<ActiveBle> {
        let p = self.status.value.as_ref()?.active()?;
        if !p.connected {
            return Some(ActiveBle::NotConnected);
        }
        if let Some(addr) = &p.address
            && !self.adapters.is_empty()
        {
            let ours = self.adapters.iter().any(|a| a.eq_ignore_ascii_case(addr));
            return Some(if ours {
                ActiveBle::ThisHost
            } else {
                ActiveBle::OtherHost
            });
        }
        Some(match self.presence {
            Some(p) if !p.ble => ActiveBle::OtherHost,
            _ => ActiveBle::Unknown,
        })
    }

    /// The device name and serial, from `info`.
    pub fn device(&self) -> Option<&Device> {
        self.info.value.as_ref().map(|i| &i.device)
    }

    /// The running firmware build, from `info`.
    pub fn build(&self) -> Option<&Build> {
        self.info.value.as_ref().map(|i| &i.build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::profiles::Profile;
    use crate::session::{ConnectedVia, Stamped};
    use crate::transport::{TransportInfo, TransportKind};

    fn t(secs: u64) -> SystemTime {
        ser::from_ms(1_800_000_000_000 + secs * 1000)
    }

    fn profiles(active: u32) -> Profiles {
        Profiles {
            max_profiles: 2,
            active_profile: Some(active),
            output_priority: Some("usb"),
            profiles: vec![Profile {
                index: active,
                active: true,
                connected: true,
                bonded: true,
                address: None,
                name: None,
            }],
        }
    }

    fn err(m: &str) -> ErrorView {
        ErrorView {
            kind: "timeout",
            message: m.into(),
            exit_code: 4,
        }
    }

    fn status(at: SystemTime, r: Result<Profiles, ErrorView>) -> Event {
        Event::Status(Stamped { at, value: r })
    }

    #[test]
    fn a_failed_read_keeps_the_last_good_value() {
        let mut s = Snapshot::new(&Config::default());
        s.apply(&status(t(0), Ok(profiles(1))));
        s.apply(&status(t(5), Err(err("quiet"))));
        assert_eq!(s.status.value.as_ref().unwrap().active_profile, Some(1));
        assert_eq!(s.status.at, Some(t(0)));
        assert_eq!(s.status.error.as_ref().unwrap().message, "quiet");
        s.apply(&status(t(10), Ok(profiles(0))));
        assert_eq!(s.status.error, None);
        assert_eq!(s.status.value.as_ref().unwrap().active_profile, Some(0));
    }

    #[test]
    fn stale_after_three_intervals() {
        let mut s = Snapshot::new(&Config::default());
        s.tick(t(1000));
        assert!(!s.status.stale, "nothing to be stale yet");
        s.apply(&status(t(0), Ok(profiles(0))));
        s.tick(t(15));
        assert!(!s.status.stale, "3 x 5 s is the limit");
        s.tick(t(16));
        assert!(s.status.stale);
        // Unfocused, the interval is 30 s.
        s.focused = false;
        s.tick(t(16));
        assert!(!s.status.stale);
        s.tick(t(91));
        assert!(s.status.stale);
        // A good read clears it; info is never stale by time.
        s.apply(&status(t(91), Ok(profiles(0))));
        assert!(!s.status.stale);
        s.info.at = Some(t(0));
        s.tick(t(1_000_000));
        assert!(!s.info.stale);
    }

    #[test]
    fn split_events_fold_like_status() {
        let mut s = Snapshot::new(&Config::default());
        let down = Split {
            known: true,
            connected: Some(false),
            is_split: Some(true),
            is_central: Some(true),
        };
        s.apply(&Event::Split(Stamped {
            at: t(0),
            value: Ok(down),
        }));
        assert_eq!(s.split.value, Some(down));
        // A failed read keeps it, and it goes stale with the status interval.
        s.apply(&Event::Split(Stamped {
            at: t(5),
            value: Err(err("quiet")),
        }));
        assert_eq!(s.split.value, Some(down));
        assert!(s.split.error.is_some());
        s.tick(t(16));
        assert!(s.split.stale);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["split"]["value"]["connected"], false);
    }

    fn split_at(at: SystemTime, connected: bool) -> Event {
        Event::Split(Stamped {
            at,
            value: Ok(Split {
                known: true,
                connected: Some(connected),
                is_split: Some(true),
                is_central: Some(true),
            }),
        })
    }

    fn right_at(at: SystemTime, percent: u8) -> Event {
        use crate::commands::battery::{BatteryLevel, BtDevice};
        let lvl = |p| BatteryLevel {
            percent: Some(p),
            state: None,
        };
        Event::Battery(Stamped {
            at,
            value: Ok(Reading {
                source: "bluez",
                device: BtDevice {
                    name: "Eyelash Sofle".into(),
                    address: "DD:1F:23:76:6E:71".into(),
                },
                left: lvl(100),
                right: lvl(percent),
                note: None,
                warnings: Vec::new(),
            }),
        })
    }

    /// Default `split_settle` is 10 s.
    fn rejoined() -> Snapshot {
        let mut s = Snapshot::new(&Config::default());
        s.apply(&split_at(t(0), true));
        s.apply(&split_at(t(5), false));
        s.apply(&split_at(t(10), true));
        s
    }

    #[test]
    fn a_zero_right_after_rejoining_is_settling() {
        let mut s = rejoined();
        // The proxy's 0% while the central fetches the real level.
        s.apply(&right_at(t(11), 0));
        assert!(s.right_settling(t(12)));
        // Another poll that still says connected keeps the window.
        s.apply(&split_at(t(15), true));
        assert!(s.right_settling(t(15)));
        // A real level ends it early, and a later 0 is shown as read.
        s.apply(&right_at(t(16), 66));
        assert!(!s.right_settling(t(16)));
        s.apply(&right_at(t(17), 0));
        assert!(!s.right_settling(t(17)));
    }

    #[test]
    fn a_zero_past_the_window_is_real() {
        let mut s = rejoined();
        s.apply(&right_at(t(11), 0));
        s.apply(&right_at(t(16), 0));
        assert!(s.right_settling(t(19)));
        assert!(!s.right_settling(t(20)), "10 s after the rejoin at t(10)");
    }

    #[test]
    fn no_window_without_a_down_to_up_edge() {
        let mut s = Snapshot::new(&Config::default());
        // First read connected: no edge.
        s.apply(&split_at(t(0), true));
        s.apply(&right_at(t(1), 0));
        assert!(!s.right_settling(t(1)));
        // Going down clears a window.
        let mut s = rejoined();
        s.apply(&right_at(t(11), 0));
        s.apply(&split_at(t(12), false));
        assert!(!s.right_settling(t(12)));
        assert_eq!(s.rejoined_at, None);
    }

    #[test]
    fn the_log_is_bounded() {
        let cfg = Config {
            log_capacity: 3,
            ..Config::default()
        };
        let mut s = Snapshot::new(&cfg);
        for i in 0..5 {
            s.apply(&Event::Log {
                at: t(i),
                level: LogLevel::Info,
                msg: format!("line {i}"),
            });
        }
        let msgs: Vec<_> = s.log.iter().map(|l| l.msg.as_str()).collect();
        assert_eq!(msgs, ["line 2", "line 3", "line 4"]);
    }

    #[test]
    fn link_and_presence_follow_the_events() {
        let mut s = Snapshot::new(&Config::default());
        let via = ConnectedVia {
            transport: TransportInfo {
                kind: TransportKind::Ble,
                device: "DD:1F:23:76:6E:71".into(),
            },
            auto: true,
        };
        s.apply(&Event::Link(LinkState::Connected { via }));
        s.apply(&Event::Presence(Presence {
            usb: false,
            ble: true,
        }));
        assert_eq!(s.link.kind(), Some(TransportKind::Ble));
        assert!(s.presence.unwrap().ble);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["link"]["state"], "connected");
        assert!(json.get("intervals").is_none());
    }
}
