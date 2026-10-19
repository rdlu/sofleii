//! The session's threads against the fake keyboard. Time is driven through
//! [`Config`]: whatever a test doesn't exercise is an hour away, so a
//! passing test can't be a lucky timer, and waits are on events or
//! counters (bounded by [`WAIT`]), never on fixed sleeps.

use crate::Error;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::fake::{FakeProbe, Mode, Shared, Step, TempDir, connector};
use super::*;
use crate::commands::profiles::Profile;
use crate::commands::update::tests::FakeHttp;
use crate::transport::TransportInfo;

/// Upper bound for anything a test waits on.
const WAIT: Duration = Duration::from_secs(5);
const HOUR: Duration = Duration::from_secs(3600);

/// Every timer an hour away.
fn quiet(dir: &TempDir) -> Config {
    Config {
        poll: HOUR,
        poll_unfocused: HOUR,
        lease_idle: HOUR,
        backoff_base: HOUR,
        backoff_cap: HOUR,
        battery_every: HOUR,
        usb_every: HOUR,
        ble_every: HOUR,
        update_every: HOUR,
        cache_dir: Some(dir.0.clone()),
        ..Config::default()
    }
}

struct Harness {
    h: SessionHandle,
    rx: Receiver<Event>,
    kb: Shared,
    _dir: TempDir,
}

fn start(cfg: impl FnOnce(Config) -> Config, kb: Kb) -> Harness {
    let dir = TempDir::new();
    let cfg = cfg(quiet(&dir));
    let shared = Shared::default();
    *shared.lock() = kb;
    let probe = FakeProbe::new(&shared);
    let (h, rx) = Session::spawn_with(
        cfg,
        Backends {
            connector: Box::new(connector(&shared)),
            probe: Box::new(probe),
            http: Box::new(FakeHttp::default()),
        },
    );
    Harness {
        h,
        rx,
        kb: shared,
        _dir: dir,
    }
}

use super::fake::Kb;

