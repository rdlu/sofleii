//! The flash job (#100): a plan of steps (optional settings reset, then
//! the firmware, per half), each one waiting for a bootloader drive,
//! deciding whether this chip is the half it's for, writing, and learning
//! the chip. The CLI and the TUI drive it through [`Ui`]: events out,
//! the two questions in (flash it? / type the half to adopt it). [`Drives`]
//! is the system side, so the whole job runs against fakes in tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::{Bootloader, drive, read_drive};
use crate::error::{Error, Result, exit};
use crate::flash::{
    self, Basis, Bundle, Chip, Chips, Decision, Half, Identity, Image, Learned, Role, Uf2File,
    decide, identify::KEYBOARD, source::role_name,
};

/// One double-tap: what to write onto which half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Step {
    pub half: Half,
    pub role: Role,
}

/// The steps for `halves` (left first, whatever the order given), each
/// preceded by settings_reset when `reset`.
pub fn plan(halves: &[Half], reset: bool) -> Vec<Step> {
    let mut hs = halves.to_vec();
    hs.sort();
    hs.dedup();
    let mut steps = Vec::new();
    for h in hs {
        if reset {
            steps.push(Step {
                half: h,
                role: Role::SettingsReset,
            });
        }
        steps.push(Step {
            half: h,
            role: h.into(),
        });
    }
    steps
}

/// The system side of a flash.
pub trait Drives {
    /// Mounted bootloader drives.
    fn drives(&self) -> Result<Vec<PathBuf>>;
    /// A bootloader on USB whose disk never appeared.
    fn without_disk(&self) -> bool;
    fn serial(&self, mount: &Path) -> Option<String>;
    fn read(&self, mount: &Path) -> Result<(Bootloader, Image)>;
    fn write(
        &mut self,
        mount: &Path,
        name: &str,
        bytes: &[u8],
        progress: &mut dyn FnMut(usize),
    ) -> Result<()>;
    fn sleep(&mut self, d: Duration);
}

impl Drives for drive::Sys {
    fn drives(&self) -> Result<Vec<PathBuf>> {
        drive::Sys::drives(self)
    }
    fn without_disk(&self) -> bool {
        self.bootloader_without_disk()
    }
    fn serial(&self, mount: &Path) -> Option<String> {
        drive::Sys::serial(self, mount)
    }
    fn read(&self, mount: &Path) -> Result<(Bootloader, Image)> {
        read_drive(mount)
    }
    fn write(
        &mut self,
        mount: &Path,
        name: &str,
        bytes: &[u8],
        progress: &mut dyn FnMut(usize),
    ) -> Result<()> {
        drive::write_uf2(mount, name, bytes, progress)
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// What happens, as it happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum Event {
    /// Step `index` (0-based) of `total` begins: the front end tells the
    /// user to double-tap reset on `step.half`.
    Step {
        index: usize,
        total: usize,
        step: Step,
        file: String,
    },
    /// A bootloader is on USB but its disk didn't appear (no `usb_storage`
    /// for the running kernel; reboot). Sent once per wait.
    NoDisk,
    DriveFound {
        mount: PathBuf,
        serial: Option<String>,
        bootloader: Bootloader,
    },
    Identified {
        identity: Identity,
        chip: Option<Chip>,
        decision: Decision,
    },
    Writing {
        written: usize,
        total: usize,
    },
    /// The chip memory learned (or refreshed) this chip.
    Learned {
        serial: String,
        half: Half,
        learned: Learned,
    },
    /// The drive went away (the half rebooted), or didn't within the wait.
    Rebooted {
        gone: bool,
    },
    /// Saving the chip memory failed; the flash itself went on.
    Note {
        message: String,
    },
}

/// What the front end is asked about.
#[derive(Debug, Clone, Copy)]
pub struct Prompt<'a> {
    pub step: Step,
    pub mount: &'a Path,
    pub serial: Option<&'a str>,
    pub identity: &'a Identity,
    pub chip: Option<&'a Chip>,
    pub decision: &'a Decision,
    pub file: &'a Uf2File,
}

