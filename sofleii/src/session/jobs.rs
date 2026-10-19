//! One-off work off the link thread. For now only the update check:
//! [`update::check`]'s HTTP part against the build the link read with
//! `info` (no RPC here). It runs once the build is known, then every
//! [`Config::update_every`], and on [`Request::CheckUpdate`](super::Request).
//!
//! Results are cached as JSON in `$XDG_CACHE_HOME/sofleii/update.json`
//! (falling back to `~/.cache/sofleii/`), keyed by the repo and the running
//! `zmk_config_version`, and honoured for [`Config::update_ttl`]. A failed
//! check (a rate limit included) never touches the cache, and the file is
//! replaced atomically, so it is never half-written. While a stale entry is
//! being refreshed it is reported first, so the front end has a last good
//! value even if GitHub refuses.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use super::{Config, ErrorView, Event, Outcome, Stamped, Ticket, ser};
use crate::commands::info::Build;
use crate::commands::update::{self, Check, Http, NO_DEVICE_INFO, Reply, Running};
use crate::error::exit;
use crate::{Error, Result};

/// The cache file's name in the cache directory.
pub const CACHE_FILE: &str = "update.json";
/// Bumped when [`CacheFile`] changes shape; other schemas are ignored.
const CACHE_SCHEMA: u32 = 1;

/// An update check as the front end shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateReport {
    pub check: Check,
    /// When GitHub was asked (for a cached report, by whoever asked).
    #[serde(rename = "checked_at_ms", serialize_with = "ser::unix_ms")]
    pub checked_at: SystemTime,
    pub from_cache: bool,
}

/// `update.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    schema: u32,
    repo: String,
    /// The running `zmk_config_version` it was checked for.
    version: Option<String>,
    dirty: bool,
    checked_at_ms: u64,
    check: Check,
}

impl CacheFile {
    fn matches(&self, repo: &str, r: &Running) -> bool {
        self.schema == CACHE_SCHEMA
            && self.repo == repo
            && self.version == r.version
            && self.dirty == r.dirty
    }

    fn report(&self) -> UpdateReport {
        UpdateReport {
            check: self.check.clone(),
            checked_at: ser::from_ms(self.checked_at_ms),
            from_cache: true,
        }
    }
}

/// `$XDG_CACHE_HOME/sofleii` (an absolute path only, as the spec says),
/// else `$HOME/.cache/sofleii`.
pub fn default_cache_dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let base = xdg.or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|h| h.join(".cache"))
    })?;
    Some(base.join("sofleii"))
}

fn load(path: &Path) -> Option<CacheFile> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|e| log::debug!("ignoring {}: {e}", path.display()))
        .ok()
}

/// Writes a temporary file next to `path` and renames it over: readers see
/// the old file or the new one, never a torn write.
fn store(path: &Path, c: &CacheFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(c).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Notes whether any reply was a rate limit (the error alone is a string).
struct RateWatch<'a> {
    inner: &'a mut dyn Http,
    limited: bool,
}

impl Http for RateWatch<'_> {
    fn get(&mut self, path: &str, accept: &str) -> Result<Reply> {
        let r = self.inner.get(path, accept)?;
        self.limited |= r.rate_limited;
        Ok(r)
    }
}

/// The update check with its cache; the thread's logic, testable without
/// one.
pub struct Checker {
    repo: String,
    ttl: Duration,
    /// `None`: no cache (no home directory).
    path: Option<PathBuf>,
}

impl Checker {
    pub fn new(repo: String, ttl: Duration, cache_dir: Option<PathBuf>) -> Self {
        Self {
            repo,
            ttl,
            path: cache_dir.map(|d| d.join(CACHE_FILE)),
        }
    }

