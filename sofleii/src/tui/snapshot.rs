//! What the dashboard shows, as plain data: a thin view derived from the
//! session's [`session::Snapshot`] (the one model, folded from the event
//! stream) at a given time, so the view renders a [`Snapshot`] and nothing
//! else and is testable without a keyboard. [`demo`] is a fixture session
//! snapshot for screenshots (`SOFLEII_TUI_DEMO=1`) and the render tests.

use std::time::{Duration, SystemTime};

use crate::commands::battery::{BatteryLevel, BtDevice, Reading};
use crate::commands::info::{Build, Device, Hardware, Info, Studio};
use crate::commands::profiles::{Profile, Profiles};
use crate::commands::status::{self, Split};
use crate::commands::update::{self, Check, Latest, Release, Running};
use crate::session::state::Field;
use crate::session::{self, ConnectedVia, Event, LinkState, LogLevel, Stamped, UpdateReport};
use crate::transport::{TransportInfo, TransportKind};

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// As the session reports it, with `Lost`'s `retry_in` counted down to
    /// what is left.
    pub link: LinkState,
    /// `None` until the link connects or a status read arrives: the
    /// "Searching…" state.
    pub keyboard: Option<Keyboard>,
    /// The log pane, oldest first: the session's log and the `log` crate's
    /// records, merged by time.
    pub log: Vec<LogLine>,
}

#[derive(Debug, Clone)]
pub struct Keyboard {
    /// The device name from `info`.
    pub name: String,
    /// The open link; `None` while it is down (the last values stay).
    pub transport: Option<TransportInfo>,
    /// Picked by `--transport auto` (vs. forced with a flag).
    pub auto: bool,
    /// Where keystrokes go right now, as `status` guesses it.
    pub output: Option<TransportKind>,
    /// `"usb"` or `"ble"`, as in `status`.
    pub output_priority: Option<&'static str>,
    /// Priority usb, yet the keystrokes go over BLE: the left half has no
    /// USB ([`crate::output::USB_LEFT_ONLY`]).
    pub usb_left_only: bool,
    /// `None` until `info` is read.
    pub studio_locked: Option<bool>,
    /// Every slot, as `profiles` reports them.
    pub profiles: Vec<Profile>,
    /// The profiles and output priority read.
    pub status: Health,
    pub battery: Battery,
    pub firmware: Firmware,
    /// `None` until the first read; `known: false` on firmware without the
    /// split-info fix.
    pub split: Option<Split>,
    /// The split read.
    pub split_health: Health,
}

/// How fresh one session field is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    /// How long ago the value was read.
    pub age: Option<Duration>,
    /// Older than [`session::state::STALE_AFTER`] refresh intervals.
    pub stale: bool,
    /// The latest read's error (the value above is the last good one).
    pub error: Option<String>,
}