pub trait Ui {
    fn event(&mut self, e: &Event);
    /// The decision is to flash: go ahead? (`y` in the TUI; the CLI says
    /// yes unless asked to confirm.)
    fn confirm(&mut self, p: &Prompt<'_>) -> bool;
    /// Nothing vouches for the chip ([`Decision::Adopt`]), or the evidence
    /// says it isn't the target ([`Decision::Refuse`]): the half the user
    /// types, or `None` to stop. Only the step's own half adopts.
    fn adopt(&mut self, p: &Prompt<'_>) -> Option<Half>;
}

#[derive(Debug, Clone)]
pub struct Options {
    /// How long to wait for the drive after asking for the double-tap.
    pub wait: Duration,
    /// How long to wait for the drive to go after writing.
    pub reboot_wait: Duration,
    pub poll: Duration,
    /// A half adopted up front (CLI `--adopt left`), applied to steps for
    /// that half.
    pub adopt: Option<Half>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            wait: Duration::from_secs(60),
            reboot_wait: Duration::from_secs(30),
            poll: Duration::from_millis(500),
            adopt: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum Outcome {
    Flashed {
        basis: Basis,
    },
    Refused {
        reason: String,
    },
    /// The user said no (or typed no half).
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepReport {
    pub step: Step,
    pub file: String,
    pub mount: PathBuf,
    pub serial: Option<String>,
    pub identity: Identity,
    pub decision: Decision,
    #[serde(flatten)]
    pub outcome: Outcome,
    /// The drive went away after the write; `null` when nothing was written.
    pub rebooted: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub origin: flash::Origin,
    /// The steps that ran, in order; a refused or cancelled step is the
    /// last one.
    pub steps: Vec<StepReport>,
    /// How many steps were planned.
    pub planned: usize,
}

impl Report {
    pub fn complete(&self) -> bool {
        self.steps.len() == self.planned
            && self
                .steps
                .iter()
                .all(|s| matches!(s.outcome, Outcome::Flashed { .. }))
    }

    /// 0 when every planned step flashed, 1 when one was refused or
    /// cancelled.
    pub fn exit_code(&self) -> u8 {
        if self.complete() {
            exit::OK
        } else {
            exit::CHECK_FAILED
        }
    }
}

/// The chip memory and where to persist it.
pub struct Memory<'a> {
    pub chips: &'a mut Chips,
    pub save: &'a mut dyn FnMut(&Chips) -> Result<()>,
    /// ISO 8601 UTC, for `first_seen`/`last_flashed`.
    pub now: &'a dyn Fn() -> String,
}

/// Runs `steps` with the firmware in `bundle`. Errors (no drive, several
/// drives, I/O) end the job; a refusal or a "no" ends it with a report.
pub fn run(
    steps: &[Step],
    bundle: &Bundle,
    memory: &mut Memory<'_>,
    drives: &mut dyn Drives,
    ui: &mut dyn Ui,
    opts: &Options,
) -> Result<Report> {
    // Every file first: never stop halfway for a missing one.
    let files = steps
        .iter()
        .map(|s| {
            bundle.get(s.role).ok_or_else(|| {
                Error::Usage(format!(
                    "the firmware source has no {} file",
                    role_name(s.role)
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut report = Report {
        origin: bundle.origin.clone(),
        steps: Vec::new(),
        planned: steps.len(),
    };
    for (index, (&step, file)) in steps.iter().zip(files).enumerate() {
        ui.event(&Event::Step {
            index,
            total: steps.len(),
            step,
            file: file.name.clone(),
        });
        let r = run_step(step, file, memory, drives, ui, opts)?;
        let done = matches!(r.outcome, Outcome::Flashed { .. });
        report.steps.push(r);
        if !done {
            break;
        }
    }
    Ok(report)
}

fn wait_for_drive(drives: &mut dyn Drives, ui: &mut dyn Ui, opts: &Options) -> Result<PathBuf> {
    let mut waited = Duration::ZERO;
    let mut warned = false;
    loop {
        let found = drives.drives()?;
        match found.len() {
            1 => return Ok(found.into_iter().next().expect("one")),
            0 => {}
            n => {
                return Err(Error::Usage(format!(
                    "{n} bootloader drives are mounted ({}); leave only the half to flash \
                     in bootloader mode",
                    found
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
        let no_disk = drives.without_disk();
        if no_disk && !warned {
            ui.event(&Event::NoDisk);
            warned = true;
        }
        if waited >= opts.wait {
            let hint = if no_disk {
                " (a nice!nano bootloader is on USB, but its disk never appeared: \
                 usb_storage can't load, usually after a kernel upgrade without a reboot)"
            } else {
                " (double-tap reset on the half; a charge-only cable shows nothing)"
            };
            return Err(Error::NotFound(format!(
                "no bootloader drive appeared within {} s{hint}",
                opts.wait.as_secs()
            )));
        }
        drives.sleep(opts.poll);
        waited += opts.poll;
    }
}

fn run_step(
    step: Step,
    file: &Uf2File,
    memory: &mut Memory<'_>,
    drives: &mut dyn Drives,
    ui: &mut dyn Ui,
    opts: &Options,
) -> Result<StepReport> {
    let mount = wait_for_drive(drives, ui, opts)?;
    let serial = drives.serial(&mount);
    let (bootloader, image) = drives.read(&mount)?;
    ui.event(&Event::DriveFound {
        mount: mount.clone(),
        serial: serial.clone(),
        bootloader,
    });
    let identity = flash::identify(&image);
    let chip = serial.as_deref().and_then(|s| memory.chips.get(s)).cloned();
    let adopt = opts.adopt.filter(|&h| h == step.half);
    let mut decision = decide(step.half, chip.as_ref(), &identity, adopt);
    ui.event(&Event::Identified {
        identity: identity.clone(),
        chip: chip.clone(),
        decision: decision.clone(),
    });
    let report = |decision: Decision, outcome, rebooted| StepReport {
        step,
        file: file.name.clone(),
        mount: mount.clone(),
        serial: serial.clone(),
        identity: identity.clone(),
        decision,
        outcome,
        rebooted,
    };
    // A closure can't tie its returned borrow to its argument's lifetime.
    macro_rules! prompt {
        ($d:expr) => {
            Prompt {
                step,
                mount: &mount,
                serial: serial.as_deref(),
                identity: &identity,
                chip: chip.as_ref(),
                decision: $d,
                file,
            }
        };
    }
    let basis = match &decision {
        Decision::Flash { basis, .. } => {
            if !ui.confirm(&prompt!(&decision)) {
                return Ok(report(decision, Outcome::Cancelled, None));
            }
            *basis
        }
        Decision::Adopt { .. } | Decision::Refuse { .. } => match ui.adopt(&prompt!(&decision)) {
            Some(h) if h == step.half => {
                decision = decide(step.half, chip.as_ref(), &identity, Some(h));
                ui.event(&Event::Identified {
                    identity: identity.clone(),
                    chip: chip.clone(),
                    decision: decision.clone(),
                });
                Basis::Adopted
            }
            _ => {
                let outcome = match &decision {
                    Decision::Refuse { reason } => Outcome::Refused {
                        reason: reason.clone(),
                    },
                    _ => Outcome::Cancelled,
                };
                return Ok(report(decision, outcome, None));
            }
        },
    };

    let total = file.bytes.len();
    drives.write(&mount, &file.name, &file.bytes, &mut |written| {
        ui.event(&Event::Writing { written, total })
    })?;

    if let Some(s) = &serial {
        let learned = match basis {
            Basis::Adopted => Learned::Adopted,
            Basis::Chip => chip.as_ref().map_or(Learned::Verified, |c| c.learned),
            Basis::Firmware => Learned::Verified,
        };
        let now = (memory.now)();
        memory
            .chips
            .learn(s, KEYBOARD, step.half, learned, &now, true);
        ui.event(&Event::Learned {
            serial: s.clone(),
            half: step.half,
            learned,
        });
        if let Err(e) = (memory.save)(memory.chips) {
            ui.event(&Event::Note {
                message: format!("couldn't save the chip memory: {e}"),
            });
        }
    }

    let mut waited = Duration::ZERO;
    let gone = loop {
        if !drives.drives()?.contains(&mount) {
            break true;
        }
        if waited >= opts.reboot_wait {
            break false;
        }
        drives.sleep(opts.poll);
        waited += opts.poll;
    };
    ui.event(&Event::Rebooted { gone });
    Ok(report(decision, Outcome::Flashed { basis }, Some(gone)))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::flash::source::tests::{firmware, release_files, settings_reset};
    use crate::flash::{Origin, uf2};

    /// One chip in bootloader mode: what `CURRENT.UF2` holds and its serial.
    #[derive(Clone)]
    struct FakeChip {
        serial: Option<&'static str>,
        image: Image,
    }

    fn chip(serial: Option<&'static str>, uf2_bytes: &[u8]) -> FakeChip {
        FakeChip {
            serial,
            image: uf2::parse(uf2_bytes).unwrap(),
        }
    }

    const LEFT: &str = "94708422E5F2FD3E";
    const RIGHT: &str = "93B0CC1C158978F8";

    /// Chips appear one per step, after `delay` polls; a write replaces
    /// the chip's image (over it, like the bootloader) and unmounts it.
    struct Fake {
        queue: VecDeque<FakeChip>,
        present: Option<FakeChip>,
        delay: usize,
        polls: usize,
        no_disk: bool,
        two_drives: bool,
        written: Vec<(&'static str, String, usize)>,
    }

    impl Fake {
        fn new(chips: Vec<FakeChip>) -> Self {
            Self {
                queue: chips.into(),
                present: None,
                delay: 2,
                polls: 0,
                no_disk: false,
                two_drives: false,
                written: Vec::new(),
            }
        }
    }

    const MNT: &str = "/run/media/me/NICENANO";

    impl Drives for Fake {
        fn drives(&self) -> Result<Vec<PathBuf>> {
            let mut v: Vec<PathBuf> = self.present.iter().map(|_| PathBuf::from(MNT)).collect();
            if self.two_drives {
                v = vec![PathBuf::from(MNT), PathBuf::from("/run/media/me/NICENANO1")];
            }
            Ok(v)
        }
        fn without_disk(&self) -> bool {
            self.no_disk
        }
        fn serial(&self, _: &Path) -> Option<String> {
            self.present.as_ref()?.serial.map(String::from)
        }
        fn read(&self, _: &Path) -> Result<(Bootloader, Image)> {
            Ok((
                Bootloader::default(),
                self.present.as_ref().unwrap().image.clone(),
            ))
        }
        fn write(
            &mut self,
            _: &Path,
            name: &str,
            bytes: &[u8],
            p: &mut dyn FnMut(usize),
        ) -> Result<()> {
            p(bytes.len());
            let c = self.present.take().unwrap();
            self.written
                .push((c.serial.unwrap_or("?"), name.into(), bytes.len()));
            Ok(())
        }
        fn sleep(&mut self, _: Duration) {
            self.polls += 1;
            if self.present.is_none() && self.polls >= self.delay && !self.no_disk {
                self.present = self.queue.pop_front();
                self.polls = 0;
            }
        }
    }

    /// Answers from a script: confirm yes/no, the typed half.
    struct Script {
        confirm: bool,
        adopt: Option<Half>,
        events: Vec<Event>,
        asked_adopt: usize,
    }

    impl Script {
        fn yes() -> Self {
            Self {
                confirm: true,
                adopt: None,
                events: Vec::new(),
                asked_adopt: 0,
            }
        }
    }

    impl Ui for Script {
        fn event(&mut self, e: &Event) {
            if !matches!(e, Event::Writing { .. }) {
                self.events.push(e.clone());
            }
        }
        fn confirm(&mut self, _: &Prompt<'_>) -> bool {
            self.confirm
        }
        fn adopt(&mut self, _: &Prompt<'_>) -> Option<Half> {
            self.asked_adopt += 1;
            self.adopt
        }
    }

    fn bundle(record: bool) -> Bundle {
        Bundle::from_files(Origin::Files, release_files(record)).unwrap()
    }

    fn opts() -> Options {
        Options {
            wait: Duration::from_secs(5),
            reboot_wait: Duration::from_secs(1),
            poll: Duration::from_millis(500),
            adopt: None,
        }
    }

    struct Run {
        report: Result<Report>,
        chips: Chips,
        saves: usize,
        fake: Fake,
        ui: Script,
    }

    fn go(steps: &[Step], chips: Chips, fake: Fake, ui: Script, opts: Options) -> Run {
        let mut chips = chips;
        let mut saves = 0;
        let mut fake = fake;
        let mut ui = ui;
        let report = {
            let mut save = |_: &Chips| {
                saves += 1;
                Ok(())
            };
            let mut memory = Memory {
                chips: &mut chips,
                save: &mut save,
                now: &|| "2026-10-24T23:00:00Z".to_string(),
            };
            run(steps, &bundle(true), &mut memory, &mut fake, &mut ui, &opts)
        };
        Run {
            report,
            chips,
            saves,
            fake,
            ui,
        }
    }

    #[test]
    fn plans() {
        use Half::*;
        assert_eq!(
            plan(&[Right, Left, Left], true),
            [
                Step {
                    half: Left,
                    role: Role::SettingsReset
                },
                Step {
                    half: Left,
                    role: Role::Left
                },
                Step {
                    half: Right,
                    role: Role::SettingsReset
                },
                Step {
                    half: Right,
                    role: Role::Right
                },
            ]
        );
        assert_eq!(
            plan(&[Right], false),
            [Step {
                half: Right,
                role: Role::Right
            }]
        );
    }

    #[test]
    fn both_halves_by_their_firmware_learn_the_serials() {
        let fake = Fake::new(vec![
            chip(Some(LEFT), &firmware(Half::Left, false)),
            chip(Some(RIGHT), &firmware(Half::Right, false)),
        ]);
        let r = go(
            &plan(&[Half::Left, Half::Right], false),
            Chips::default(),
            fake,
            Script::yes(),
            opts(),
        );
        let report = r.report.unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(report.exit_code(), exit::OK);
        assert_eq!(
            report
                .steps
                .iter()
                .map(|s| s.outcome.clone())
                .collect::<Vec<_>>(),
            [
                Outcome::Flashed {
                    basis: Basis::Firmware
                },
                Outcome::Flashed {
                    basis: Basis::Firmware
                }
            ]
        );
        assert_eq!(report.steps[0].rebooted, Some(true));
        assert_eq!(
            r.fake
                .written
                .iter()
                .map(|(s, n, _)| (*s, n.as_str()))
                .collect::<Vec<_>>(),
            [
                (LEFT, "eyelash_sofle_studio_left.uf2"),
                (RIGHT, "nice_view-eyelash_sofle_right-zmk.uf2")
            ]
        );
        assert_eq!(r.chips.get(LEFT).unwrap().half, Half::Left);
        assert_eq!(r.chips.get(RIGHT).unwrap().learned, Learned::Verified);
        assert_eq!(r.saves, 2);
    }

    #[test]
    fn the_wrong_half_is_refused_and_nothing_written() {
        let fake = Fake::new(vec![chip(Some(RIGHT), &firmware(Half::Right, true))]);
        let r = go(
            &plan(&[Half::Left], false),
            Chips::default(),
            fake,
            Script::yes(),
            opts(),
        );
        let report = r.report.unwrap();
        assert!(
            matches!(&report.steps[0].outcome, Outcome::Refused { reason } if reason.contains("right half"))
        );
        assert_eq!(report.exit_code(), exit::CHECK_FAILED);
        assert!(r.fake.written.is_empty());
        assert_eq!(r.ui.asked_adopt, 1, "the front end may offer adoption");
        assert!(r.chips.chips.is_empty());
    }

    #[test]
    fn a_known_chip_with_the_wrong_firmware_is_recovered() {
        let mut known = Chips::default();
        known.learn(LEFT, KEYBOARD, Half::Left, Learned::Adopted, "t0", false);
        // Right-half firmware ended up on the left chip.
        let fake = Fake::new(vec![chip(Some(LEFT), &firmware(Half::Right, true))]);
        let r = go(
            &plan(&[Half::Left], false),
            known,
            fake,
            Script::yes(),
            opts(),
        );
        let report = r.report.unwrap();
        assert_eq!(
            report.steps[0].outcome,
            Outcome::Flashed { basis: Basis::Chip }
        );
        let c = r.chips.get(LEFT).unwrap();
        assert_eq!(
            (c.learned, c.first_seen.as_str(), c.last_flashed.as_deref()),
            (Learned::Adopted, "t0", Some("2026-10-24T23:00:00Z"))
        );
    }

    #[test]
    fn an_unknown_chip_is_adopted_by_the_typed_half_only() {
        let blank = uf2::encode(0x1000, &[0xFF; 4096], 0x239a_00b3);
        let mut ui = Script::yes();
        ui.adopt = Some(Half::Right);
        let fake = Fake::new(vec![chip(Some(RIGHT), &blank)]);
        let r = go(
            &plan(&[Half::Left], false),
            Chips::default(),
            fake,
            ui,
            opts(),
        );
        assert_eq!(r.report.unwrap().steps[0].outcome, Outcome::Cancelled);
        assert!(r.fake.written.is_empty());

        let mut ui = Script::yes();
        ui.adopt = Some(Half::Right);
        let fake = Fake::new(vec![chip(Some(RIGHT), &blank)]);
        let r = go(
            &plan(&[Half::Right], false),
            Chips::default(),
            fake,
            ui,
            opts(),
        );
        let report = r.report.unwrap();
        assert_eq!(
            report.steps[0].outcome,
            Outcome::Flashed {
                basis: Basis::Adopted
            }
        );
        assert!(matches!(
            report.steps[0].decision,
            Decision::Flash {
                basis: Basis::Adopted,
                ..
            }
        ));
        assert_eq!(r.chips.get(RIGHT).unwrap().learned, Learned::Adopted);

        // Adopted up front (--adopt right): no question asked.
        let mut o = opts();
        o.adopt = Some(Half::Right);
        let fake = Fake::new(vec![chip(Some(RIGHT), &blank)]);
        let r = go(
            &plan(&[Half::Right], false),
            Chips::default(),
            fake,
            Script::yes(),
            o,
        );
        assert!(r.report.unwrap().complete());
        assert_eq!(r.ui.asked_adopt, 0);
    }

    #[test]
    fn reset_then_firmware_on_the_same_chip() {
        // The second double-tap finds settings_reset over the old left
        // firmware; the serial learned on the first step decides.
        let over = |older: Vec<u8>, newer: Vec<u8>| {
            let (o, n) = (uf2::parse(&older).unwrap(), uf2::parse(&newer).unwrap());
            let mut b = o.bytes.clone();
            b[..n.bytes.len()].copy_from_slice(&n.bytes);
            FakeChip {
                serial: Some(LEFT),
                image: Image {
                    base: o.base,
                    bytes: b,
                },
            }
        };
        let fake = Fake::new(vec![
            chip(Some(LEFT), &firmware(Half::Left, false)),
            over(firmware(Half::Left, false), settings_reset()),
        ]);
        let r = go(
            &plan(&[Half::Left], true),
            Chips::default(),
            fake,
            Script::yes(),
            opts(),
        );
        let report = r.report.unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(
            report.steps[1].outcome,
            Outcome::Flashed { basis: Basis::Chip }
        );
        assert_eq!(r.fake.written[0].1, "settings_reset.uf2");
    }

    #[test]
    fn no_and_errors() {
        let mut ui = Script::yes();
        ui.confirm = false;
        let fake = Fake::new(vec![chip(Some(LEFT), &firmware(Half::Left, true))]);
        let r = go(
            &plan(&[Half::Left], false),
            Chips::default(),
            fake,
            ui,
            opts(),
        );
        assert_eq!(r.report.unwrap().steps[0].outcome, Outcome::Cancelled);
        assert!(r.fake.written.is_empty());

        let mut fake = Fake::new(vec![]);
        fake.no_disk = true;
        let r = go(
            &plan(&[Half::Left], false),
            Chips::default(),
            fake,
            Script::yes(),
            opts(),
        );
        let e = r.report.unwrap_err();
        assert_eq!(e.exit_code(), exit::NOT_FOUND);
        assert!(e.to_string().contains("usb_storage"), "{e}");
        assert_eq!(
            r.ui.events.iter().filter(|e| **e == Event::NoDisk).count(),
            1
        );

        let mut fake = Fake::new(vec![chip(Some(LEFT), &firmware(Half::Left, true))]);
        fake.two_drives = true;
        let r = go(
            &plan(&[Half::Left], false),
            Chips::default(),
            fake,
            Script::yes(),
            opts(),
        );
        assert_eq!(r.report.unwrap_err().exit_code(), exit::USAGE);

        // A source without the file: refused before waiting for anything.
        let only_left = Bundle::from_files(
            Origin::Files,
            vec![("l.uf2".into(), firmware(Half::Left, true))],
        )
        .unwrap();
        let mut chips = Chips::default();
        let mut save = |_: &Chips| Ok(());
        let mut memory = Memory {
            chips: &mut chips,
            save: &mut save,
            now: &|| "t".into(),
        };
        let e = run(
            &plan(&[Half::Left, Half::Right], false),
            &only_left,
            &mut memory,
            &mut Fake::new(vec![]),
            &mut Script::yes(),
            &opts(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("no right-half file"), "{e}");
    }

    #[test]
    fn no_serial_still_flashes_but_learns_nothing() {
        let fake = Fake::new(vec![chip(None, &firmware(Half::Left, true))]);
        let r = go(
            &plan(&[Half::Left], false),
            Chips::default(),
            fake,
            Script::yes(),
            opts(),
        );
        assert!(r.report.unwrap().complete());
        assert!(r.chips.chips.is_empty());
        assert_eq!(r.saves, 0);
    }
}
