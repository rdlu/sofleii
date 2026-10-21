//! One-off work off the link thread. For now only the update check:
//! [`update::check_with`]'s HTTP part against the build the link read with
//! `info` (no RPC here), plus this binary against the newest sofleii
//! release. It runs once the build is known, then every
//! [`Config::update_every`], and on [`Request::CheckUpdate`](super::Request).
//!
//! [`Checker`] is shared with `sofleii update check`, so the CLI, cron,
//! waybar and the TUI use one cache: JSON in
//! `$XDG_CACHE_HOME/sofleii/update.json` (falling back to
//! `~/.cache/sofleii/`), keyed by the repo, with a firmware part (keyed by
//! the running `zmk_config_version`) and a CLI part (the newest
//! `sofleii-v*` release; the verdict is worked out against the running
//! binary on read, so two installed versions share it), each with its own
//! timestamp and honoured for [`Config::update_ttl`]. One release list
//! answers both parts, so a refresh always renews the CLI part. A failed
//! request never replaces a good entry, and the file is replaced
//! atomically, so it is never half-written. While a stale entry is being
//! refreshed the TUI gets it first, so it has a last good value even if
//! GitHub refuses.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use super::{Config, ErrorView, Event, Outcome, Stamped, Ticket, ser};
use crate::commands::info::Build;
use crate::commands::update::{
    self, CLI_VERSION, Check, Checked, CliCheck, Http, Listed, NO_DEVICE_INFO, Reply, Running,
};
use crate::error::exit;
use crate::{Error, Result};

/// The cache file's name in the cache directory.
pub const CACHE_FILE: &str = "update.json";
/// Bumped when [`CacheFile`] changes shape; other schemas are ignored (and
/// replaced by the next check). 2 (#83): separate firmware and CLI parts.
/// 3 (v0.1.1): the firmware-inputs rule, so a cached false "behind" from
/// 0.1.0 isn't served again.
const CACHE_SCHEMA: u32 = 3;
/// How long a cached check is honoured, by the TUI and `update check` alike.
pub const UPDATE_TTL: Duration = Duration::from_secs(6 * 3600);

/// An update check as the TUI shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateReport {
    pub check: Check,
    /// This binary against the newest sofleii release.
    pub cli: CliCheck,
    /// When GitHub was asked (for a cached report, by whoever asked; the
    /// older part's time).
    #[serde(rename = "checked_at_ms", serialize_with = "ser::unix_ms")]
    pub checked_at: SystemTime,
    pub from_cache: bool,
}

