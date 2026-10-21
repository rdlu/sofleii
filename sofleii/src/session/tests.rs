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

fn split_read(e: &Event) -> Option<&Result<crate::commands::split::Split, ErrorView>> {
    match e {
        Event::Split(Stamped { value, .. }) => Some(value),
        _ => None,
    }
}

#[test]
fn the_split_link_follows_each_status() {
    for (kb_split, connected, known) in [
        (Some(true), Some(true), true),
        (Some(false), Some(false), true),
        // Older firmware: an empty reply, unknown rather than disconnected.
        (None, None, false),
    ] {
        let s = start(
            |c| c,
            Kb {
                split: kb_split,
                ..Kb::default()
            },
        );
        let seen = s.until("split", |e| split_read(e).is_some());
        assert!(seen.iter().any(status_ok), "split comes after status");
        let sp = split_read(seen.last().unwrap()).unwrap().clone().unwrap();
        assert_eq!((sp.connected, sp.known), (connected, known), "{kb_split:?}");
        // One lease for info, status and split.
        assert_eq!(s.kb.lock().opened, 1);
    }
}

#[test]
fn a_split_edge_rereads_the_batteries_now_and_later() {
    let s = start(
        |c| Config {
            battery_after_split: Duration::from_millis(50),
            ..c
        },
        Kb::default(),
    );
    s.until("split", |e| split_read(e).is_some());
    // The start-up read only (battery_every is an hour away).
    s.wait_kb("first battery read", |kb| kb.battery_reads == 1);
    // No edge: a poll with the link still up reads nothing more.
    let t = s.h.send(Request::Refresh(Scope::Status));
    assert!(s.done(t).1.is_ok());
    assert_eq!(s.kb.lock().battery_reads, 1);

    // The right half goes off: a read at once, and the follow-up.
    s.kb.lock().split = Some(false);
    let t = s.h.send(Request::Refresh(Scope::Status));
    assert!(s.done(t).1.is_ok());
    s.wait_kb("read on the edge + follow-up", |kb| kb.battery_reads == 3);
    // And back on: the same again.
    s.kb.lock().split = Some(true);
    let t = s.h.send(Request::Refresh(Scope::Status));
    assert!(s.done(t).1.is_ok());
    s.wait_kb("read on the edge + follow-up", |kb| kb.battery_reads == 5);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(s.kb.lock().battery_reads, 5, "one follow-up per edge");
}

#[test]
fn an_unknown_split_never_rereads_the_batteries() {
    let s = start(
        |c| Config {
            battery_after_split: Duration::from_millis(10),
            ..c
        },
        Kb {
            split: None,
            ..Kb::default()
        },
    );
    s.until("split", |e| split_read(e).is_some());
    s.wait_kb("first battery read", |kb| kb.battery_reads == 1);
    let t = s.h.send(Request::Refresh(Scope::Status));
    assert!(s.done(t).1.is_ok());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.kb.lock().battery_reads, 1);
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
fn an_output_handover_is_verified_over_ble_and_the_link_follows() {
    let s = start(
        |c| c,
        Kb {
            host_profile: Some(0),
            ..Kb::default()
        },
    );
    let seen = s.until("status", status_ok);
    assert_eq!(
        links(&seen).last().and_then(LinkState::kind),
        Some(TransportKind::Usb)
    );
    let t = s.h.send(Request::SetOutput(TransportKind::Ble));
    let (seen, r) = s.done(t);
    // The firmware switched before replying: silence over USB, then the
    // read-back over BLE.
    match r {
        Ok(Outcome::Output(o)) => {
            assert!(o.verified && !o.reply, "{o:?}");
            assert_eq!(o.verified_over, Some(TransportKind::Ble));
            assert_eq!(o.studio_transport, Some(TransportKind::Ble));
        }
        other => panic!("{other:?}"),
    }
    assert!(
        links(&seen).is_empty(),
        "no link event before Done: {seen:#?}"
    );
    // Then the session replans (USB is deaf now) and settles on BLE, with
    // no Deaf/Lost/Searching on the way.
    let seen = s.until("status", status_ok);
    let got = links(&seen);
    assert_eq!(
        got,
        [
            LinkState::Connecting {
                kind: Some(TransportKind::Ble)
            },
            got.last().cloned().unwrap()
        ],
        "{seen:#?}"
    );
    assert_eq!(got[1].kind(), Some(TransportKind::Ble));
    let Some(Event::Status(Stamped { value: Ok(p), .. })) = seen.last() else {
        unreachable!()
    };
    assert_eq!(p.output_priority, Some("ble"));
}

#[test]
fn output_ble_with_the_active_profile_unconnected_stays_on_usb() {
    // Profile 1 (another host) is active but not connected; 0 is this
    // host, connected. The firmware's BLE priority falls back to USB, so
    // the reply comes back here and the link doesn't move.
    let s = start(
        |c| c,
        Kb {
            active: 1,
            connected: [true, false],
            host_profile: Some(0),
            ..Kb::default()
        },
    );
    s.until("status", status_ok);
    let t = s.h.send(Request::SetOutput(TransportKind::Ble));
    let (seen, r) = s.done(t);
    match r {
        Ok(Outcome::Output(o)) => {
            assert!(o.reply && o.verified, "{o:?}");
            assert_eq!(o.verified_over, Some(TransportKind::Usb));
            assert_eq!(o.studio_transport, Some(TransportKind::Usb));
            assert!(o.note.unwrap().contains("isn't connected"));
        }
        other => panic!("{other:?}"),
    }
    let seen: Vec<_> = seen
        .into_iter()
        .chain(s.until("status", status_ok))
        .collect();
    assert!(links(&seen).is_empty(), "no link change: {seen:#?}");
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, Event::Log { msg, .. } if msg.contains("Studio moved"))),
        "{seen:#?}"
    );
    let Some(Event::Status(Stamped { value: Ok(p), .. })) = seen.last() else {
        unreachable!()
    };
    assert_eq!(p.output_priority, Some("ble"));
    assert_eq!(s.kb.lock().selected(), Some(TransportKind::Usb));
}

