//! Flash mode (#100): `F` opens a full-screen wizard over the dashboard.
//!
//! **Setup** picks the firmware source (the newest release by default, the
//! newest green CI build of a branch, or the files of a local build) and
//! the plan (settings reset first, left, right). **Running** hands both to
//! a worker thread that fetches the firmware and runs the same flash job
//! as the CLI ([`job::run`]); its [`Ui`](job::Ui) turns the job's events
//! into [`Msg`]s on the loop's channel and blocks on [`Answer`]s for the two
//! questions: flash this chip? (**only `y`** says yes, as in the TUI's other
//! confirmations) and, for a chip nothing vouches for, the typed half.
//! **Done** shows the outcome until `⏎`/`Esc`.
//!
//! While the left half is in the plan, the dashboard's link is suspended
//! ([`Request::Suspend`]): the half drops off USB/BLE in its bootloader,
//! and that's no reason to show a lost link. It resumes when the job ends,
//! and the resumed link's first info read is the left half's new version.
//!
//! The state is plain data, so the flow is tested without threads; the
//! worker is [`spawn`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime};

use ratatui::crossterm::event::KeyCode;

use crate::commands::flash::{chips, drive, job, source};
use crate::commands::update::DEFAULT_REPO;
use crate::error::Error;
use crate::flash::{Chips, Decision, Half, Learned, Role, identify::KEYBOARD};
use crate::output::{self, Style};
use crate::session::{ErrorView, LogLevel, Request};

use super::Input;

/// The local build's file names (`mise run build-*`), looked for in the
/// working directory and its parents.
pub const LOCAL_FILES: [&str; 3] = [
    "eyelash_sofle_left.uf2",
    "eyelash_sofle_right.uf2",
    "settings_reset.uf2",
];

/// Where the firmware comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Release,
    Ci,
    Local,
}

impl SourceKind {
    fn next(self) -> Self {
        match self {
            SourceKind::Release => SourceKind::Ci,
            SourceKind::Ci => SourceKind::Local,
            SourceKind::Local => SourceKind::Release,
        }
    }
}

/// Setup's rows, top to bottom; `Branch` only with the CI source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Source,
    Branch,
    Reset,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub source: SourceKind,
    pub branch: String,
    /// The branch field takes keystrokes.
    pub editing: bool,
    pub reset: bool,
    pub left: bool,
    pub right: bool,
    pub cursor: Row,
    /// `GH_TOKEN`/`GITHUB_TOKEN` is set (CI needs it).
    pub token: bool,
    /// The local build's files found, if any.
    pub local: Vec<PathBuf>,
    /// Why `⏎` did nothing, until the next key.
    pub problem: Option<String>,
}

impl Setup {
    pub fn new(token: bool, local: Vec<PathBuf>) -> Self {
        Self {
            source: SourceKind::Release,
            branch: "main".into(),
            editing: false,
            reset: false,
            left: true,
            right: true,
            cursor: Row::Source,
            token,
            local,
            problem: None,
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut r = vec![Row::Source];
        if self.source == SourceKind::Ci {
            r.push(Row::Branch);
        }
        r.extend([Row::Reset, Row::Left, Row::Right]);
        r
    }

    fn halves(&self) -> Vec<Half> {
        [(self.left, Half::Left), (self.right, Half::Right)]
            .into_iter()
            .filter_map(|(on, h)| on.then_some(h))
            .collect()
    }

    /// What to run, or why not.
    pub fn spec(&self) -> Result<Spec, String> {
        let halves = self.halves();
        if halves.is_empty() {
            return Err("pick a half to flash (space toggles)".into());
        }
        let source = match self.source {
            SourceKind::Release => source::Source::LatestRelease,
            SourceKind::Ci if !self.token => {
                return Err(
                    "CI builds need a GitHub token: start sofleii with GH_TOKEN=$(gh auth token)"
                        .into(),
                );
            }
            SourceKind::Ci if self.branch.trim().is_empty() => {
                return Err("type a branch (b)".into());
            }
            SourceKind::Ci => source::Source::Ci {
                branch: self.branch.trim().to_string(),
            },
            SourceKind::Local if self.local.is_empty() => {
                return Err(format!(
                    "no local build here: none of {} in this directory or above \
                     (mise run build-left / build-right / build-reset)",
                    LOCAL_FILES.join(", ")
                ));
            }
            SourceKind::Local => source::Source::Files(self.local.clone()),
        };
        Ok(Spec {
            source,
            steps: job::plan(&halves, self.reset),
        })
    }

    /// One key; `true` when it asks to start.
    fn on_key(&mut self, code: KeyCode) -> bool {
        self.problem = None;
        if self.editing {
            match code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Tab => self.editing = false,
                KeyCode::Backspace => {
                    self.branch.pop();
                }
                KeyCode::Char(c) if !c.is_whitespace() => self.branch.push(c),
                _ => {}
            }
            return false;
        }
        let rows = self.rows();
        let at = rows.iter().position(|r| *r == self.cursor).unwrap_or(0);
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.cursor = rows[at.saturating_sub(1)],
            KeyCode::Down | KeyCode::Char('j') => self.cursor = rows[(at + 1).min(rows.len() - 1)],
            KeyCode::Char('s') => self.source = self.source.next(),
            KeyCode::Char('b') if self.source == SourceKind::Ci => {
                self.cursor = Row::Branch;
                self.editing = true;
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => match self.cursor {
                Row::Source => self.source = self.source.next(),
                Row::Branch => self.editing = true,
                Row::Reset => self.reset = !self.reset,
                Row::Left => self.left = !self.left,
                Row::Right => self.right = !self.right,
            },
            KeyCode::Enter => return true,
            _ => {}
        }
        if !self.rows().contains(&self.cursor) {
            self.cursor = Row::Source;
        }
        false
    }
}

