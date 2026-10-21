//! The write actions as the TUI words them: what a confirmation says
//! ([`confirm`]), what the status edge shows while one is in flight
//! ([`doing`]) and once it's done ([`finished`]). Plain data and strings,
//! no drawing; [`super::App`] owns the flow and [`super::view`] draws it.
//!
//! Whether to ask is [`Request::risk`]'s call, the same rule as the CLI's;
//! this only says it in words. Rename and unpair have their wording here
//! already, for when their keys come (v1.1).

use std::time::SystemTime;

use crate::commands::profiles::Profiles;
use crate::session::{self, ActiveBle, ErrorView, LogLevel, Outcome, Request, Risk};
use crate::transport::TransportKind;

/// A request waiting for `y` / `n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub req: Request,
    pub risk: Risk,
    /// The modal's title, e.g. `Switch output?`.
    pub title: String,
    /// What will happen, in plain words.
    pub text: String,
}

/// A write in flight: action keys are ignored until its `Done`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub ticket: session::Ticket,
    pub req: Request,
    /// For the spinner.
    pub since: SystemTime,
}

/// The status edge's message after a write (or a key that did nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flash {
    pub at: SystemTime,
    pub level: LogLevel,
    pub text: String,
    /// Stays until the next key (errors); otherwise it also fades after
    /// [`super::FLASH_FOR`].
    pub sticky: bool,
}

/// The writes, as opposed to refreshes and the like.
pub fn is_action(req: &Request) -> bool {
    matches!(
        req,
        Request::SwitchProfile(_)
            | Request::SetOutput(_)
            | Request::RenameProfile(..)
            | Request::UnpairProfile(_)
    )
}

fn kind_name(k: TransportKind) -> &'static str {
    match k {
        TransportKind::Usb => "USB",
        TransportKind::Ble => "BLE",
    }
}

fn other(k: TransportKind) -> TransportKind {
    crate::commands::handover::other(k)
}

/// Whether this host can reach the keyboard over `k`, from presence.
fn present(snap: &session::Snapshot, k: TransportKind) -> bool {
    snap.presence.is_some_and(|p| match k {
        TransportKind::Usb => p.usb,
        TransportKind::Ble => p.ble,
    })
}

/// How sofleii carries on after its link `from` goes quiet.
fn continues(snap: &session::Snapshot, from: TransportKind) -> String {
    let to = other(from);
    if present(snap, to) {
        format!(
            "sofleii continues over {} (verified by read-back).",
            kind_name(to)
        )
    } else {
        format!(
            "this host has no {} link to the keyboard, so sofleii can't follow or verify it.",
            kind_name(to)
        )
    }
}

/// The modal for `req`, or `None` when [`Request::risk`] says to just send.
pub fn confirm(req: &Request, snap: &session::Snapshot) -> Option<Confirm> {
    let risk = req.risk(snap);
    if risk == Risk::None {
        return None;
    }
    let profiles = snap.status.value.as_ref();
    let (title, text) = match *req {
        Request::SetOutput(target) => ("Switch output?".to_string(), output_text(target, snap)),
        Request::SwitchProfile(n) => (
            "Switch profile?".to_string(),
            switch_text(n, snap, profiles),
        ),
        Request::UnpairProfile(n) => (
            "Unpair profile?".to_string(),
            format!(
                "Unpairing profile {n} deletes its bond: that host has to pair again. \
                 This can't be undone."
            ),
        ),
        // Never risky (rename) or not a write.
        _ => ("Continue?".to_string(), format!("{req:?}")),
    };
    Some(Confirm {
        req: req.clone(),
        risk,
        title,
        text,
    })
}

/// `profile 1`, or `profile 1 "laptop"`.
fn slot(p: &crate::commands::profiles::Profile) -> String {
    match &p.name {
        Some(n) => format!("profile {} \"{n}\"", p.index),
        None => format!("profile {}", p.index),
    }
}

