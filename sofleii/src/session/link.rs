//! The RPC worker thread: the only code in the session that touches Studio
//! RPC. It owns the client and leases it (connect for a batch of requests,
//! drop it after [`Config::lease_idle`] idle), polls profiles and output
//! priority, reads `info` once per (re)connect, and runs the write actions
//! through `commands` as the CLI does.
//!
//! A newly opened link is [`LinkState::Connecting`] until a request answers
//! on it (an open BLE link can still be deaf), then `Connected`; a deaf link
//! stays `Deaf` through its replans until one answers. Retaking the lease on
//! the same healthy link changes nothing.
//!
//! Failures are classified (`classify`) by the error and
//! [`Client::is_dead`](crate::client::Client::is_dead):
//! - **deaf** (a request timed out on a live client, e.g. USB while the
//!   output is BLE): the plan is invalidated and redone at once; a second
//!   deaf in a row backs off like a loss;
//! - **lost** (the client is dead, or a connect failed): reconnect with backoff,
//!   `backoff_base * 2^n` capped at `backoff_cap` (0.5, 1, 2, 4, 5 s);
//! - **not found**: [`LinkState::Searching`], retried with the same backoff;
//! - anything else (an RPC error, a bad reply) is the read's error only.
//!
//! A presence edge from [`super::watch`] retries at once, and also replans a
//! healthy link, so plugging USB in moves the session to it.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{
    Config, ErrorView, Event, LinkState, LogLevel, Outcome, Presence, Request, Scope, Stamped,
    Ticket, jobs,
};
use crate::client::rpc::ble;
use crate::commands::{info, output, profile, profiles};
use crate::connect::{self, Opener};
use crate::transport::{Preference, TransportInfo, TransportKind};
use crate::{Error, Result};

/// The client type the session runs on.
pub type RpcClient = connect::DynClient;

/// How the link was opened, for the front end.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectedVia {
    pub transport: TransportInfo,
    /// Picked by `--transport auto` (vs. forced with a flag).
    pub auto: bool,
}

impl PartialEq for ConnectedVia {
    fn eq(&self, o: &Self) -> bool {
        self.auto == o.auto
            && self.transport.kind == o.transport.kind
            && self.transport.device == o.transport.device
    }
}

/// Opens Studio RPC links for the session, so the link thread has no
/// connection code of its own. Implemented by [`connect::Connector`] (the
/// plan cache and the `--transport` flags); tests give it a fake
/// [`Opener`].
pub trait Connect: Send {
    /// Plans a transport (honouring a cached plan) and opens it.
    fn connect(&mut self) -> Result<(RpcClient, ConnectedVia)>;
    /// Forgets the cached plan: the next [`connect`](Self::connect) plans
    /// afresh (after a deaf link or a presence edge).
    fn invalidate(&mut self);
    /// Opens the transport that isn't `current`, for the read-back after a
    /// write handed Studio over to it.
    fn open_other(&mut self, current: TransportKind) -> Result<RpcClient>;
}

impl<O: Opener + Send> Connect for connect::Connector<O> {
    fn connect(&mut self) -> Result<(RpcClient, ConnectedVia)> {
        let c = connect::Connector::connect(self)?;
        let cfg = self.config();
        let auto =
            cfg.transport == Preference::Auto && cfg.device.is_none() && cfg.bt_address.is_none();
        Ok((
            c.client,
            ConnectedVia {
                transport: c.info,
                auto,
            },
        ))
    }

    fn invalidate(&mut self) {
        connect::Connector::invalidate(self);
    }

    fn open_other(&mut self, current: TransportKind) -> Result<RpcClient> {
        connect::Connector::open_other(self, current)
    }
}

pub(crate) enum Msg {
    Req(Ticket, Request),
    Presence(Presence),
}

/// What a failure means for the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Open but not answering; `hint` from the transport.
    Deaf(Option<String>),
    /// The transport failed.
    Lost(String),
    /// No keyboard to connect to.
    NotFound(String),
    /// The link is fine; only this read failed.
    Read,
}