impl Harness {
    /// Events up to and including the first that matches.
    fn until(&self, what: &str, pred: impl Fn(&Event) -> bool) -> Vec<Event> {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            match self
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(e) => {
                    let hit = pred(&e);
                    seen.push(e);
                    if hit {
                        return seen;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no {what} within {WAIT:?}: {seen:#?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("session stopped: {seen:#?}"),
            }
        }
    }

    fn done(&self, t: Ticket) -> (Vec<Event>, Result<Outcome, ErrorView>) {
        let seen = self.until(
            "Done",
            |e| matches!(e, Event::Done { ticket, .. } if *ticket == t),
        );
        let Some(Event::Done { result, .. }) = seen.last().cloned() else {
            unreachable!()
        };
        (seen, result)
    }

    /// Waits for a condition on the fake keyboard.
    fn wait_kb(&self, what: &str, cond: impl Fn(&Kb) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !cond(&self.kb.lock()) {
            assert!(Instant::now() < deadline, "{what}: {:?}", self.kb.lock());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn status_ok(e: &Event) -> bool {
    matches!(e, Event::Status(Stamped { value: Ok(_), .. }))
}

fn connected(e: &Event) -> bool {
    matches!(e, Event::Link(LinkState::Connected { .. }))
}

#[test]
fn first_poll_reads_info_then_status_on_one_lease() {
    let s = start(|c| c, Kb::default());
    let seen = s.until("status", status_ok);
    assert!(seen.iter().any(connected));
    let info = seen
        .iter()
        .find_map(|e| match e {
            Event::Info(Stamped { value: Ok(i), .. }) => Some(i.clone()),
            _ => None,
        })
        .expect("info before status");
    assert_eq!(info.device.name, "Eyelash Sofle");
    assert!(info.build.available);
    assert_eq!(s.kb.lock().opened, 1);
}

#[test]
fn the_lease_drops_the_client_after_idle() {
    let s = start(
        |c| Config {
            lease_idle: Duration::from_millis(20),
            ..c
        },
        Kb::default(),
    );
    s.until("status", status_ok);
    s.wait_kb("lease released", |kb| kb.dropped == 1);
    assert_eq!(s.kb.lock().opened, 1, "info and status shared one lease");

    // The next request takes a new lease, quietly: no new link state and no
    // second info read (it's the same link).
    let t = s.h.send(Request::Refresh(Scope::Status));
    let (seen, r) = s.done(t);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(s.kb.lock().opened, 2);
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, Event::Link(_) | Event::Info(_))),
        "{seen:#?}"
    );
    assert!(seen.iter().any(status_ok));
}

#[test]
fn a_deaf_link_invalidates_the_plan_and_replans_at_once() {
    let s = start(
        |c| c,
        Kb {
            // Past the connect probe, then quiet.
            modes: [Mode::DeafAfter(1)].into(),
            ..Kb::default()
        },
    );
    // Backoff is an hour, so only the immediate replan gets here in time.
    let seen = s.until("status", status_ok);
    let deaf = seen
        .iter()
        .position(|e| matches!(e, Event::Link(LinkState::Deaf { .. })))
        .expect("a deaf link");
    let back = seen.iter().rposition(connected).unwrap();
    assert!(deaf < back, "{seen:#?}");
    let kb = s.kb.lock();
    // The first plan, and the fresh one after `invalidate`.
    assert_eq!(kb.plans, 2, "{kb:?}");
    assert_eq!(kb.opened, 2);
}

/// The link states in `seen`, in order.
fn links(seen: &[Event]) -> Vec<LinkState> {
    seen.iter()
        .filter_map(|e| match e {
            Event::Link(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_link_that_opens_but_never_answers_is_steadily_deaf() {
    let ms = Duration::from_millis;
    let s = start(
        |c| Config {
            backoff_base: ms(1),
            backoff_cap: ms(2),
            ..c
        },
        Kb {
            // Past the connect probe, then quiet, four times; then healthy.
            modes: [Mode::DeafAfter(1); 4].into(),
            ..Kb::default()
        },
    );
    let seen = s.until("connected", connected);
    let states = links(&seen);
    // Opened four times without an answer, but never Connected, and no
    // Connecting between the replans: one steady Deaf.
    assert!(
        matches!(
            states.as_slice(),
            [
                LinkState::Searching,
                LinkState::Connecting {
                    kind: Some(TransportKind::Usb)
                },
                LinkState::Deaf { .. },
                LinkState::Connected { .. },
            ]
        ),
        "{states:#?}"
    );
    assert_eq!(s.kb.lock().opened, 5);
}

#[test]
fn a_healthy_link_stays_connected_across_leases() {
    let ms = Duration::from_millis;
    let s = start(
        |c| Config {
            poll: ms(15),
            lease_idle: ms(3),
            ..c
        },
        Kb::default(),
    );
    let polls = std::cell::Cell::new(0);
    let seen = s.until("four polls", |e| {
        if status_ok(e) {
            polls.set(polls.get() + 1);
        }
        polls.get() == 4
    });
    assert!(s.kb.lock().opened >= 3, "{:?}", s.kb.lock());
    let states = links(&seen);
    assert!(
        matches!(
            states.as_slice(),
            [
                LinkState::Searching,
                LinkState::Connecting { .. },
                LinkState::Connected { .. }
            ]
        ),
        "{states:#?}"
    );
    let connects = seen
        .iter()
        .filter(|e| matches!(e, Event::Log { msg, .. } if msg.starts_with("connected over")))
        .count();
    assert_eq!(connects, 1, "logged once, not per lease");
}

#[test]
fn a_lost_link_backs_off() {
    let ms = Duration::from_millis;
    let s = start(
        |c| Config {
            backoff_base: ms(5),
            backoff_cap: ms(20),
            ..c
        },
        Kb {
            steps: [Step::Io; 5].into(),
            ..Kb::default()
        },
    );
    let seen = s.until("connected", connected);
    let delays: Vec<_> = seen
        .iter()
        .filter_map(|e| match e {
            Event::Link(LinkState::Lost { retry_in, .. }) => Some(*retry_in),
            _ => None,
        })
        .collect();
    assert_eq!(delays, [ms(5), ms(10), ms(20), ms(20), ms(20)]);
    assert_eq!(s.kb.lock().attempts, 6);
}

#[test]
fn a_dead_transport_is_lost_not_deaf() {
    let s = start(
        |c| Config {
            backoff_base: Duration::from_millis(1),
            ..c
        },
        Kb {
            modes: [Mode::Dead].into(),
            ..Kb::default()
        },
    );
    let seen = s.until("status", status_ok);
    assert!(
        seen.iter()
            .any(|e| matches!(e, Event::Link(LinkState::Lost { .. })))
    );
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, Event::Link(LinkState::Deaf { .. })))
    );
}

#[test]
fn a_presence_edge_retries_at_once() {
    let s = start(
        |c| Config {
            usb_every: Duration::from_millis(2),
            ..c
        },
        Kb {
            usb: false,
            ..Kb::default()
        },
    );
    // The starting presence (not an edge) and the failed first attempt.
    s.until("presence", |e| matches!(e, Event::Presence(p) if !p.usb));
    s.wait_kb("first attempt", |kb| kb.plans == 1);
    // The next retry is an hour away; plugging USB in skips the wait.
    s.kb.lock().usb = true;
    let seen = s.until("connected", connected);
    assert!(
        seen.iter()
            .any(|e| matches!(e, Event::Presence(p) if p.usb))
    );
    let kb = s.kb.lock();
    assert_eq!((kb.plans, kb.attempts), (2, 1), "{kb:?}");
}

#[test]
fn an_action_reports_done_then_refreshes() {
    let s = start(|c| c, Kb::default());
    s.until("status", status_ok);
    let t = s.h.send(Request::SwitchProfile(1));
    let (_, r) = s.done(t);
    match r {
        Ok(Outcome::Profile(o)) => {
            assert!(o.verified, "{o:?}");
            assert_eq!(o.active_profile, Some(1));
        }
        other => panic!("{other:?}"),
    }
    let seen = s.until("status", status_ok);
    let Some(Event::Status(Stamped { value: Ok(p), .. })) = seen.last() else {
        unreachable!()
    };
    assert_eq!(p.active_profile, Some(1));

    let t = s.h.send(Request::SetOutput(TransportKind::Usb));
    let (_, r) = s.done(t);
    assert!(
        matches!(r, Ok(Outcome::Output(ref o)) if o.verified),
        "{r:?}"
    );
}

#[test]
fn a_bad_rename_fails_before_any_rpc() {
    let s = start(|c| c, Kb::default());
    s.until("status", status_ok);
    let before = s.kb.lock().requests;
    let t = s.h.send(Request::RenameProfile(0, "x".repeat(40)));
    let (_, r) = s.done(t);
    assert_eq!(r.unwrap_err().kind, "usage");
    assert_eq!(s.kb.lock().requests, before);
}

#[test]
fn battery_refresh_is_answered_by_the_watch_thread() {
    let s = start(|c| c, Kb::default());
    let t = s.h.send(Request::Refresh(Scope::Battery));
    let (seen, r) = s.done(t);
    assert_eq!(r.unwrap_err().kind, "not_found");
    assert!(
        seen.iter()
            .any(|e| matches!(e, Event::Battery(Stamped { value: Err(_), .. })))
    );
}

#[test]
fn quit_stops_every_thread() {
    let s = start(|c| c, Kb::default());
    s.until("status", status_ok);
    s.h.send(Request::Quit);
    let Harness { h, rx, .. } = s;
    drop(h);
    let deadline = Instant::now() + WAIT;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(_) => {}
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("threads still running"),
        }
    }
}