#[test]
fn output_ble_to_another_hosts_profile_loses_the_keyboard() {
    // Profile 1 is active and connected, to another host: BLE is
    // selected, Studio answers there, and neither of this host's links
    // hears it.
    let s = start(
        |c| c,
        Kb {
            active: 1,
            host_profile: Some(0),
            ..Kb::default()
        },
    );
    s.until("status", status_ok);
    let t = s.h.send(Request::SetOutput(TransportKind::Ble));
    let (_, r) = s.done(t);
    match r {
        Ok(Outcome::Output(o)) => {
            assert!(!o.reply && !o.verified, "{o:?}");
            assert!(o.note.unwrap().contains("not verified"));
        }
        other => panic!("{other:?}"),
    }
    // The session follows to BLE, which is open but deaf: steadily not
    // answering, never Connected.
    let seen = s.until("deaf", |e| matches!(e, Event::Link(LinkState::Deaf { .. })));
    assert!(!seen.iter().any(connected), "{seen:#?}");
}

/// [`snapshot`] with profile 1 active, its `connected` flag, distinct
/// addresses (0 is this host's), and this host's adapters/presence.
fn active_one(connected: bool, adapters: &[&str], ble: Option<bool>) -> Snapshot {
    let mut s = snapshot(Some(TransportKind::Usb), 1, "usb");
    let p = s.status.value.as_mut().unwrap();
    for (slot, addr) in p
        .profiles
        .iter_mut()
        .zip(["84:9E:56:52:6A:D2", "AA:BB:CC:DD:EE:FF"])
    {
        slot.address = Some(addr.into());
    }
    p.profiles[1].connected = connected;
    s.adapters = adapters.iter().map(|a| a.to_string()).collect();
    s.presence = ble.map(|ble| Presence { usb: true, ble });
    s
}

#[test]
fn output_ble_risk_follows_who_the_active_profile_is() {
    use TransportKind::Ble;
    let ours = ["84:9e:56:52:6a:d2"];
    // Not connected: the keyboard stays on USB for now, but asks.
    let s = active_one(false, &ours, Some(true));
    assert_eq!(s.active_ble(), Some(ActiveBle::NotConnected));
    assert_eq!(Request::SetOutput(Ble).risk(&s), Risk::DropsCurrentLink);
    // Connected to another host (by address, case-insensitive): out of
    // reach from here.
    let s = active_one(true, &ours, Some(true));
    assert_eq!(s.active_ble(), Some(ActiveBle::OtherHost));
    assert_eq!(Request::SetOutput(Ble).risk(&s), Risk::Destructive);
    // To this host.
    let s = active_one(true, &["AA:BB:CC:DD:EE:FF"], Some(true));
    assert_eq!(s.active_ble(), Some(ActiveBle::ThisHost));
    assert_eq!(Request::SetOutput(Ble).risk(&s), Risk::DropsCurrentLink);
    // No adapter address: no BLE link from this host at all means another
    // host's; with one, it can't be told.
    let s = active_one(true, &[], Some(false));
    assert_eq!(s.active_ble(), Some(ActiveBle::OtherHost));
    assert_eq!(Request::SetOutput(Ble).risk(&s), Risk::Destructive);
    let s = active_one(true, &[], Some(true));
    assert_eq!(s.active_ble(), Some(ActiveBle::Unknown));
    assert_eq!(Request::SetOutput(Ble).risk(&s), Risk::DropsCurrentLink);
    // Back to USB is unaffected.
    let mut s = active_one(true, &ours, Some(true));
    s.link = snapshot(Some(Ble), 1, "ble").link;
    assert_eq!(
        Request::SetOutput(TransportKind::Usb).risk(&s),
        Risk::DropsCurrentLink
    );
    assert_eq!(Snapshot::new(&Config::default()).active_ble(), None);
}

#[test]
fn adapters_fold_into_the_snapshot() {
    let mut s = Snapshot::new(&Config::default());
    s.apply(&Event::Adapters(vec!["84:9E:56:52:6A:D2".into()]));
    assert_eq!(s.adapters, ["84:9E:56:52:6A:D2"]);
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["adapters"][0], "84:9E:56:52:6A:D2");
}

#[test]
fn a_bad_rename_fails_before_any_rpc() {
    let s = start(|c| c, Kb::default());
    // The split read ends the first batch.
    s.until("split", |e| split_read(e).is_some());
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
    // An empty slot starts pairing and drops the BLE connection, even over
    // USB with priority usb.
    assert_eq!(Request::SwitchProfile(2).risk(&usb), Risk::DropsCurrentLink);
    let on_empty = snapshot(Some(Usb), 2, "usb");
    assert_eq!(
        Request::SwitchProfile(2).risk(&on_empty),
        Risk::None,
        "already active"
    );

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