/// What `e` means for the link. `dead`: [`is_dead`] after a failed
/// request, `None` for a failed connect (nothing to keep, so anything but
/// "not found" or a deaf probe counts as lost).
///
/// [`is_dead`]: crate::client::Client::is_dead
pub(crate) fn classify(e: &Error, dead: Option<bool>) -> Failure {
    match (e, dead) {
        (Error::NotFound(m), _) => Failure::NotFound(m.clone()),
        (_, Some(true)) => Failure::Lost(e.to_string()),
        (Error::Timeout { hint, .. }, _) => Failure::Deaf(hint.clone()),
        (_, None) => Failure::Lost(e.to_string()),
        (_, Some(false)) => Failure::Read,
    }
}

/// The `n`th retry's delay: `base * 2^n`, at most `cap`.
pub fn backoff(base: Duration, cap: Duration, n: u32) -> Duration {
    base.saturating_mul(1u32.checked_shl(n).unwrap_or(u32::MAX))
        .min(cap)
}

pub(crate) fn spawn(
    cfg: Config,
    connector: Box<dyn Connect>,
    events: Sender<Event>,
    jobs: Sender<jobs::Msg>,
) -> Sender<Msg> {
    let (tx, rx) = mpsc::channel();
    // Built on the thread: the client (and its transport) never crosses
    // threads, so `Transport` needn't be `Send`; only the connector moves.
    std::thread::Builder::new()
        .name("sofleii-link".into())
        .spawn(move || Link::new(cfg, connector, events, jobs).run(rx))
        .expect("spawning the link thread");
    tx
}

struct Link {
    cfg: Config,
    connector: Box<dyn Connect>,
    events: Sender<Event>,
    jobs: Sender<jobs::Msg>,
    /// The leased client; `None` between batches and while down.
    client: Option<RpcClient>,
    /// The last link, kept across lease releases.
    via: Option<ConnectedVia>,
    /// Opened but nothing answered on it yet: shown as Connecting (or
    /// still Deaf), and Connected on the first answer.
    pending: Option<ConnectedVia>,
    /// What the front end was last told.
    state: LinkState,
    /// `cormoran_ble`'s index, resolved once per (re)connect.
    ble_index: Option<u32>,
    need_info: bool,
    focused: bool,
    last_used: Instant,
    /// The next poll, or the next connection attempt while down.
    due: Instant,
    /// Failed attempts in a row, for the backoff.
    attempt: u32,
    deaf_streak: u32,
    presence: Option<Presence>,
    /// The front end went away.
    closed: bool,
}

impl Link {
    fn new(
        cfg: Config,
        connector: Box<dyn Connect>,
        events: Sender<Event>,
        jobs: Sender<jobs::Msg>,
    ) -> Self {
        let now = Instant::now();
        Self {
            cfg,
            connector,
            events,
            jobs,
            client: None,
            via: None,
            pending: None,
            state: LinkState::Searching,
            ble_index: None,
            need_info: true,
            focused: true,
            last_used: now,
            due: now,
            attempt: 0,
            deaf_streak: 0,
            presence: None,
            closed: false,
        }
    }