fn snapshot(kind: Option<TransportKind>, active: u32, priority: &'static str) -> Snapshot {
    let mut s = Snapshot::new(&Config::default());
    if let Some(kind) = kind {
        s.link = LinkState::Connected {
            via: ConnectedVia {
                transport: TransportInfo {
                    kind,
                    device: "x".into(),
                },
                auto: true,
            },
        };
    }
    let slot = |index: u32, bonded: bool| Profile {
        index,
        active: index == active,
        connected: bonded,
        bonded,
        address: None,
        name: None,
    };
    s.status.value = Some(Profiles {
        max_profiles: 3,
        active_profile: Some(active),
        output_priority: Some(priority),
        profiles: vec![slot(0, true), slot(1, true), slot(2, false)],
    });
    s
}

#[test]
fn risk_follows_the_handover_rules() {
    use TransportKind::{Ble, Usb};
    let usb = snapshot(Some(Usb), 0, "usb");
    let ble = snapshot(Some(Ble), 0, "ble");
    let usb_prio_ble = snapshot(Some(Usb), 0, "ble");

    // Unpair always confirms, unless the slot is known empty.
    assert_eq!(Request::UnpairProfile(1).risk(&usb), Risk::Destructive);
    assert_eq!(Request::UnpairProfile(0).risk(&ble), Risk::Destructive);
    assert_eq!(Request::UnpairProfile(2).risk(&usb), Risk::None);
    let unknown = Snapshot::new(&Config::default());
    assert_eq!(Request::UnpairProfile(2).risk(&unknown), Risk::Destructive);

    // Switching moves Studio over BLE, or over USB with priority ble.
    assert_eq!(Request::SwitchProfile(1).risk(&usb), Risk::None);
    assert_eq!(Request::SwitchProfile(1).risk(&ble), Risk::DropsCurrentLink);
    assert_eq!(
        Request::SwitchProfile(1).risk(&usb_prio_ble),
        Risk::DropsCurrentLink
    );
    assert_eq!(
        Request::SwitchProfile(0).risk(&ble),
        Risk::None,
        "no change"
    );
    assert_eq!(Request::SwitchProfile(1).risk(&unknown), Risk::None);

    // Output: away from the transport in use.
    assert_eq!(Request::SetOutput(Ble).risk(&usb), Risk::DropsCurrentLink);
    assert_eq!(Request::SetOutput(Usb).risk(&usb), Risk::None);
    assert_eq!(Request::SetOutput(Usb).risk(&ble), Risk::DropsCurrentLink);

    for r in [
        Request::RenameProfile(0, "desk".into()),
        Request::Refresh(Scope::All),
        Request::CheckUpdate,
        Request::Reconnect,
        Request::Focus(false),
    ] {
        assert_eq!(r.risk(&ble), Risk::None, "{r:?}");
    }
}