    /// One check for `build` at `now`: what to report, in order (a stale
    /// cached report first when GitHub is asked). `force` skips a fresh
    /// cache entry.
    pub fn run(
        &self,
        http: &mut dyn Http,
        build: &Build,
        force: bool,
        now: SystemTime,
    ) -> Vec<Result<UpdateReport, ErrorView>> {
        if !build.available {
            let why = build.note.clone().unwrap_or_else(|| NO_DEVICE_INFO.into());
            return vec![Err(Error::Unsupported(why).view())];
        }
        let running = update::running_from(build);
        let mut out = Vec::new();
        let cached = self
            .path
            .as_deref()
            .and_then(load)
            .filter(|c| c.matches(&self.repo, &running));
        if let Some(c) = &cached {
            // An entry from the future (the clock moved back) counts as expired.
            let age = now.duration_since(ser::from_ms(c.checked_at_ms)).ok();
            if !force && age.is_some_and(|a| a < self.ttl) {
                return vec![Ok(c.report())];
            }
            out.push(Ok(c.report()));
        }
        let mut w = RateWatch {
            inner: http,
            limited: false,
        };
        match update::check(&mut w, &self.repo, running.clone()) {
            Ok(check) => {
                if let Some(p) = &self.path {
                    let entry = CacheFile {
                        schema: CACHE_SCHEMA,
                        repo: self.repo.clone(),
                        version: running.version,
                        dirty: running.dirty,
                        checked_at_ms: ser::to_ms(now),
                        check: check.clone(),
                    };
                    if let Err(e) = store(p, &entry) {
                        log::warn!("update check: can't write {}: {e}", p.display());
                    }
                }
                out.push(Ok(UpdateReport {
                    check,
                    checked_at: now,
                    from_cache: false,
                }));
            }
            Err(e) => {
                let mut v = e.view();
                if w.limited {
                    v.kind = "rate_limited";
                }
                out.push(Err(v));
            }
        }
        out
    }
}

pub(crate) enum Msg {
    /// The running build, from each `info` read.
    Build(Build),
    /// `force`: past the cache.
    Check {
        ticket: Option<Ticket>,
        force: bool,
    },
    Quit,
}

pub(crate) fn spawn(cfg: Config, http: Box<dyn Http + Send>, events: Sender<Event>) -> Sender<Msg> {
    let (tx, rx) = mpsc::channel();
    let cache_dir = cfg.cache_dir.clone().or_else(default_cache_dir);
    let checker = Checker::new(cfg.repo.clone(), cfg.update_ttl, cache_dir);
    std::thread::Builder::new()
        .name("sofleii-jobs".into())
        .spawn(move || run(cfg.update_every, checker, http, events, rx))
        .expect("spawning the jobs thread");
    tx
}

