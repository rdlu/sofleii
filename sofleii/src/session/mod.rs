//! The session: the long-running core a front end (the TUI now, a Tauri GUI
//! later) talks to. Three threads, std `mpsc` channels, no async runtime:
//!
//! - [`link`]: the only thread that touches Studio RPC. It **leases** the
//!   connection: connects for a batch of requests and drops the client after
//!   [`Config::lease_idle`], so the exclusively opened USB port stays free
//!   for waybar's `sofleii status`. Polls profiles, output priority and
//!   the split link, reads `info` once per (re)connect, runs the write
//!   actions, and reconnects with backoff.
//! - [`watch`]: batteries over BlueZ GATT (never RPC), USB presence (sysfs)
//!   and BLE `Connected`, on its own BlueZ connection. Presence edges go to
//!   `link` (an immediate retry) and to the front end.
//! - [`jobs`]: one-off work off the hot path; for now the update check,
//!   cached in `$XDG_CACHE_HOME/sofleii/update.json`.
//!
//! The front end sends [`Request`]s through a [`SessionHandle`] (never
//! blocks) and reads [`Event`]s from one `Receiver`. Every event is
//! `Serialize + Clone`, so a GUI can forward the same stream;
//! [`state::Snapshot::apply`] folds it into the shared state both front ends
//! render. The core never prompts: [`Request::risk`] says which requests a
//! front end should confirm, the same way everywhere. No UI code here
//! (`tests/layering.rs`).

pub mod jobs;
pub mod link;
pub mod state;
pub mod watch;

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::commands::battery::Reading;
use crate::commands::info::Info;
use crate::commands::profiles::Profiles;
use crate::commands::split::Split;
use crate::commands::update::{DEFAULT_REPO, Http};
use crate::commands::{output, profile};
use crate::connect;
use crate::github::GitHub;
use crate::transport::TransportKind;

pub use crate::error::ErrorView;
pub use jobs::UpdateReport;
pub use link::{Connect, ConnectedVia};
pub use state::{ActiveBle, Snapshot};
pub use watch::{Probe, SystemProbe};

/// Intervals and paths. The defaults are the #44 plan's; tests shrink them
/// instead of sleeping.
#[derive(Debug, Clone)]
pub struct Config {
    /// Profiles, output priority and the split link, while the front end
    /// is focused.
    pub poll: Duration,
    /// The same, while it isn't ([`Request::Focus`]`(false)`).
    pub poll_unfocused: Duration,
    /// The RPC client is dropped after this long without a request.
    pub lease_idle: Duration,
    /// Reconnect backoff: `backoff_base * 2^n`, at most `backoff_cap`.
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    pub battery_every: Duration,
    /// After the split link changes, the batteries are read at once and
    /// again this much later: a reconnected right half's level takes the
    /// central a moment to fetch (until then its proxy says 0%).
    pub battery_after_split: Duration,
    /// How long after the right half rejoins a right-half 0% shows as
    /// "reading…" ([`Snapshot::right_settling`]) instead of 0%.
    pub split_settle: Duration,
    pub usb_every: Duration,
    pub ble_every: Duration,
    /// How often the update check runs while the session is up.
    pub update_every: Duration,
    /// How long a cached update check is honoured.
    pub update_ttl: Duration,
    /// `OWNER/NAME` whose releases the update check compares with.
    pub repo: String,
    /// `None`: `$XDG_CACHE_HOME/sofleii`, else `~/.cache/sofleii`.
    pub cache_dir: Option<PathBuf>,
    /// Each BlueZ method call (battery, presence).
    pub bluez_timeout: Duration,
    /// Each GitHub request (update check).
    pub github_timeout: Duration,
    /// Lines kept in [`Snapshot::log`].
    pub log_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(5),
            poll_unfocused: Duration::from_secs(30),
            lease_idle: Duration::from_millis(1500),
            backoff_base: Duration::from_millis(500),
            backoff_cap: Duration::from_secs(5),
            battery_every: Duration::from_secs(60),
            battery_after_split: Duration::from_secs(5),
            split_settle: Duration::from_secs(10),
            usb_every: Duration::from_secs(2),
            ble_every: Duration::from_secs(5),
            update_every: Duration::from_secs(6 * 3600),
            update_ttl: jobs::UPDATE_TTL,
            repo: DEFAULT_REPO.into(),
            cache_dir: None,
            bluez_timeout: Duration::from_secs(3),
            github_timeout: Duration::from_secs(10),
            log_capacity: 200,
        }
    }
}

