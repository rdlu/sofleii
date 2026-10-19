//! What the dashboard shows, as plain data: the view renders a
//! [`Snapshot`] and nothing else, so it is testable without a keyboard.
//! Nothing fills it from the keyboard yet (a later PR adds the session
//! thread); [`Snapshot::searching`] is the empty state and
//! [`Snapshot::demo`] a fixture for screenshots (`SOFLEII_TUI_DEMO=1`).

use std::time::Duration;

use crate::commands::profiles::Profile;
use crate::commands::status::Split;
use crate::commands::update;
use crate::transport::{TransportInfo, TransportKind};

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// `None` until a keyboard answers: the "Searching…" state.
    pub keyboard: Option<Keyboard>,
    /// The log pane, oldest first.
    pub log: Vec<LogLine>,
}

#[derive(Debug, Clone)]
pub struct Keyboard {
    /// The device name from `get_device_info`.
    pub name: String,
    pub transport: TransportInfo,
    /// Picked by `--transport auto` (vs. forced with a flag).
    pub auto: bool,
    /// Where keystrokes go right now.
    pub output: Option<TransportKind>,
    /// `"usb"` or `"ble"`, as in `status`.
    pub output_priority: Option<&'static str>,
    pub studio_locked: bool,
    /// Every slot, as `profiles` reports them.
    pub profiles: Vec<Profile>,
    pub battery: Battery,
    pub firmware: Firmware,
    /// `None` while the firmware can't report it (always, for now: see
    /// [`crate::commands::status::SPLIT_NOTE`]).
    pub split: Option<Split>,
}

#[derive(Debug, Clone, Default)]
pub struct Battery {
    pub left: Option<u8>,
    pub right: Option<u8>,
    /// How long ago the levels were read.
    pub age: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
pub struct Firmware {
    /// The running build, e.g. `v3.1.0 (f9a665f)`.
    pub running: Option<String>,
    /// The last `update check`, if any.
    pub update: Option<UpdateState>,
}

#[derive(Debug, Clone)]
pub struct UpdateState {
    pub status: update::Status,
    /// The latest release's tag.
    pub latest: String,
    /// How long ago it was checked.
    pub age: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Local time, `HH:MM:SS`.
    pub at: String,
    pub text: String,
}

impl Keyboard {
    /// The active slot, if the firmware marked one.
    pub fn active(&self) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.active)
    }
}

impl Snapshot {
    /// Nothing known yet.
    pub fn searching() -> Self {
        Self::default()
    }

    /// A fixture with every pane filled, for screenshots and render tests.
    pub fn demo() -> Self {
        let slot = |index: u32, name: Option<&str>, address: Option<&str>| Profile {
            index,
            active: index == 0,
            connected: index == 0,
            bonded: address.is_some(),
            address: address.map(str::to_string),
            name: name.map(str::to_string),
        };
        Self {
            keyboard: Some(Keyboard {
                name: "Eyelash Sofle".into(),
                transport: TransportInfo {
                    kind: TransportKind::Usb,
                    device: "/dev/ttyACM1".into(),
                },
                auto: true,
                output: Some(TransportKind::Usb),
                output_priority: Some("usb"),
                studio_locked: false,
                profiles: vec![
                    slot(0, Some("desk"), Some("DD:1F:23:76:6E:71")),
                    slot(1, Some("laptop"), Some("C8:3A:35:10:22:9E")),
                    slot(2, None, None),
                    slot(3, None, None),
                    slot(4, None, None),
                ],
                battery: Battery {
                    left: Some(82),
                    right: Some(61),
                    age: Some(Duration::from_secs(12)),
                },
                firmware: Firmware {
                    running: Some("v3.1.0 (f9a665f)".into()),
                    update: Some(UpdateState {
                        status: update::Status::UpToDate,
                        latest: "v3.1.0".into(),
                        age: Duration::from_secs(120),
                    }),
                },
                split: None,
            }),
            log: vec![
                LogLine {
                    at: "17:41:58".into(),
                    text: "connected over USB /dev/ttyACM1".into(),
                },
                LogLine {
                    at: "17:42:10".into(),
                    text: "USB went quiet (output BLE?), switched to BLE DD:1F:23:76:6E:71".into(),
                },
            ],
        }
    }
}