/// `update.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    schema: u32,
    repo: String,
    firmware: Option<FirmwareEntry>,
    cli: Option<CliEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FirmwareEntry {
    /// The running `zmk_config_version` it was checked for.
    version: Option<String>,
    dirty: bool,
    checked_at_ms: u64,
    check: Check,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliEntry {
    checked_at_ms: u64,
    /// The newest `sofleii-v*` release; `null` when there was none.
    latest: Option<Listed>,
}

impl FirmwareEntry {
    fn matches(&self, r: &Running) -> bool {
        self.version == r.version && self.dirty == r.dirty
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

/// The update check with its cache; the jobs thread's logic and
/// `update check`'s, testable without either.
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

    /// The cache file, if it is this schema and repo's.
    fn load(&self) -> Option<CacheFile> {
        load(self.path.as_deref()?).filter(|f| f.schema == CACHE_SCHEMA && f.repo == self.repo)
    }

    fn save(&self, file: &CacheFile) {
        if let Some(p) = &self.path
            && let Err(e) = store(p, file)
        {
            log::warn!("update check: can't write {}: {e}", p.display());
        }
    }

    /// The cached answer when every part asked for is cached (the firmware
    /// part for this running build), and whether all of them are fresh at
    /// `now`. An entry from the future (the clock moved back) is stale.
    fn cached(
        &self,
        file: Option<&CacheFile>,
        firmware: Option<&Running>,
        cli: bool,
        now: SystemTime,
    ) -> Option<(Checked, bool)> {
        let fw = match firmware {
            Some(r) => Some(file?.firmware.as_ref().filter(|e| e.matches(r))?),
            None => None,
        };
        let ce = if cli { Some(file?.cli.as_ref()?) } else { None };
        let stamps: Vec<u64> = fw
            .map(|e| e.checked_at_ms)
            .into_iter()
            .chain(ce.map(|e| e.checked_at_ms))
            .collect();
        let oldest = *stamps.iter().min()?;
        let fresh = stamps.iter().all(|ms| {
            now.duration_since(ser::from_ms(*ms))
                .is_ok_and(|age| age < self.ttl)
        });
        let checked = Checked {
            firmware: fw.map(|e| e.check.clone()),
            cli: ce.map(|e| update::cli_check(CLI_VERSION, e.latest.as_ref())),
            checked_at: ser::from_ms(oldest),
            from_cache: true,
        };
        Some((checked, fresh))
    }

    /// Asks GitHub: one release list for both parts, then the firmware
    /// part's compare calls. Stores what it learnt, keeping a part that
    /// wasn't asked for; the CLI part is renewed whenever the list was read.
    /// Also says whether any reply was a rate limit.
    fn fetch(
        &self,
        http: &mut dyn Http,
        firmware: Option<&Running>,
        cli: bool,
        now: SystemTime,
    ) -> (Result<Checked>, bool) {
        let mut w = RateWatch {
            inner: http,
            limited: false,
        };
        let releases = match update::list_releases(&mut w, &self.repo) {
            Ok(r) => r,
            Err(e) => return (Err(e), w.limited),
        };
        let mut file = self.load().unwrap_or_else(|| CacheFile {
            schema: CACHE_SCHEMA,
            repo: self.repo.clone(),
            firmware: None,
            cli: None,
        });
        let latest_cli = releases.cli.first().cloned();
        file.cli = Some(CliEntry {
            checked_at_ms: ser::to_ms(now),
            latest: latest_cli.clone(),
        });
        let fw = match firmware {
            None => None,
            Some(r) => match update::check_with(&mut w, &self.repo, &releases, r.clone()) {
                Ok(check) => {
                    file.firmware = Some(FirmwareEntry {
                        version: r.version.clone(),
                        dirty: r.dirty,
                        checked_at_ms: ser::to_ms(now),
                        check: check.clone(),
                    });
                    Some(check)
                }
                Err(e) => {
                    self.save(&file);
                    return (Err(e), w.limited);
                }
            },
        };
        self.save(&file);
        let checked = Checked {
            firmware: fw,
            cli: cli.then(|| update::cli_check(CLI_VERSION, latest_cli.as_ref())),
            checked_at: now,
            from_cache: false,
        };
        (Ok(checked), w.limited)
    }

    /// `update check`: the firmware part for `firmware` (the running build,
    /// when asked for) and the CLI part when `cli`, from the cache when
    /// every part is fresh, else from GitHub. `refresh` skips the cache
    /// (the answer is still stored).
    pub fn check(
        &self,
        http: &mut dyn Http,
        firmware: Option<&Running>,
        cli: bool,
        refresh: bool,
        now: SystemTime,
    ) -> Result<Checked> {
        if !refresh && let Some((c, true)) = self.cached(self.load().as_ref(), firmware, cli, now) {
            return Ok(c);
        }
        self.fetch(http, firmware, cli, now).0
    }

    /// One TUI check for `build` at `now` (both parts): what to report, in
    /// order (a stale cached report first when GitHub is asked). `force`
    /// skips a fresh cache entry.
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
        if let Some((c, fresh)) = self.cached(self.load().as_ref(), Some(&running), true, now) {
            let report = report(c);
            if fresh && !force {
                return vec![Ok(report)];
            }
            out.push(Ok(report));
        }
        let (r, limited) = self.fetch(http, Some(&running), true, now);
        out.push(r.map(report).map_err(|e| {
            let mut v = e.view();
            if limited {
                v.kind = "rate_limited";
            }
            v
        }));
        out
    }
}

/// A both-parts [`Checked`] as the TUI's report.
fn report(c: Checked) -> UpdateReport {
    let (Some(check), Some(cli)) = (c.firmware, c.cli) else {
        unreachable!("the TUI always asks for both parts")
    };
    UpdateReport {
        check,
        cli,
        checked_at: c.checked_at,
        from_cache: c.from_cache,
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
                result: r.map(|u| Outcome::Update(Box::new(u))),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::update::tests::{FakeHttp, LIST, RELEASE_SHA};
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
        assert_eq!(file.firmware.unwrap().checked_at_ms, ser::to_ms(later));

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
            LIST.into(),
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

    fn running() -> Running {
        update::running_from(&build())
    }

    /// A list with a sofleii release newer than this binary.
    fn with_cli_release() -> FakeHttp {
        FakeHttp::with_releases(&[("sofleii-v99.0.0", false, false), ("v3.1.0", false, false)])
    }

    #[test]
    fn update_check_hits_misses_and_refreshes() {
        let dir = TempDir::new();
        let c = checker(&dir);
        let r = running();

        // Miss: GitHub, both parts, stored.
        let mut http = with_cli_release();
        let first = c.check(&mut http, Some(&r), true, false, t0()).unwrap();
        assert!(!first.from_cache);
        assert_eq!(first.firmware.unwrap().status, update::Status::UpToDate);
        let cli = first.cli.unwrap();
        assert_eq!(cli.status, update::Status::Behind);
        assert_eq!(cli.latest.as_deref(), Some("99.0.0"));
        let asked = http.requested.len();
        assert_eq!(http.requested.iter().filter(|p| *p == LIST).count(), 1);

        // Hit: no network.
        let hit = c
            .check(&mut http, Some(&r), true, false, t0() + HOUR)
            .unwrap();
        assert!(hit.from_cache);
        assert_eq!(hit.checked_at, t0());
        assert_eq!(hit.cli.unwrap().status, update::Status::Behind);
        assert_eq!(http.requested.len(), asked);

        // --refresh: GitHub again, and the cache takes the new time.
        let fresh = c
            .check(&mut http, Some(&r), true, true, t0() + HOUR)
            .unwrap();
        assert!(!fresh.from_cache);
        assert!(http.requested.len() > asked);
        let file = load(&dir.0.join(CACHE_FILE)).unwrap();
        assert_eq!(
            file.firmware.unwrap().checked_at_ms,
            ser::to_ms(t0() + HOUR)
        );

        // Expired: GitHub.
        let n = http.requested.len();
        let late = c
            .check(&mut http, Some(&r), true, false, t0() + 8 * HOUR)
            .unwrap();
        assert!(!late.from_cache);
        assert!(http.requested.len() > n);
    }

    #[test]
    fn the_parts_share_one_file() {
        let dir = TempDir::new();
        let c = checker(&dir);
        let r = running();

        // `update check --cli`: one request (the list), no firmware part.
        let mut http = FakeHttp::released();
        let cli = c.check(&mut http, None, true, false, t0()).unwrap();
        assert_eq!(http.requested, [LIST]);
        assert!(cli.firmware.is_none());
        let cli = cli.cli.unwrap();
        assert_eq!(cli.status, update::Status::Unknown);
        assert_eq!(cli.exit_code(), 0);
        assert!(load(&dir.0.join(CACHE_FILE)).unwrap().firmware.is_none());

        // `--firmware` next: its part is missing, so GitHub; the CLI part
        // is renewed from the same list.
        let mut http = FakeHttp::released();
        let fw = c
            .check(&mut http, Some(&r), false, false, t0() + HOUR)
            .unwrap();
        assert!(!fw.from_cache && fw.cli.is_none());
        let file = load(&dir.0.join(CACHE_FILE)).unwrap();
        assert_eq!(file.cli.unwrap().checked_at_ms, ser::to_ms(t0() + HOUR));

        // Now the TUI (both parts) and `--cli` hit the cache.
        let mut http = FakeHttp::released();
        let both = c.run(&mut http, &build(), false, t0() + 2 * HOUR);
        assert_eq!(both.len(), 1);
        assert!(ok(&both[0]).from_cache);
        assert_eq!(ok(&both[0]).cli.status, update::Status::Unknown);
        assert!(
            c.check(&mut http, None, true, false, t0() + 2 * HOUR)
                .unwrap()
                .from_cache
        );
        assert!(http.requested.is_empty());

        // Another running build misses the firmware part only.
        let other = Running {
            version: Some("v3.1.0-2-gabcdef1".into()),
            ..r.clone()
        };
        assert!(
            c.cached(c.load().as_ref(), Some(&other), false, t0())
                .is_none()
        );
        assert!(c.cached(c.load().as_ref(), None, true, t0()).is_some());
    }

    #[test]
    fn a_failed_firmware_check_keeps_its_last_entry() {
        let dir = TempDir::new();
        let c = checker(&dir);
        let r = running();
        c.check(&mut FakeHttp::released(), Some(&r), true, false, t0())
            .unwrap();
        // The list works, the tag lookup fails.
        let mut broken = FakeHttp::released();
        broken.replies.remove("/repos/rdlu/sofleii/commits/v3.1.0");
        let later = t0() + 7 * HOUR;
        assert!(c.check(&mut broken, Some(&r), true, false, later).is_err());
        let file = load(&dir.0.join(CACHE_FILE)).unwrap();
        assert_eq!(file.firmware.unwrap().checked_at_ms, ser::to_ms(t0()));
        assert_eq!(file.cli.unwrap().checked_at_ms, ser::to_ms(later));
    }

    #[test]
    fn a_schema_1_file_is_ignored_and_replaced() {
        let dir = TempDir::new();
        let path = dir.0.join(CACHE_FILE);
        std::fs::write(
            &path,
            r#"{"schema":1,"repo":"rdlu/sofleii","version":"5c53b30","dirty":false,
                "checked_at_ms":1800000000000,"check":{}}"#,
        )
        .unwrap();
        let c = checker(&dir);
        let mut http = FakeHttp::released();
        let r = c
            .check(&mut http, Some(&running()), true, false, t0())
            .unwrap();
        assert!(!r.from_cache);
        assert_eq!(load(&path).unwrap().schema, CACHE_SCHEMA);
    }

    #[test]
    fn cache_dir_follows_xdg() {
        // Read-only on the environment: just the shape of the answer.
        if let Some(d) = default_cache_dir() {
            assert!(d.is_absolute() && d.ends_with("sofleii"), "{}", d.display());
        }
    }
}