/// What output `target` does from here. BLE is only selected while the
/// active profile is connected, and Studio then answers on that profile's
/// host ([`session::Snapshot::active_ble`]).
fn output_text(target: TransportKind, snap: &session::Snapshot) -> String {
    let from = snap.link.kind().unwrap_or(other(target));
    let active = snap.status.value.as_ref().and_then(|p| p.active());
    match (target, snap.active_ble(), active) {
        (TransportKind::Ble, Some(ActiveBle::NotConnected), Some(p)) => format!(
            "Output priority becomes BLE, but the active {s} isn't connected, so the keyboard \
             keeps using USB until a BLE host connects; sofleii stays on USB. Once {s}'s host \
             connects, keystrokes and Studio move to it.",
            s = slot(p)
        ),
        (TransportKind::Ble, Some(ActiveBle::OtherHost), Some(p)) => format!(
            "The active {s} is connected to another host: keystrokes and Studio move there. \
             This USB link goes quiet and this host isn't the active BLE profile, so sofleii \
             loses the keyboard until the output is switched back on the keyboard or from \
             that host.",
            s = slot(p)
        ),
        _ => format!(
            "Switching output to {}: this {} link goes quiet; {}",
            kind_name(target),
            kind_name(from),
            continues(snap, from)
        ),
    }
}

fn switch_text(n: u32, snap: &session::Snapshot, profiles: Option<&Profiles>) -> String {
    let kind = snap.link.kind();
    let empty = profiles
        .and_then(|p| p.profiles.iter().find(|s| s.index == n))
        .is_some_and(|s| !s.bonded);
    let priority_ble = profiles.is_some_and(|p| p.output_priority == Some("ble"));
    let tail = match kind {
        Some(TransportKind::Ble) => continues(snap, TransportKind::Ble),
        Some(TransportKind::Usb) if priority_ble => format!(
            "with priority BLE its connection can take over from USB, and this USB link goes \
             quiet; {}",
            continues(snap, TransportKind::Usb)
        ),
        _ => "sofleii stays on USB.".to_string(),
    };
    if empty {
        format!(
            "Profile {n} is empty: switching starts pairing on it and disconnects this host's \
             BLE link; {tail}"
        )
    } else {
        format!("Switching to profile {n} drops this host's BLE connection; {tail}")
    }
}

/// `switching to profile 1…`, for the status edge while in flight.
pub fn doing(req: &Request) -> String {
    match req {
        Request::SwitchProfile(n) => format!("switching to profile {n}…"),
        Request::SetOutput(k) => format!("switching output to {}…", kind_name(*k)),
        Request::RenameProfile(n, _) => format!("renaming profile {n}…"),
        Request::UnpairProfile(n) => format!("unpairing profile {n}…"),
        r => format!("{r:?}…"),
    }
}

/// What a request was, for an error: `switching to profile 1`.
pub fn what(req: &Request) -> String {
    doing(req).trim_end_matches('…').to_string()
}

/// ` (verified over usb)`, or ` (not verified)`.
fn verified(ok: bool, over: Option<TransportKind>) -> String {
    match (ok, over) {
        (true, Some(k)) => format!(" (verified over {k})"),
        (true, None) => String::new(),
        (false, _) => " (not verified)".into(),
    }
}

/// The status edge's message and the log lines for a finished write: the
/// first line is the message (as the log shows it too), then any note.
pub fn finished(req: &Request, result: &Result<Outcome, ErrorView>) -> (LogLevel, Vec<String>) {
    let (level, line, note) = match result {
        Err(e) => (
            LogLevel::Error,
            format!("{} failed: {}", what(req), e.message),
            None,
        ),
        Ok(Outcome::Profile(o)) => {
            let line = match o.action {
                "profile_rename" => format!("profile {} renamed", o.profile),
                "profile_unpair" => format!("profile {} unpaired", o.profile),
                _ => format!("profile {} active", o.profile),
            };
            let level = if o.verified {
                LogLevel::Info
            } else {
                LogLevel::Warn
            };
            (
                level,
                format!("{line}{}", verified(o.verified, o.verified_over)),
                o.note.clone(),
            )
        }
        // Priority BLE, but the reply came back over USB: no BLE host is
        // connected, so the firmware kept USB.
        Ok(Outcome::Output(o))
            if o.reply
                && o.output_priority == "ble"
                && o.studio_transport == Some(TransportKind::Usb) =>
        {
            (
                LogLevel::Info,
                "output priority ble; still on USB until a BLE host connects".to_string(),
                o.note.clone(),
            )
        }
        Ok(Outcome::Output(o)) => (
            if o.verified {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            format!(
                "output priority {}{}",
                o.output_priority,
                verified(o.verified, o.verified_over)
            ),
            o.note.clone(),
        ),
        Ok(_) => (LogLevel::Info, format!("{} done", what(req)), None),
    };
    (level, std::iter::once(line).chain(note).collect())
}