/// What the worker runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub source: source::Source,
    pub steps: Vec<job::Step>,
}

/// A question the job waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Flash `file` onto this chip? Only `y` says yes.
    Confirm { step: job::Step, file: String },
    /// Nothing vouches for the chip (or its firmware says the other half):
    /// type the half to adopt it.
    Adopt {
        step: job::Step,
        serial: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Confirm(bool),
    Adopt(Option<Half>),
}

/// The worker's news, on the loop's channel.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The firmware is fetched: its summary lines.
    Fetched(Vec<String>),
    Event(job::Event),
    Ask(Ask),
    Finished(Result<job::Report, ErrorView>),
}

/// The TUI's end of a running worker.
#[derive(Debug, Clone)]
pub struct Worker {
    pub answers: Sender<Answer>,
    pub cancel: Arc<AtomicBool>,
}

/// One step as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepState {
    Pending,
    /// Waiting for its drive since then.
    Waiting(SystemTime),
    /// Drive found; the lines say what's on it and what was decided.
    Active,
    Writing {
        written: usize,
        total: usize,
    },
    Flashed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepView {
    pub step: job::Step,
    pub state: StepState,
    /// The CLI's commentary for it (`drive …`, `on chip …`, `decision …`).
    pub lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Running {
    pub fetched: Vec<String>,
    pub steps: Vec<StepView>,
    pub current: Option<usize>,
    pub ask: Option<Ask>,
    /// The typed half, while [`Ask::Adopt`] is up.
    pub typed: String,
    /// The link was suspended for this run.
    pub suspended: bool,
    pub cancelling: bool,
    pub worker: Worker,
}

impl Running {
    /// A write is in progress: the half must not be interrupted.
    pub fn writing(&self) -> bool {
        self.current.is_some_and(|i| {
            matches!(
                self.steps[i].state,
                StepState::Writing { written, total } if written < total
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct Done {
    pub steps: Vec<StepView>,
    pub result: Result<job::Report, ErrorView>,
    pub at: SystemTime,
}

#[derive(Debug, Clone)]
pub enum Stage {
    Setup(Setup),
    Running(Box<Running>),
    Done(Box<Done>),
}

/// What a key or a message asks the [`super::App`] to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Close,
    /// Spawn the worker for this.
    Start(Spec),
    Send(Request),
    /// A line for the Log pane.
    Log(LogLevel, String),
}

#[derive(Debug, Clone)]
pub struct Mode {
    pub stage: Stage,
}

impl Mode {
    pub fn new(setup: Setup) -> Self {
        Self {
            stage: Stage::Setup(setup),
        }
    }

    /// The worker started for `spec`; `suspend` when the left half is in it.
    pub fn started(&mut self, spec: &Spec, worker: Worker) -> bool {
        let suspend = spec.steps.iter().any(|s| s.half == Half::Left);
        self.stage = Stage::Running(Box::new(Running {
            fetched: Vec::new(),
            steps: spec
                .steps
                .iter()
                .map(|&step| StepView {
                    step,
                    state: StepState::Pending,
                    lines: Vec::new(),
                })
                .collect(),
            current: None,
            ask: None,
            typed: String::new(),
            suspended: suspend,
            cancelling: false,
            worker,
        }));
        suspend
    }

    /// `Esc` would leave Flash mode now (not mid-run).
    pub fn closable(&self) -> bool {
        !matches!(self.stage, Stage::Running(_))
    }

    pub fn on_key(&mut self, code: KeyCode) -> Effect {
        match &mut self.stage {
            Stage::Setup(s) => {
                if code == KeyCode::Esc && !s.editing {
                    return Effect::Close;
                }
                if s.on_key(code) {
                    match s.spec() {
                        Ok(spec) => return Effect::Start(spec),
                        Err(why) => s.problem = Some(why),
                    }
                }
                Effect::None
            }
            Stage::Running(r) => r.on_key(code),
            Stage::Done(_) => match code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => Effect::Close,
                _ => Effect::None,
            },
        }
    }

    /// One worker message; the effects for the app.
    pub fn apply(&mut self, msg: Msg, now: SystemTime) -> Vec<Effect> {
        let Stage::Running(r) = &mut self.stage else {
            return Vec::new();
        };
        let mut fx = Vec::new();
        match msg {
            Msg::Fetched(lines) => {
                for l in &lines {
                    fx.push(Effect::Log(LogLevel::Info, format!("flash: {}", l.trim())));
                }
                r.fetched = lines;
            }
            Msg::Ask(a) => r.ask = Some(a),
            Msg::Event(e) => fx.extend(r.on_event(&e, now)),
            Msg::Finished(result) => {
                if let Some(i) = r.current {
                    let s = &mut r.steps[i];
                    if !matches!(s.state, StepState::Flashed) {
                        s.state = StepState::Stopped;
                    }
                }
                match &result {
                    Ok(rep) if rep.complete() => {
                        fx.push(Effect::Log(LogLevel::Info, "flash: done".into()));
                    }
                    Ok(_) => fx.push(Effect::Log(LogLevel::Warn, "flash: stopped".into())),
                    Err(e) => fx.push(Effect::Log(
                        LogLevel::Error,
                        format!("flash: {}", e.message),
                    )),
                }
                if r.suspended {
                    fx.push(Effect::Send(Request::Suspend(false)));
                }
                self.stage = Stage::Done(Box::new(Done {
                    steps: std::mem::take(&mut r.steps),
                    result,
                    at: now,
                }));
            }
        }
        fx
    }
}

impl Running {
    fn on_key(&mut self, code: KeyCode) -> Effect {
        if let Some(ask) = self.ask.clone() {
            return self.answer(&ask, code);
        }
        match code {
            KeyCode::Esc | KeyCode::Char('q') if self.writing() => Effect::Log(
                LogLevel::Warn,
                "writing to the half: wait for the copy to finish".into(),
            ),
            KeyCode::Esc => {
                self.cancelling = true;
                self.worker.cancel.store(true, Ordering::SeqCst);
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn answer(&mut self, ask: &Ask, code: KeyCode) -> Effect {
        let reply = match ask {
            Ask::Confirm { .. } => Answer::Confirm(matches!(code, KeyCode::Char('y' | 'Y'))),
            Ask::Adopt { .. } => match code {
                KeyCode::Enter => Answer::Adopt(Half::parse(&self.typed.trim().to_lowercase())),
                KeyCode::Esc => Answer::Adopt(None),
                KeyCode::Backspace => {
                    self.typed.pop();
                    return Effect::None;
                }
                KeyCode::Char(c) => {
                    self.typed.push(c);
                    return Effect::None;
                }
                _ => return Effect::None,
            },
        };
        self.ask = None;
        self.typed.clear();
        let _ = self.worker.answers.send(reply);
        Effect::None
    }

    fn line(&mut self, text: String) {
        if let Some(i) = self.current {
            self.steps[i].lines.extend(text.lines().map(String::from));
        }
    }

    fn on_event(&mut self, e: &job::Event, now: SystemTime) -> Vec<Effect> {
        use job::Event;
        let plain = Style::plain(output::Format::Human);
        let mut fx = Vec::new();
        match e {
            Event::Step { index, .. } => {
                if let Some(prev) = self.current
                    && prev != *index
                    && !matches!(self.steps[prev].state, StepState::Stopped)
                {
                    self.steps[prev].state = StepState::Flashed;
                }
                self.current = Some(*index);
                if let Some(s) = self.steps.get_mut(*index) {
                    s.state = StepState::Waiting(now);
                    fx.push(Effect::Log(
                        LogLevel::Info,
                        format!(
                            "flash: double-tap reset on the {} half",
                            s.step.half.as_str()
                        ),
                    ));
                }
            }
            Event::Writing { written, total } => {
                if let Some(i) = self.current {
                    self.steps[i].state = StepState::Writing {
                        written: *written,
                        total: *total,
                    };
                }
            }
            Event::Rebooted { gone } => {
                if let Some(i) = self.current {
                    self.steps[i].state = StepState::Flashed;
                }
                if let Some(l) = output::flash_event(plain, e) {
                    self.line(l);
                }
                if !gone {
                    fx.push(Effect::Log(
                        LogLevel::Warn,
                        "flash: the drive is still mounted after the copy".into(),
                    ));
                }
            }
            e => {
                if let Some(i) = self.current
                    && matches!(e, Event::DriveFound { .. })
                {
                    self.steps[i].state = StepState::Active;
                }
                if let Some(l) = output::flash_event(plain, e) {
                    for one in l.lines() {
                        fx.push(Effect::Log(
                            LogLevel::Info,
                            format!("flash: {}", squeeze(one)),
                        ));
                    }
                    self.line(l);
                }
            }
        }
        fx
    }
}

/// `drive      /run/…` → `drive /run/…`, for the log.
fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The local build's files in the working directory or the nearest parent
/// that has any.
pub fn find_local(start: &Path) -> Vec<PathBuf> {
    for dir in start.ancestors() {
        let found: Vec<PathBuf> = LOCAL_FILES
            .iter()
            .map(|n| dir.join(n))
            .filter(|p| p.is_file())
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// Learns the left chip from Studio's serial number (only the central half
/// runs Studio), unless the chip is already known. `true` when learned.
pub fn learn_from_studio(chips: &mut Chips, serial: &str, now: &str) -> bool {
    if chips.get(serial).is_some() {
        return false;
    }
    chips.learn(serial, KEYBOARD, Half::Left, Learned::Studio, now, false)
}

/// The job's [`job::Ui`] on the worker thread: events out, answers in.
struct ChannelUi {
    tx: Sender<Input>,
    answers: Receiver<Answer>,
    cancel: Arc<AtomicBool>,
}

impl ChannelUi {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(Input::Flash(Box::new(m)));
    }
}

impl job::Ui for ChannelUi {
    fn event(&mut self, e: &job::Event) {
        self.send(Msg::Event(e.clone()));
    }

    fn confirm(&mut self, p: &job::Prompt<'_>) -> bool {
        self.send(Msg::Ask(Ask::Confirm {
            step: p.step,
            file: p.file.name.clone(),
        }));
        matches!(self.answers.recv(), Ok(Answer::Confirm(true)))
    }

    fn adopt(&mut self, p: &job::Prompt<'_>) -> Option<Half> {
        let reason = match p.decision {
            Decision::Adopt { reason } | Decision::Refuse { reason } => reason.clone(),
            Decision::Flash { .. } => String::new(),
        };
        self.send(Msg::Ask(Ask::Adopt {
            step: p.step,
            serial: p.serial.map(String::from),
            reason,
        }));
        match self.answers.recv() {
            Ok(Answer::Adopt(h)) => h,
            _ => None,
        }
    }

    fn cancelled(&mut self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// ISO 8601 UTC, seconds.
pub fn now_iso() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Starts the worker: fetch, then the job, then [`Msg::Finished`].
pub fn spawn(spec: Spec, tx: Sender<Input>) -> Worker {
    let (answers_tx, answers) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = Worker {
        answers: answers_tx,
        cancel: Arc::clone(&cancel),
    };
    std::thread::Builder::new()
        .name("sofleii-flash".into())
        .spawn(move || {
            let mut ui = ChannelUi {
                tx,
                answers,
                cancel,
            };
            let result = run_worker(&spec, &mut ui).map_err(|e| e.view());
            ui.send(Msg::Finished(result));
        })
        .expect("spawning the flash worker");
    worker
}

fn run_worker(spec: &Spec, ui: &mut ChannelUi) -> Result<job::Report, Error> {
    let mut github = crate::github::GitHub::new(Duration::from_secs(30));
    let bundle = source::fetch(&spec.source, DEFAULT_REPO, &mut github)?;
    let summary = output::flash_bundle(Style::plain(output::Format::Human), &bundle);
    ui.send(Msg::Fetched(summary.lines().map(String::from).collect()));
    let path = chips::default_path();
    let mut known = chips::load(&path);
    let mut save = |c: &Chips| chips::save(&path, c);
    let now = now_iso;
    let mut memory = job::Memory {
        chips: &mut known,
        save: &mut save,
        now: &now,
    };
    let mut drives = drive::Sys::default();
    job::run(
        &spec.steps,
        &bundle,
        &mut memory,
        &mut drives,
        ui,
        &job::Options::default(),
    )
}

/// The step's title: `Left half`, `Left half: settings reset`.
pub fn step_title(s: &job::Step) -> String {
    let half = match s.half {
        Half::Left => "Left half",
        Half::Right => "Right half",
    };
    match s.role {
        Role::SettingsReset => format!("{half}: settings reset"),
        _ => half.to_string(),
    }
}