#[test]
fn events_serialize_with_a_type_tag() {
    let at = ser::from_ms(1_800_000_000_000);
    let events = [
        Event::Link(LinkState::Searching),
        Event::Link(LinkState::Lost {
            reason: "gone".into(),
            retry_in: Duration::from_millis(500),
        }),
        Event::Presence(Presence {
            usb: true,
            ble: false,
        }),
        Event::Status(Stamped {
            at,
            value: Err(Error::Rpc("no".into()).view()),
        }),
        Event::Done {
            ticket: Ticket(7),
            result: Ok(Outcome::Ok),
        },
        Event::Log {
            at,
            level: LogLevel::Warn,
            msg: "hm".into(),
        },
    ];
    let json: Vec<_> = events
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    assert_eq!(
        json[0],
        serde_json::json!({"type": "link", "data": {"state": "searching"}})
    );
    assert_eq!(json[1]["data"]["retry_in_ms"], 500);
    assert_eq!(json[2]["type"], "presence");
    assert_eq!(json[3]["data"]["at_ms"], 1_800_000_000_000u64);
    assert_eq!(json[3]["data"]["value"]["Err"]["kind"], "rpc");
    assert_eq!(json[4]["data"]["ticket"], 7);
    assert_eq!(json[5]["data"]["level"], "warn");
}