/// Names one [`Request`]; its [`Event::Done`] carries the same ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Ticket(pub u64);

/// What to re-read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Status and info over RPC, the batteries, and the update check (which
    /// honours its cache).
    All,
    /// Profiles, output priority and the split link.
    Status,
    Battery,
    Info,
    /// The update check, honouring the cache ([`Request::CheckUpdate`]
    /// skips it).
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Refresh(Scope),
    /// 0-based, as `profiles` lists them.
    SwitchProfile(u32),
    /// `""` clears the name.
    RenameProfile(u32, String),
    UnpairProfile(u32),
    /// The output priority (`sofleii output usb|ble`).
    SetOutput(TransportKind),
    /// The update check now, past the cache.
    CheckUpdate,
    /// Drops the link and plans the transport afresh.
    Reconnect,
    /// The front end gained (`true`) or lost focus: slower polling while
    /// it's in the background.
    Focus(bool),
    /// Stops every thread. Gets no [`Event::Done`].
    Quit,
}

/// How a front end should treat a request before sending it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    None,
    /// Studio RPC moves to the other transport, or the BLE connection drops
    /// (a profile switch over BLE, or to an empty slot, which also starts
    /// pairing on it): confirm, then the session follows it.
    DropsCurrentLink,
    /// Can't be undone from here: an unpair (the host has to pair again),
    /// or output BLE while the active profile is another host's (Studio
    /// answers there, and this host loses the keyboard until the output is
    /// switched back on the keyboard or from that host). Always confirm.
    Destructive,
}

impl Request {
    /// Whether to confirm first, judged from what the front end shows
    /// (`snap`), with the same rules as the CLI's handover logic
    /// ([`crate::commands::handover`]). Unknown state never asks, except
    /// for an unpair, which always asks unless the slot is known empty.
    pub fn risk(&self, snap: &Snapshot) -> Risk {
        let kind = snap.link.kind();
        let profiles = snap.status.value.as_ref();
        match *self {
            Request::UnpairProfile(n) => {
                let empty = profiles
                    .and_then(|p| p.profiles.iter().find(|s| s.index == n))
                    .is_some_and(|s| !s.bonded);
                if empty { Risk::None } else { Risk::Destructive }
            }
            Request::SwitchProfile(n) => {
                let Some(p) = profiles else {
                    return Risk::None;
                };
                let changes = p.active_profile != Some(n);
                let hands_over =
                    kind == Some(TransportKind::Ble) || p.output_priority == Some("ble");
                // A known-empty slot starts pairing on it, and the active
                // profile's BLE connection (likely this host's) drops.
                let empty = p.profiles.iter().any(|s| s.index == n && !s.bonded);
                if changes && (hands_over || empty) {
                    Risk::DropsCurrentLink
                } else {
                    Risk::None
                }
            }
            // BLE is only selected while the active profile is connected
            // (else the firmware stays on USB), and then Studio answers on
            // that profile's host: this one, or out of reach. Not connected
            // still asks: the link moves as soon as that host connects.
            Request::SetOutput(target) => match kind {
                Some(k) if k != target => {
                    if target == TransportKind::Ble
                        && snap.active_ble() == Some(ActiveBle::OtherHost)
                    {
                        Risk::Destructive
                    } else {
                        Risk::DropsCurrentLink
                    }
                }
                _ => Risk::None,
            },
            Request::Refresh(_)
            | Request::RenameProfile(..)
            | Request::CheckUpdate
            | Request::Reconnect
            | Request::Focus(_)
            | Request::Quit => Risk::None,
        }
    }
}