fn run(
    every: Duration,
    checker: Checker,
    mut http: Box<dyn Http + Send>,
    events: Sender<Event>,
    rx: Receiver<Msg>,
) {
    let mut build: Option<Build> = None;
    let mut due: Option<Instant> = None;
    loop {
        let msg = match due {
            Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let (ticket, force) = match msg {
            Ok(Msg::Build(b)) => {
                if build.as_ref() == Some(&b) {
                    continue;
                }
                build = Some(b);
                (None, false)
            }
            Ok(Msg::Check { ticket, force }) => (ticket, force),
            Err(RecvTimeoutError::Timeout) => (None, false),
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => return,
        };
        let Some(b) = &build else {
            if let Some(ticket) = ticket {
                let _ = events.send(Event::Done {
                    ticket,
                    result: Err(ErrorView {
                        kind: "pending",
                        message: "the running build isn't known yet (no info read from the \
                                  keyboard)"
                            .into(),
                        exit_code: exit::TRANSPORT,
                    }),
                });
            }
            continue;
        };
        let now = SystemTime::now();
        let results = checker.run(http.as_mut(), b, force, now);
        due = Some(Instant::now() + every);
        let last = results.last().cloned();
        for r in results {
            if events
                .send(Event::Update(Stamped { at: now, value: r }))
                .is_err()
            {
                return;
            }
        }
        if let (Some(ticket), Some(r)) = (ticket, last) {
            let _ = events.send(Event::Done {
                ticket,
                result: r.map(Outcome::Update),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::update::tests::{FakeHttp, RELEASE_SHA};
    use crate::session::fake::TempDir;

    const HOUR: Duration = Duration::from_secs(3600);

    /// The release commit itself: up to date in two requests.
    fn build() -> Build {
        Build {
            available: true,
            zmk_config_version: Some(RELEASE_SHA[..7].into()),
            ..Default::default()
        }
    }

    fn t0() -> SystemTime {
        ser::from_ms(1_800_000_000_000)
    }

    fn checker(dir: &TempDir) -> Checker {
        Checker::new("rdlu/sofleii".into(), 6 * HOUR, Some(dir.0.clone()))
    }

    fn ok(r: &Result<UpdateReport, ErrorView>) -> &UpdateReport {
        r.as_ref().expect("an update report")
    }

    #[test]
    fn a_fresh_cache_answers_without_github() {
        let dir = TempDir::new();
        let c = checker(&dir);
        let mut http = FakeHttp::released();
        let first = c.run(&mut http, &build(), false, t0());
        assert_eq!(first.len(), 1);
        assert!(!ok(&first[0]).from_cache);
        assert_eq!(ok(&first[0]).check.status, update::Status::UpToDate);
        let asked = http.requested.len();
        assert!(asked > 0);

        let again = c.run(&mut http, &build(), false, t0() + HOUR);
        assert_eq!(again.len(), 1);
        assert!(ok(&again[0]).from_cache);
        assert_eq!(ok(&again[0]).checked_at, t0());
        assert_eq!(http.requested.len(), asked, "no network on a cache hit");

        // `force` (CheckUpdate) asks anyway.
        let forced = c.run(&mut http, &build(), true, t0() + HOUR);
        assert!(http.requested.len() > asked);
        assert!(!ok(forced.last().unwrap()).from_cache);
    }

    #[test]
    fn an_expired_or_foreign_entry_is_refreshed() {
        let dir = TempDir::new();
        let c = checker(&dir);
        let mut http = FakeHttp::released();
        c.run(&mut http, &build(), false, t0());
        let asked = http.requested.len();

        // 7 h later: the stale entry first, then the new check.
        let later = t0() + 7 * HOUR;
        let r = c.run(&mut http, &build(), false, later);
        assert_eq!(r.len(), 2);
        assert!(ok(&r[0]).from_cache);
        assert!(!ok(&r[1]).from_cache);
        assert!(http.requested.len() > asked);
        let file = load(&dir.0.join(CACHE_FILE)).unwrap();
        assert_eq!(file.checked_at_ms, ser::to_ms(later));

        // Another running build doesn't use it.
        let other = Build {
            zmk_config_version: Some("v3.1.0-2-gabcdef1".into()),
            ..build()
        };
        let mut http = FakeHttp::released();
        let r = c.run(&mut http, &other, false, later);
        assert!(r.iter().all(|r| !matches!(r, Ok(x) if x.from_cache)));
    }

    #[test]
    fn a_rate_limit_is_reported_and_leaves_the_cache_alone() {
        let dir = TempDir::new();
        let c = checker(&dir);
        c.run(&mut FakeHttp::released(), &build(), false, t0());
        let path = dir.0.join(CACHE_FILE);
        let before = std::fs::read(&path).unwrap();

        let mut limited = FakeHttp::default();
        limited.replies.insert(
            "/repos/rdlu/sofleii/releases/latest".into(),
            Reply {
                status: 403,
                body: r#"{"message":"API rate limit exceeded"}"#.into(),
                rate_limited: true,
            },
        );
        let r = c.run(&mut limited, &build(), false, t0() + 7 * HOUR);
        assert_eq!(r.len(), 2);
        assert!(ok(&r[0]).from_cache, "the last good value comes first");
        let e = r[1].as_ref().unwrap_err();
        assert_eq!(e.kind, "rate_limited");
        assert!(e.message.contains("rate limited"), "{}", e.message);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            !std::fs::read_dir(&dir.0)
                .unwrap()
                .any(|e| { e.unwrap().file_name().to_string_lossy().ends_with(".tmp") })
        );
    }

    #[test]
    fn unavailable_build_and_unreadable_cache() {
        let dir = TempDir::new();
        let c = checker(&dir);
        let r = c.run(
            &mut FakeHttp::released(),
            &Build::unavailable("old"),
            false,
            t0(),
        );
        assert_eq!(r[0].as_ref().unwrap_err().kind, "unsupported");

        std::fs::write(dir.0.join(CACHE_FILE), "{ not json").unwrap();
        let r = c.run(&mut FakeHttp::released(), &build(), false, t0());
        assert!(!ok(&r[0]).from_cache);
        assert!(load(&dir.0.join(CACHE_FILE)).is_some(), "rewritten");
    }

    #[test]
    fn cache_dir_follows_xdg() {
        // Read-only on the environment: just the shape of the answer.
        if let Some(d) = default_cache_dir() {
            assert!(d.is_absolute() && d.ends_with("sofleii"), "{}", d.display());
        }
    }
}