    fn run(mut self, rx: Receiver<Msg>) {
        self.emit(Event::Link(LinkState::Searching));
        while !self.closed {
            if Instant::now() >= self.due {
                let _ = self.poll();
                continue;
            }
            self.release_if_idle();
            let mut wake = self.due;
            if self.client.is_some() {
                wake = wake.min(self.last_used + self.cfg.lease_idle);
            }
            match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
                Ok(Msg::Req(_, Request::Quit)) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Msg::Req(t, r)) => self.handle(t, r),
                Ok(Msg::Presence(p)) => self.on_presence(p),
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn emit(&mut self, e: Event) {
        if self.events.send(e).is_err() {
            self.closed = true;
        }
    }

    fn log(&mut self, level: LogLevel, msg: String) {
        self.emit(Event::log(level, msg));
    }

    fn set_state(&mut self, s: LinkState) {
        if s != self.state {
            self.state = s.clone();
            self.emit(Event::Link(s));
        }
    }

    fn up(&self) -> bool {
        matches!(self.state, LinkState::Connected { .. })
    }

    /// Up, or opened and waiting for its first answer.
    fn linked(&self) -> bool {
        self.up() || self.pending.is_some()
    }

    /// Something answered on the link: a pending one is Connected now.
    fn confirm(&mut self) {
        self.deaf_streak = 0;
        self.attempt = 0;
        if let Some(via) = self.pending.take() {
            let msg = format!(
                "connected over {} {}{}",
                via.transport.kind,
                via.transport.device,
                if via.auto { " (auto)" } else { "" }
            );
            self.log(LogLevel::Info, msg);
            self.set_state(LinkState::Connected { via });
        }
    }

    fn interval(&self) -> Duration {
        if self.focused {
            self.cfg.poll
        } else {
            self.cfg.poll_unfocused
        }
    }

    /// Ends the lease after `lease_idle` without a request.
    fn release_if_idle(&mut self) {
        if self.client.is_some() && self.last_used.elapsed() >= self.cfg.lease_idle {
            self.client = None;
            log::debug!("RPC lease released after {:?} idle", self.cfg.lease_idle);
        }
    }

    /// Drops the client and forgets the plan; `down` also forgets the link.
    fn drop_link(&mut self, down: bool) {
        self.client = None;
        self.connector.invalidate();
        if down {
            self.ble_index = None;
            self.need_info = true;
            self.pending = None;
        }
    }

    /// Takes a client from the lease, connecting if it lapsed.
    fn connect(&mut self) -> Result<(), ErrorView> {
        if self.client.is_some() {
            return Ok(());
        }
        // Searching retries quietly, and a deaf link stays deaf until it
        // answers (no Connecting/Deaf flicker on every replan).
        if !self.linked() && !matches!(self.state, LinkState::Searching | LinkState::Deaf { .. }) {
            let kind = self.via.as_ref().map(|v| v.transport.kind);
            self.set_state(LinkState::Connecting { kind });
        }
        match self.connector.connect() {
            Ok((c, via)) => {
                let was_linked = self.linked();
                if !was_linked || self.via.as_ref() != Some(&via) {
                    if !was_linked {
                        self.need_info = true;
                        self.ble_index = None;
                    }
                    // Open, but not Connected until a request answers on it
                    // (`confirm`): a deaf BLE link opens fine and then times
                    // out. A lease cycle on the same healthy link skips this.
                    self.via = Some(via.clone());
                    if !matches!(self.state, LinkState::Deaf { .. }) {
                        self.set_state(LinkState::Connecting {
                            kind: Some(via.transport.kind),
                        });
                    }
                    self.pending = Some(via);
                }
                self.client = Some(c);
                self.last_used = Instant::now();
                Ok(())
            }
            Err(e) => {
                let v = e.view();
                self.fail(&e, None);
                Err(v)
            }
        }
    }

    /// Updates the link after a failed connect or request. Returns whether
    /// the link went down.
    fn fail(&mut self, e: &Error, dead: Option<bool>) -> bool {
        let now = Instant::now();
        match classify(e, dead) {
            Failure::Read => false,
            Failure::Deaf(hint) => {
                let kind = self
                    .client
                    .as_ref()
                    .map(|c| c.transport_info().kind)
                    .or(self.via.as_ref().map(|v| v.transport.kind));
                self.drop_link(true);
                self.deaf_streak += 1;
                let msg = match kind {
                    Some(k) => format!("{k} went quiet ({e}); replanning"),
                    None => format!("no answer ({e}); replanning"),
                };
                self.log(LogLevel::Warn, msg);
                self.set_state(LinkState::Deaf { kind, hint });
                // Replan at once; a second deaf in a row waits like a loss.
                self.due = if self.deaf_streak == 1 {
                    now
                } else {
                    now + self.next_backoff()
                };
                true
            }
            Failure::Lost(reason) => {
                self.drop_link(true);
                let retry_in = self.next_backoff();
                self.due = now + retry_in;
                self.log(LogLevel::Warn, format!("link lost: {reason}"));
                self.set_state(LinkState::Lost { reason, retry_in });
                true
            }
            Failure::NotFound(m) => {
                self.drop_link(true);
                self.due = now + self.next_backoff();
                if self.state != LinkState::Searching {
                    self.log(LogLevel::Info, format!("searching: {m}"));
                }
                self.set_state(LinkState::Searching);
                true
            }
        }
    }

    fn next_backoff(&mut self) -> Duration {
        let d = backoff(self.cfg.backoff_base, self.cfg.backoff_cap, self.attempt);
        self.attempt = self.attempt.saturating_add(1);
        d
    }

    /// Runs `f` on the leased client (connecting if needed), and classifies
    /// a failure.
    fn with_client<R>(
        &mut self,
        f: impl FnOnce(&mut RpcClient, &mut dyn Connect) -> Result<R>,
    ) -> Result<R, ErrorView> {
        self.connect()?;
        let Some(c) = self.client.as_mut() else {
            unreachable!("connect() leaves a client");
        };
        let r = f(c, self.connector.as_mut());
        let dead = c.is_dead();
        self.last_used = Instant::now();
        match r {
            Ok(v) => {
                self.confirm();
                Ok(v)
            }
            Err(e) => {
                // An RPC error is still an answer: the link works.
                if classify(&e, Some(dead)) == Failure::Read {
                    self.confirm();
                }
                let v = e.view();
                self.fail(&e, Some(dead));
                Err(v)
            }
        }
    }

    /// One batch: `info` if this is a new link, then the profiles. Schedules
    /// the next poll (a failure reschedules it as a retry).
    fn poll(&mut self) -> Result<(), ErrorView> {
        self.due = Instant::now() + self.interval();
        // Reads are only reported from a live link: a failed connect is the
        // link state's news, not each read's.
        self.connect()?;
        if self.need_info {
            self.need_info = false;
            let r = self.with_client(|c, _| info::run(c));
            if let Ok(i) = &r {
                let _ = self.jobs.send(jobs::Msg::Build(i.build.clone()));
            }
            let failed = r.as_ref().err().cloned();
            self.emit(Event::Info(Stamped::now(r)));
            if let Some(e) = failed
                && !self.up()
            {
                return Err(e);
            }
        }
        let index = self.ble_index;
        let r = self.with_client(|c, _| {
            let i = match index {
                Some(i) => i,
                None => c.resolve_subsystem(ble::IDENTIFIER)?,
            };
            profiles::fetch(c, i).map(|p| (i, p))
        });
        let r = r.map(|(i, p)| {
            self.ble_index = Some(i);
            p
        });
        let out = r.as_ref().map(drop).map_err(Clone::clone);
        self.emit(Event::Status(Stamped::now(r)));
        out
    }

    fn done(&mut self, ticket: Ticket, result: Result<Outcome, ErrorView>) {
        self.emit(Event::Done { ticket, result });
    }

    fn handle(&mut self, t: Ticket, req: Request) {
        match req {
            Request::Refresh(scope) => {
                if matches!(scope, Scope::All | Scope::Info) {
                    self.need_info = true;
                }
                let r = self.poll();
                self.done(t, r.map(|()| Outcome::Ok));
            }
            Request::Focus(f) => {
                self.focused = f;
                // Coming back: refresh now rather than in up to 30 s.
                if f {
                    self.due = self.due.min(Instant::now());
                }
                self.done(t, Ok(Outcome::Ok));
            }
            Request::Reconnect => {
                self.drop_link(true);
                self.attempt = 0;
                self.deaf_streak = 0;
                self.via = None;
                self.set_state(LinkState::Connecting { kind: None });
                let r = self.poll();
                self.done(t, r.map(|()| Outcome::Ok));
            }
            Request::SwitchProfile(n) => self.action(t, move |c, conn| {
                let tg = profile::prepare(c, n)?;
                let kind = tg.kind;
                profile::switch(c, &tg, || conn.open_other(kind)).map(Outcome::Profile)
            }),
            Request::RenameProfile(n, name) => {
                if let Err(e) = profile::validate_name(&name) {
                    return self.done(t, Err(e.view()));
                }
                self.action(t, move |c, _| {
                    let tg = profile::prepare(c, n)?;
                    profile::rename(c, &tg, &name).map(Outcome::Profile)
                })
            }
            Request::UnpairProfile(n) => self.action(t, move |c, conn| {
                let tg = profile::prepare(c, n)?;
                let kind = tg.kind;
                profile::unpair(c, &tg, || conn.open_other(kind)).map(Outcome::Profile)
            }),
            Request::SetOutput(target) => self.action(t, move |c, conn| {
                let kind = c.transport_info().kind;
                output::set(c, target, || conn.open_other(kind)).map(Outcome::Output)
            }),
            // Routed to the other threads by the handle; answer anyway.
            Request::CheckUpdate | Request::Quit => self.done(t, Ok(Outcome::Ok)),
        }
    }

    /// Runs a write now, reports it, then follows a handover and refreshes.
    fn action(
        &mut self,
        t: Ticket,
        f: impl FnOnce(&mut RpcClient, &mut dyn Connect) -> Result<Outcome>,
    ) {
        let r = self.with_client(f);
        let moved_to = match &r {
            Ok(Outcome::Profile(o)) => o.studio_transport,
            Ok(Outcome::Output(o)) => o.studio_transport,
            _ => None,
        };
        let current = self.via.as_ref().map(|v| v.transport.kind);
        self.done(t, r);
        if let (Some(to), Some(from)) = (moved_to, current)
            && to != from
        {
            // Studio answers on the other transport now: replan there.
            self.log(
                LogLevel::Info,
                format!("Studio moved from {from} to {to}; following it"),
            );
            self.drop_link(false);
        }
        // A failed connect already scheduled its retry.
        if self.up() {
            let _ = self.poll();
        }
    }

    fn on_presence(&mut self, p: Presence) {
        let prev = self.presence.replace(p);
        // The first report is the starting point, not an edge.
        if prev.is_none() || prev == Some(p) {
            return;
        }
        log::debug!("presence changed: usb {} ble {}", p.usb, p.ble);
        self.drop_link(false);
        self.attempt = 0;
        self.deaf_streak = 0;
        self.due = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let ms = Duration::from_millis;
        let got: Vec<_> = (0..7)
            .map(|n| backoff(ms(500), Duration::from_secs(5), n))
            .collect();
        assert_eq!(
            got,
            [
                ms(500),
                ms(1000),
                ms(2000),
                ms(4000),
                ms(5000),
                ms(5000),
                ms(5000)
            ]
        );
        assert_eq!(backoff(ms(500), ms(5000), 40), ms(5000));
    }

    #[test]
    fn classification() {
        let timeout = Error::Timeout {
            what: "x".into(),
            after: Duration::from_millis(1),
            hint: Some("BLE".into()),
        };
        let io = Error::io("read", std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        let rpc = Error::Rpc("no".into());
        // A live client: a timeout is deaf, an RPC error just a read.
        assert_eq!(
            classify(&timeout, Some(false)),
            Failure::Deaf(Some("BLE".into()))
        );
        assert_eq!(classify(&rpc, Some(false)), Failure::Read);
        // A dead one is lost, whatever the error.
        assert!(matches!(classify(&timeout, Some(true)), Failure::Lost(_)));
        assert!(matches!(classify(&rpc, Some(true)), Failure::Lost(_)));
        // A failed connect: the probe's timeout is deaf, the rest lost.
        assert!(matches!(classify(&timeout, None), Failure::Deaf(_)));
        assert!(matches!(classify(&io, None), Failure::Lost(_)));
        assert!(matches!(
            classify(&Error::NotFound("gone".into()), None),
            Failure::NotFound(_)
        ));
    }
}