impl Event {
    /// A [`Event::Log`] stamped now.
    pub fn log(level: LogLevel, msg: impl Into<String>) -> Self {
        Event::Log {
            at: SystemTime::now(),
            level,
            msg: msg.into(),
        }
    }
}

/// A value and when it was produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stamped<T> {
    #[serde(rename = "at_ms", serialize_with = "ser::unix_ms")]
    pub at: SystemTime,
    pub value: T,
}

impl<T> Stamped<T> {
    pub fn now(value: T) -> Self {
        Self {
            at: SystemTime::now(),
            value,
        }
    }
}

/// A read's result as events carry it.
pub type Reads<T> = Stamped<Result<T, ErrorView>>;

/// The RPC link as the front end shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LinkState {
    /// No keyboard found (yet); retrying quietly, and at once on a presence
    /// edge.
    Searching,
    /// Opening a transport, or opened and waiting for its first answer;
    /// `kind` is the one being opened (or the last one), if known.
    Connecting { kind: Option<TransportKind> },
    /// A request answered on this link. Stays `Connected` while the lease
    /// is released and retaken between polls, until a request fails.
    Connected { via: ConnectedVia },
    /// The transport is open but Studio doesn't answer (on BLE, usually USB
    /// being the selected output). The session replans at once, and stays
    /// `Deaf` through the replans until something answers.
    Deaf {
        kind: Option<TransportKind>,
        hint: Option<String>,
    },
    /// The transport failed; the next try is `retry_in` away.
    Lost {
        reason: String,
        #[serde(rename = "retry_in_ms", serialize_with = "ser::ms")]
        retry_in: Duration,
    },
}

impl LinkState {
    /// The transport in use, when connected.
    pub fn kind(&self) -> Option<TransportKind> {
        match self {
            LinkState::Connected { via } => Some(via.transport.kind),
            _ => None,
        }
    }
}

/// Whether the keyboard is reachable over each transport, without RPC:
/// `usb` is the serial port's presence, `ble` BlueZ's `Device1.Connected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Presence {
    pub usb: bool,
    pub ble: bool,
}

/// What a finished request produced.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// A refresh, focus change or reconnect.
    Ok,
    Profile(profile::Outcome),
    Output(output::Outcome),
    Update(Box<UpdateReport>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Everything the session tells the front end.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    Link(LinkState),
    Presence(Presence),
    /// Profiles and output priority (the polled part of `status`).
    Status(Reads<Profiles>),
    /// The split link, right after each good [`Event::Status`] (unknown on
    /// firmware without the split-info fix).
    Split(Reads<Split>),
    /// Once per (re)connect, and on [`Scope::Info`].
    Info(Reads<Info>),
    Battery(Reads<Reading>),
    /// This host's Bluetooth adapter addresses (BlueZ `Adapter1.Address`),
    /// when they change; empty if BlueZ can't say. Compared with the
    /// profiles' peer addresses ([`Snapshot::active_ble`]).
    Adapters(Vec<String>),
    Update(Reads<UpdateReport>),
    Done {
        ticket: Ticket,
        result: Result<Outcome, ErrorView>,
    },
    /// What the session did, in words (connects, handovers, retries).
    Log {
        #[serde(rename = "at_ms", serialize_with = "ser::unix_ms")]
        at: SystemTime,
        level: LogLevel,
        msg: String,
    },
}

/// The pieces a session runs on; [`Session::spawn`] fills in the real ones.
pub struct Backends {
    pub connector: Box<dyn Connect>,
    pub probe: Box<dyn Probe>,
    pub http: Box<dyn Http + Send>,
}

pub struct Session;