impl Health {
    fn of<T>(f: &Field<T>, now: SystemTime) -> Self {
        Self {
            age: f.age(now),
            stale: f.stale,
            error: f.error.as_ref().map(|e| e.message.clone()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Battery {
    pub left: Option<u8>,
    /// `None` when [`right_disconnected`](Self::right_disconnected): the
    /// central's proxy keeps a stale 0% for a gone right half.
    pub right: Option<u8>,
    /// The split link is known to be down.
    pub right_disconnected: bool,
    /// A 0% right after the right half rejoined: shown as "reading…"
    /// ([`session::Snapshot::right_settling`]).
    pub right_settling: bool,
    pub health: Health,
}

#[derive(Debug, Clone, Default)]
pub struct Firmware {
    /// The running build, e.g. `v3.1.0 (f9a665f)`.
    pub running: Option<String>,
    /// The `info` read (for its error).
    pub info: Health,
    /// The last update check, if any.
    pub update: Option<UpdateState>,
    pub update_health: Health,
}

#[derive(Debug, Clone)]
pub struct UpdateState {
    pub status: update::Status,
    /// The latest release's tag.
    pub latest: String,
    /// How long ago GitHub was asked.
    pub age: Duration,
    pub from_cache: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub at: SystemTime,
    pub level: LogLevel,
    /// With a `warning: `/`error: ` prefix, as [`super::log_pane::text`]
    /// writes it.
    pub text: String,
}

impl Keyboard {
    /// The active slot, if the firmware marked one.
    pub fn active(&self) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.active)
    }
}

fn kind(s: &str) -> Option<TransportKind> {
    match s {
        "usb" => Some(TransportKind::Usb),
        "ble" => Some(TransportKind::Ble),
        _ => None,
    }
}

/// `v3.1.0 (f9a665f)` when the update check mapped the build to a release
/// its version string doesn't name (a CI build reports a bare SHA), else
/// the version as the firmware reports it.
fn running(s: &session::Snapshot) -> Option<String> {
    let version = s.build().and_then(|b| b.zmk_config_version.clone());
    let release = s
        .update
        .value
        .as_ref()
        .and_then(|u| u.check.running.release.as_ref());
    match (version, release) {
        (Some(v), Some(r)) if !v.starts_with(&r.tag) => Some(format!("{} ({v})", r.label())),
        (Some(v), _) => Some(v),
        (None, Some(r)) => Some(r.label()),
        (None, None) => None,
    }
}

impl Snapshot {
    /// The view of `s` at `now`; `link_at` is when the link state last
    /// changed (for the retry countdown) and `extra` the `log` crate's lines.
    pub fn derive(
        s: &session::Snapshot,
        link_at: SystemTime,
        now: SystemTime,
        extra: &[LogLine],
    ) -> Self {
        let link = match &s.link {
            LinkState::Lost { reason, retry_in } => LinkState::Lost {
                reason: reason.clone(),
                retry_in: retry_in
                    .saturating_sub(now.duration_since(link_at).unwrap_or(Duration::ZERO)),
            },
            l => l.clone(),
        };
        let via = match &s.link {
            LinkState::Connected { via } => Some(via),
            _ => None,
        };
        let profiles = s.status.value.as_ref();
        let keyboard = (via.is_some() || profiles.is_some()).then(|| {
            let priority = profiles.and_then(|p| p.output_priority);
            let active = profiles.and_then(|p| p.active());
            let output = via
                .and_then(|v| status::effective(&v.transport, priority, active))
                .and_then(kind);
            let over_ble = via.is_some_and(|v| v.transport.kind == TransportKind::Ble);
            let right_gone = s.split.value.is_some_and(|sp| sp.disconnected());
            let level = |b: Option<&Reading>, left: bool| {
                b.and_then(|r| if left { r.left } else { r.right }.percent)
            };
            Keyboard {
                name: s
                    .device()
                    .map_or_else(|| "Eyelash Sofle".into(), |d| d.name.clone()),
                transport: via.map(|v| v.transport.clone()),
                auto: via.is_some_and(|v| v.auto),
                output,
                output_priority: priority,
                usb_left_only: over_ble
                    && priority == Some("usb")
                    && output == Some(TransportKind::Ble),
                studio_locked: s.info.value.as_ref().map(|i| i.studio.locked),
                profiles: profiles.map(|p| p.profiles.clone()).unwrap_or_default(),
                status: Health::of(&s.status, now),
                battery: Battery {
                    left: level(s.battery.value.as_ref(), true),
                    right: level(s.battery.value.as_ref(), false).filter(|_| !right_gone),
                    right_disconnected: right_gone,
                    right_settling: !right_gone && s.right_settling(now),
                    health: Health::of(&s.battery, now),
                },
                firmware: Firmware {
                    running: running(s),
                    info: Health::of(&s.info, now),
                    update: s.update.value.as_ref().map(|u| UpdateState {
                        status: u.check.status,
                        latest: u.check.latest.tag.clone(),
                        age: now.duration_since(u.checked_at).unwrap_or(Duration::ZERO),
                        from_cache: u.from_cache,
                    }),
                    update_health: Health::of(&s.update, now),
                },
                split: s.split.value,
                split_health: Health::of(&s.split, now),
            }
        });
        let mut log: Vec<LogLine> = s
            .log
            .iter()
            .map(|l| LogLine {
                at: l.at,
                level: l.level,
                text: super::log_pane::text(l.level, &format_args!("{}", l.msg)),
            })
            .chain(extra.iter().cloned())
            .collect();
        // Stable: lines from one source keep their order on a tie.
        log.sort_by_key(|l| l.at);
        Self {
            link,
            keyboard,
            log,
        }
    }
}

/// A connected session with every pane filled, as `now` sees it: the
/// fixture `SOFLEII_TUI_DEMO=1` shows and the render tests start from.
pub fn demo(now: SystemTime) -> session::Snapshot {
    let ago = |s: u64| now - Duration::from_secs(s);
    let slot = |index: u32, name: Option<&str>, address: Option<&str>| Profile {
        index,
        active: index == 0,
        connected: index == 0,
        bonded: address.is_some(),
        address: address.map(str::to_string),
        name: name.map(str::to_string),
    };
    let mut s = session::Snapshot::new(&session::Config::default());
    let events = [
        Event::Log {
            at: ago(14),
            level: LogLevel::Info,
            msg: "connected over usb /dev/ttyACM1 (auto)".into(),
        },
        Event::Link(LinkState::Connected {
            via: ConnectedVia {
                transport: TransportInfo {
                    kind: TransportKind::Usb,
                    device: "/dev/ttyACM1".into(),
                },
                auto: true,
            },
        }),
        Event::Info(Stamped {
            at: ago(14),
            value: Ok(Info {
                device: Device {
                    name: "Eyelash Sofle".into(),
                    serial_number: Some("0011223344556677".into()),
                },
                build: Build {
                    available: true,
                    zmk_config_version: Some("f9a665f".into()),
                    zmk_config_dirty: Some(false),
                    ..Build::default()
                },
                hardware: Hardware::default(),
                studio: Studio { locked: false },
                custom_subsystems: Vec::new(),
            }),
        }),
        Event::Status(Stamped {
            at: ago(2),
            value: Ok(Profiles {
                max_profiles: 5,
                active_profile: Some(0),
                output_priority: Some("usb"),
                profiles: vec![
                    slot(0, Some("desk"), Some("00:11:22:33:44:55")),
                    slot(1, Some("laptop"), Some("00:11:22:33:44:66")),
                    slot(2, None, None),
                    slot(3, None, None),
                    slot(4, None, None),
                ],
            }),
        }),
        Event::Split(Stamped {
            at: ago(2),
            value: Ok(Split {
                known: true,
                connected: Some(true),
                is_split: Some(true),
                is_central: Some(true),
            }),
        }),
        Event::Battery(Stamped {
            at: ago(12),
            value: Ok(Reading {
                source: "bluez",
                device: BtDevice {
                    name: "Eyelash Sofle".into(),
                    address: "00:11:22:33:44:55".into(),
                },
                left: BatteryLevel {
                    percent: Some(82),
                    state: None,
                },
                right: BatteryLevel {
                    percent: Some(61),
                    state: None,
                },
                note: None,
                warnings: Vec::new(),
            }),
        }),
        Event::Update(Stamped {
            at: ago(12),
            value: Ok(UpdateReport {
                check: Check {
                    running: Running {
                        version: Some("f9a665f".into()),
                        commit: Some("f9a665f".into()),
                        dirty: false,
                        release: Some(Release {
                            tag: "v3.1.0".into(),
                            exact: true,
                            commits_after: 0,
                        }),
                    },
                    latest: Latest {
                        tag: "v3.1.0".into(),
                        commit: "f9a665f0c1d2e3b4a5968778695a4b3c2d1e0f9a".into(),
                        published_at: None,
                    },
                    status: update::Status::UpToDate,
                    commits_behind: Some(0),
                    commits_ahead: Some(0),
                    note: None,
                },
                checked_at: ago(120),
                from_cache: true,
            }),
        }),
    ];
    for e in &events {
        s.apply(e);
    }
    s.tick(now);
    s
}