impl Session {
    /// Starts the threads on the real system: Studio RPC through a
    /// [`connect::Connector`] for `link` (the CLI's `--transport`,
    /// `--device`, `--bt-address` and `--timeout`), the batteries and
    /// presence over sysfs and BlueZ, and GitHub for the update check.
    pub fn spawn(config: Config, link: connect::Config) -> (SessionHandle, Receiver<Event>) {
        let probe = SystemProbe::new(link.bt_address.clone(), config.bluez_timeout);
        let connector = connect::Connector::new(link);
        let http = GitHub::new(config.github_timeout);
        Self::spawn_with(
            config,
            Backends {
                connector: Box::new(connector),
                probe: Box::new(probe),
                http: Box::new(http),
            },
        )
    }

    pub fn spawn_with(config: Config, b: Backends) -> (SessionHandle, Receiver<Event>) {
        let (events, rx) = mpsc::channel();
        let jobs = jobs::spawn(config.clone(), b.http, events.clone());
        // Made first: the link tells watch about split edges, and watch
        // tells the link about presence edges.
        let (watch, watch_rx) = mpsc::channel();
        let link = link::spawn(
            config.clone(),
            b.connector,
            events.clone(),
            jobs.clone(),
            watch.clone(),
        );
        watch::spawn(config, b.probe, events.clone(), link.clone(), watch_rx);
        let handle = SessionHandle {
            link,
            watch,
            jobs,
            events,
            next: Arc::new(AtomicU64::new(1)),
        };
        (handle, rx)
    }
}

/// The front end's side of the session. Cheap to clone; [`send`] never
/// blocks.
///
/// [`send`]: SessionHandle::send
#[derive(Clone)]
pub struct SessionHandle {
    link: Sender<link::Msg>,
    watch: Sender<watch::Msg>,
    jobs: Sender<jobs::Msg>,
    /// To answer a request whose thread is gone.
    events: Sender<Event>,
    next: Arc<AtomicU64>,
}

impl SessionHandle {
    /// Queues `req` for the thread that owns it. Each request but
    /// [`Request::Quit`] gets exactly one [`Event::Done`] with this ticket.
    pub fn send(&self, req: Request) -> Ticket {
        let t = Ticket(self.next.fetch_add(1, Ordering::Relaxed));
        let sent = match req {
            Request::Quit => {
                let _ = self.watch.send(watch::Msg::Quit);
                let _ = self.jobs.send(jobs::Msg::Quit);
                let _ = self.link.send(link::Msg::Req(t, Request::Quit));
                true
            }
            Request::Refresh(Scope::Battery) => {
                self.watch.send(watch::Msg::Refresh(Some(t))).is_ok()
            }
            Request::Refresh(Scope::Update) => self
                .jobs
                .send(jobs::Msg::Check {
                    ticket: Some(t),
                    force: false,
                })
                .is_ok(),
            Request::CheckUpdate => self
                .jobs
                .send(jobs::Msg::Check {
                    ticket: Some(t),
                    force: true,
                })
                .is_ok(),
            Request::Refresh(Scope::All) => {
                // The link answers for the whole refresh; the others just run.
                let _ = self.watch.send(watch::Msg::Refresh(None));
                let _ = self.jobs.send(jobs::Msg::Check {
                    ticket: None,
                    force: false,
                });
                self.link.send(link::Msg::Req(t, req)).is_ok()
            }
            req => self.link.send(link::Msg::Req(t, req)).is_ok(),
        };
        if !sent {
            let _ = self.events.send(Event::Done {
                ticket: t,
                result: Err(stopped()),
            });
        }
        t
    }
}

fn stopped() -> ErrorView {
    ErrorView {
        kind: "stopped",
        message: "the session has stopped".into(),
        exit_code: crate::error::exit::TRANSPORT,
    }
}

/// Serde helpers: times and durations as integer milliseconds.
pub(crate) mod ser {
    use serde::Serializer;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn to_ms(t: SystemTime) -> u64 {
        t.duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }

    pub fn from_ms(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    pub fn unix_ms<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(to_ms(*t))
    }

    pub fn opt_unix_ms<S: Serializer>(t: &Option<SystemTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_some(&to_ms(*t)),
            None => s.serialize_none(),
        }
    }

    pub fn ms<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }
}
