//! `sofleii update check` (#47, #83): the running firmware's
//! `zmk_config_version` (from `zmk__device_info`) against the newest
//! firmware release of the repo, and this binary's version against the
//! newest sofleii release. Both come from one release list
//! ([`list_releases`]): firmware releases are tagged `v*`, sofleii's own
//! `sofleii-v*` (published with `--latest=false`, so GitHub's "latest"
//! stays the firmware; this module never asks for `releases/latest`).
//! HTTP goes through the [`Http`] trait (the real client is
//! [`crate::github`]), so this module has no network code and its tests run
//! on canned JSON.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::info::{self, BUILD_EMPTY_NOTE, Build};
use crate::client::Client;
use crate::error::exit;
use crate::transport::Transport;
use crate::{Error, Result};

pub const DEFAULT_REPO: &str = "rdlu/sofleii";

/// The REST API's own media type.
pub const ACCEPT_JSON: &str = "application/vnd.github+json";
/// Makes `GET /repos/{repo}/commits/{ref}` answer with just the 40-hex SHA
/// instead of the whole commit, files included.
pub const ACCEPT_SHA: &str = "application/vnd.github.sha";

/// Why `update check` refuses firmware without the subsystem.
pub const NO_DEVICE_INFO: &str = "no zmk__device_info subsystem, so the running build can't \
     be read (firmware before v3.1.0 / #46); flash a newer build";

/// One HTTP response: any status is a reply, only network failures are errors.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    /// 429, or 403 with `x-ratelimit-remaining: 0`.
    pub rate_limited: bool,
}

/// `GET https://api.github.com{path}` with the given `Accept` header.
pub trait Http {
    fn get(&mut self, path: &str, accept: &str) -> Result<Reply>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Running {
    /// `zmk_config_version` as the firmware reports it (`git describe
    /// --dirty --always --tags`), `null` when empty.
    pub version: Option<String>,
    /// The commit the build came from: the abbreviated SHA in `version`, or
    /// the full SHA an exact tag resolved to; `null` when unknown.
    pub commit: Option<String>,
    /// Built with uncommitted changes on top of `commit`.
    pub dirty: bool,
    /// The release this build is, or comes after; `null` when no release
    /// precedes it (or it couldn't be mapped). The firmware can't know its
    /// own tag: CI builds the merge commit before it's tagged, from a
    /// shallow checkout, so it only ever reports a bare SHA.
    pub release: Option<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    /// The build is that release: the same commit, or (`equivalent`) one
    /// whose firmware inputs are the same.
    pub exact: bool,
    /// Commits on top of `tag`; 0 when `exact`. Shown as `tag+N`.
    pub commits_after: u32,
    /// `exact` by the firmware inputs only: the build's commit isn't the
    /// tag's, but nothing under [`FIRMWARE_INPUTS`] differs (a PR-branch
    /// build of a merged PR, a docs-only commit). Text shows it as
    /// `v3.2.0 (built from 0858966)`.
    #[serde(default)]
    pub equivalent: bool,
}

impl Release {
    fn exact(tag: &str) -> Self {
        Release {
            tag: tag.into(),
            exact: true,
            commits_after: 0,
            equivalent: false,
        }
    }

    fn equivalent(tag: &str) -> Self {
        Release {
            equivalent: true,
            ..Release::exact(tag)
        }
    }

    fn after(tag: &str, n: u32) -> Self {
        Release {
            tag: tag.into(),
            exact: n == 0,
            commits_after: n,
            equivalent: false,
        }
    }

    /// `v3.1.0`, or `v3.0.0+68`.
    pub fn label(&self) -> String {
        if self.exact {
            self.tag.clone()
        } else {
            format!("{}+{}", self.tag, self.commits_after)
        }
    }
}

/// The paths a firmware build reads: exactly the `paths:` that trigger
/// `.github/workflows/build.yml` (keep the two in sync). A directory ends
/// in `/`; anything else is one file. The workflow lists itself too, and
/// it stays here: it pins the reusable ZMK workflow, hence the build
/// container and toolchain.
pub const FIRMWARE_INPUTS: &[&str] = &[
    "config/",
    "build.yaml",
    "zmk-rgb-layer/",
    ".github/workflows/build.yml",
];

/// Whether a repo path is one of the [`FIRMWARE_INPUTS`].
pub fn is_firmware_input(path: &str) -> bool {
    FIRMWARE_INPUTS.iter().any(|p| match p.strip_suffix('/') {
        Some(dir) => path.strip_prefix(dir).is_some_and(|r| r.starts_with('/')),
        None => path == *p,
    })
}

/// GitHub's compare lists at most this many changed files. A list this
/// long may be cut short, so it can't prove that no firmware input changed.
pub const COMPARE_FILE_CAP: usize = 300;

/// How many older releases a `behind` bare SHA may compare against.
pub const RELEASE_SCAN: u32 = 10;

/// Releases per page of the one list request. GitHub lists newest first,
/// and both tag families fit in one page for years to come (19 releases
/// on 2026-10-19).
pub const RELEASE_PAGE: u32 = 50;

/// Tag prefix of the firmware releases (`v3.1.0`).
pub const FIRMWARE_TAG_PREFIX: &str = "v";
/// Tag prefix of sofleii's own releases (`sofleii-v0.1.0`).
pub const CLI_TAG_PREFIX: &str = "sofleii-v";
/// This binary's version (`Cargo.toml`).
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The one request both checks share.
pub fn release_list_path(repo: &str) -> String {
    format!("/repos/{repo}/releases?per_page={RELEASE_PAGE}")
}

/// A semantic version (`3.1.0`, `0.2.0-rc.1`); build metadata is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The pre-release part after `-`, if any.
    pub pre: Option<String>,
}

impl Version {
    /// `MAJOR.MINOR.PATCH[-PRE][+BUILD]`, nothing else.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = match s.split_once('-') {
            Some((_, "")) => return None,
            Some((c, p)) => (c, Some(p.to_string())),
            None => (s, None),
        };
        let mut it = core.split('.');
        let mut num = || -> Option<u64> {
            let p = it.next()?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let (major, minor, patch) = (num()?, num()?, num()?);
        if it.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (&self.pre, &o.pre) {
                (None, None) => Ordering::Equal,
                // A pre-release sorts before its release.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => cmp_pre(a, b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// semver's pre-release precedence: dot-separated identifiers, numeric
/// ones numerically and below alphanumeric ones, then the longer wins.
fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        let o = match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) => match (p.parse::<u64>(), q.parse::<u64>()) {
                (Ok(m), Ok(n)) => m.cmp(&n),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => p.cmp(q),
            },
        };
        if o != Ordering::Equal {
            return o;
        }
    }
}

/// One published release from the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub tag: String,
    /// ISO 8601, UTC, as GitHub gives it.
    pub published_at: Option<String>,
}

/// The repo's published releases (no drafts, no prereleases), split by tag
/// family, each newest first by semver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Releases {
    /// `v*` tags (never `sofleii-v*`).
    pub firmware: Vec<Listed>,
    /// `sofleii-v*` tags.
    pub cli: Vec<Listed>,
}

impl Releases {
    /// Splits GitHub's list. A tag that isn't semver after its prefix, or
    /// belongs to neither family (`canary-zephyr-4.1`), is skipped.
    fn from_list(list: Vec<ReleaseListJson>) -> Releases {
        let mut fw = Vec::new();
        let mut cli = Vec::new();
        for r in list {
            if r.draft || r.prerelease {
                continue;
            }
            let (bucket, rest) = if let Some(v) = r.tag_name.strip_prefix(CLI_TAG_PREFIX) {
                (&mut cli, v)
            } else if let Some(v) = r.tag_name.strip_prefix(FIRMWARE_TAG_PREFIX) {
                (&mut fw, v)
            } else {
                continue;
            };
            if let Some(v) = Version::parse(rest) {
                let listed = Listed {
                    tag: r.tag_name.clone(),
                    published_at: r.published_at,
                };
                bucket.push((v, listed));
            }
        }
        // Stable: equal versions keep GitHub's (newest-first) order.
        let sorted = |mut v: Vec<(Version, Listed)>| -> Vec<Listed> {
            v.sort_by(|a, b| b.0.cmp(&a.0));
            v.into_iter().map(|(_, l)| l).collect()
        };
        Releases {
            firmware: sorted(fw),
            cli: sorted(cli),
        }
    }
}

/// `GET /repos/{repo}/releases`, once, for both checks.
pub fn list_releases<H: Http + ?Sized>(http: &mut H, repo: &str) -> Result<Releases> {
    let r = http.get(&release_list_path(repo), ACCEPT_JSON)?;
    if r.status == 404 && !r.rate_limited {
        return Err(Error::GitHub(format!(
            "no published release found for {repo} (HTTP 404: no such repo, \
             or a private repo without a token)"
        )));
    }
    let list: Vec<ReleaseListJson> = parse(&ok(r, "release list")?, "release list")?;
    Ok(Releases::from_list(list))
}

/// This binary against the newest sofleii release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliCheck {
    /// The running binary's version, e.g. `0.1.0`.
    pub version: String,
    /// The newest release's version (its tag without `sofleii-v`); `null`
    /// when there is none.
    pub latest: Option<String>,
    /// Its tag, e.g. `sofleii-v0.1.0`.
    pub tag: Option<String>,
    pub published_at: Option<String>,
    /// `unknown` when there is no release to compare with.
    pub status: Status,
    pub note: Option<String>,
}

impl CliCheck {
    /// 1 only when a newer sofleii is out: no release yet is not a failure.
    pub fn exit_code(&self) -> u8 {
        match self.status {
            Status::Behind => exit::CHECK_FAILED,
            Status::UpToDate | Status::Ahead | Status::Unknown => exit::OK,
        }
    }
}

/// The parts of an update check that were asked for (`session::jobs`'s
/// `Checker` answers it, from its cache or GitHub).
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    /// The running firmware against the newest `v*` release.
    pub firmware: Option<Check>,
    /// This binary against the newest `sofleii-v*` release.
    pub cli: Option<CliCheck>,
    /// When GitHub was asked; for a cached answer, the older part's time.
    pub checked_at: std::time::SystemTime,
    pub from_cache: bool,
}

impl Checked {
    /// 1 if a checked part needs doing (see [`Status::exit_code`] and
    /// [`CliCheck::exit_code`]), else 0.
    pub fn exit_code(&self) -> u8 {
        let fw = self
            .firmware
            .as_ref()
            .map_or(exit::OK, |c| c.status.exit_code());
        let cli = self.cli.as_ref().map_or(exit::OK, CliCheck::exit_code);
        fw.max(cli)
    }
}

/// Why the CLI part is `unknown` with no `sofleii-v*` release.
pub const NO_CLI_RELEASE: &str = "no sofleii release yet (no sofleii-v* tag published)";

/// `current` (a semver string, normally [`CLI_VERSION`]) against `latest`,
/// the newest `sofleii-v*` release (the first of [`Releases::cli`]).
pub fn cli_check(current: &str, latest: Option<&Listed>) -> CliCheck {
    let mut c = CliCheck {
        version: current.to_string(),
        latest: None,
        tag: None,
        published_at: None,
        status: Status::Unknown,
        note: None,
    };
    let Some(l) = latest else {
        c.note = Some(NO_CLI_RELEASE.into());
        return c;
    };
    let v = l.tag.strip_prefix(CLI_TAG_PREFIX).unwrap_or(&l.tag);
    c.latest = Some(v.to_string());
    c.tag = Some(l.tag.clone());
    c.published_at = l.published_at.clone();
    match (Version::parse(current), Version::parse(v)) {
        (Some(mine), Some(theirs)) => {
            c.status = match mine.cmp(&theirs) {
                Ordering::Less => Status::Behind,
                Ordering::Equal => Status::UpToDate,
                Ordering::Greater => Status::Ahead,
            };
        }
        _ => c.note = Some(format!("can't compare {current:?} with {v:?} as semver")),
    }
    c
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latest {
    pub tag: String,
    /// Full SHA of the commit the tag points at.
    pub commit: String,
    /// ISO 8601, UTC, as GitHub gives it.
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The release's commit, or one with the same source tree (a PR-branch
    /// build that was merged as is: the counts then show the merge commit).
    UpToDate,
    /// The release has commits the keyboard doesn't: an update is available.
    /// That includes GitHub's `diverged` (a branch build off an older
    /// release): the version decides, so `v3.0.0+9` is behind `v3.1.0`, and
    /// `commits_ahead` plus a note keep the unreleased commits visible.
    Behind,
    /// The keyboard runs commits past the release (a newer main build).
    Ahead,
    /// No commit to compare (unknown or untagged version, or not on GitHub).
    Unknown,
}

impl Status {
    /// 0 when nothing needs doing, 1 for an update or an unknown build.
    pub fn exit_code(self) -> u8 {
        match self {
            Status::UpToDate | Status::Ahead => exit::OK,
            Status::Behind | Status::Unknown => exit::CHECK_FAILED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub running: Running,
    pub latest: Latest,
    pub status: Status,
    /// Commits in the release that the keyboard lacks; `null` when unknown.
    pub commits_behind: Option<u32>,
    /// Commits on the keyboard that the release lacks; `null` when unknown.
    pub commits_ahead: Option<u32>,
    /// Files that differ between the build's commit and the release's
    /// (for a diverged build: changed on either side since the merge base);
    /// 0 for the same commit, `null` when not compared (`unknown`) or when
    /// GitHub's file list was missing or possibly cut short.
    #[serde(default)]
    pub files_changed: Option<u32>,
    /// How many of those are [`FIRMWARE_INPUTS`]: 0 means the running
    /// firmware is the release's, whatever the commits say. `null` as above.
    #[serde(default)]
    pub firmware_files_changed: Option<u32>,
    /// Why the status is `unknown`, why a build from another commit counts
    /// as up to date, why the file list couldn't decide, or why
    /// `running.release` couldn't be looked up; else `null`.
    pub note: Option<String>,
}

impl Check {
    fn add_note(&mut self, n: String) {
        self.note = Some(match self.note.take() {
            Some(prev) => format!("{prev}; {n}"),
            None => n,
        });
    }
}

/// What a `git describe --dirty --always --tags` string names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Describe {
    /// Empty: nothing to go on (the old CI bug).
    Unknown,
    /// `v3.0.0-68-gf9a665f` or a bare `f9a665f`.
    Commit(String),
    /// Exactly on a tag (`v3.1.0`): no SHA in the string.
    Tag(String),
}

/// Splits off `-dirty`, then reads `<tag>-<n>-g<sha>`, a bare SHA, or a tag.
pub fn parse_describe(raw: &str) -> (Describe, bool) {
    let (s, dirty) = match raw.strip_suffix("-dirty") {
        Some(s) => (s, true),
        None => (raw, false),
    };
    if s.is_empty() {
        return (Describe::Unknown, dirty);
    }
    let is_sha =
        |h: &str| h.len() >= 4 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let mut parts = s.rsplitn(3, '-');
    if let (Some(g), Some(n), Some(_tag)) = (parts.next(), parts.next(), parts.next())
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
        && let Some(sha) = g.strip_prefix('g')
        && is_sha(sha)
    {
        return (Describe::Commit(sha.to_string()), dirty);
    }
    if is_sha(s) {
        return (Describe::Commit(s.to_string()), dirty);
    }
    (Describe::Tag(s.to_string()), dirty)
}

/// The release a local `git describe` names: `v3.0.0-68-gf9a665f` is
/// 68 commits after v3.0.0, `v3.1.0` is v3.1.0. `None` for a bare SHA (CI).
pub fn describe_release(raw: &str) -> Option<Release> {
    let s = raw.strip_suffix("-dirty").unwrap_or(raw);
    match parse_describe(raw).0 {
        Describe::Unknown => None,
        Describe::Tag(t) => Some(Release::exact(&t)),
        Describe::Commit(_) => {
            let mut parts = s.rsplitn(3, '-');
            let (_g, n, tag) = (parts.next()?, parts.next()?, parts.next()?);
            Some(Release::after(tag, n.parse().ok()?))
        }
    }
}

/// `Running` from `info`'s build; the tag form's commit is filled in later.
pub fn running_from(build: &Build) -> Running {
    let version = build.zmk_config_version.clone();
    let (describe, dirty) = parse_describe(version.as_deref().unwrap_or(""));
    Running {
        commit: match describe {
            Describe::Commit(c) => Some(c),
            Describe::Unknown | Describe::Tag(_) => None,
        },
        dirty: dirty || build.zmk_config_dirty == Some(true),
        release: None,
        version,
    }
}

/// Reads the running build from the keyboard, then asks GitHub. The
/// keyboard goes first, so "not found" exits 2 without touching the network.
pub fn run<T: Transport, H: Http>(
    client: &mut Client<T>,
    http: &mut H,
    repo: &str,
) -> Result<Check> {
    let running = read_running(client)?;
    check(http, repo, running)
}

/// The running build from `zmk__device_info`; `unsupported` without it.
pub fn read_running<T: Transport>(client: &mut Client<T>) -> Result<Running> {
    let subsystems = client.list_custom_subsystems()?;
    match info::fetch_build(client, &subsystems)? {
        Some((b, _)) if b.available => Ok(running_from(&b)),
        Some(_) => Err(Error::Unsupported(BUILD_EMPTY_NOTE.into())),
        None => Err(Error::Unsupported(NO_DEVICE_INFO.into())),
    }
}

#[derive(Deserialize)]
struct ReleaseListJson {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
}

#[derive(Deserialize)]
struct CompareJson {
    status: String,
    ahead_by: u32,
    behind_by: u32,
    /// Changed files from the merge base to the head (first page only, at
    /// most [`COMPARE_FILE_CAP`]); `[]` = the head adds no file changes.
    files: Option<Vec<FileJson>>,
}

#[derive(Deserialize)]
struct FileJson {
    filename: String,
    /// Set on a rename: the path it came from.
    previous_filename: Option<String>,
}

fn touches_firmware(f: &FileJson) -> bool {
    is_firmware_input(&f.filename)
        || f.previous_filename
            .as_deref()
            .is_some_and(is_firmware_input)
}

/// The changed files GitHub listed, or why the list can't be trusted to
/// be complete.
fn complete_files(files: Option<Vec<FileJson>>) -> std::result::Result<Vec<FileJson>, String> {
    match files {
        None => Err("GitHub's compare listed no files".into()),
        Some(f) if f.len() >= COMPARE_FILE_CAP => Err(format!(
            "GitHub lists at most {COMPARE_FILE_CAP} files per compare and this one has {}, \
             so the list may be cut short",
            f.len()
        )),
        Some(f) => Ok(f),
    }
}

/// Changed paths, and which of them touch a firmware input (a rename
/// counts when either side does).
#[derive(Default)]
struct Changed {
    files: BTreeSet<String>,
    firmware: BTreeSet<String>,
}

impl Changed {
    fn add(&mut self, files: Vec<FileJson>) {
        for f in files {
            if touches_firmware(&f) {
                self.firmware.insert(f.filename.clone());
            }
            self.files.insert(f.filename);
        }
    }
}

/// [`list_releases`], then [`check_with`].
pub fn check<H: Http + ?Sized>(http: &mut H, repo: &str, running: Running) -> Result<Check> {
    let releases = list_releases(http, repo)?;
    check_with(http, repo, &releases, running)
}

/// Compares `running` with the newest firmware release of `repo`
/// (`OWNER/NAME`) in `releases`, then names the release it is or follows
/// (`running.release`), cheapest way first: the verdict itself, then the
/// describe string, and only for a bare SHA that's behind, a scan of older
/// releases from the same list.
pub fn check_with<H: Http + ?Sized>(
    http: &mut H,
    repo: &str,
    releases: &Releases,
    running: Running,
) -> Result<Check> {
    let mut c = compare_latest(http, repo, releases, running)?;
    let described = c.running.version.as_deref().and_then(describe_release);
    let tag = &c.latest.tag;
    c.running.release = match c.status {
        // The same commit; else a build that differs only outside the
        // firmware inputs (the compare counted commits).
        Status::UpToDate if c.commits_behind == Some(0) && c.commits_ahead == Some(0) => {
            Some(Release::exact(tag))
        }
        Status::UpToDate => Some(Release::equivalent(tag)),
        Status::Ahead => Some(Release::after(tag, c.commits_ahead.unwrap_or(0))),
        Status::Unknown => described,
        Status::Behind => match (described, c.running.commit.clone()) {
            (Some(r), _) => Some(r),
            (None, Some(commit)) => match scan_releases(http, repo, releases, tag, &commit) {
                Ok(r) => r,
                Err(e) => {
                    c.add_note(format!("couldn't map {} to a release: {e}", short(&commit)));
                    None
                }
            },
            (None, None) => None,
        },
    };
    Ok(c)
}

/// The newest older firmware release that `commit` is (same commit, or
/// the same firmware inputs) or comes after: takes up to [`RELEASE_SCAN`]
/// of `releases` other than `latest_tag` (already compared), and compares
/// each `{tag}...{commit}` newest-first until one fits. Only GitHub's
/// `ahead` fits: then the merge base is the tag, so the file list is the
/// whole tag-to-build diff (a `behind` or `diverged` list says nothing
/// about the build's side).
fn scan_releases<H: Http + ?Sized>(
    http: &mut H,
    repo: &str,
    releases: &Releases,
    latest_tag: &str,
    commit: &str,
) -> Result<Option<Release>> {
    let older = releases.firmware.iter().filter(|r| r.tag != latest_tag);
    for rel in older.take(RELEASE_SCAN as usize) {
        let r = http.get(
            &format!(
                "/repos/{repo}/compare/{}...{commit}?per_page=1",
                encode_ref(&rel.tag)
            ),
            ACCEPT_JSON,
        )?;
        if matches!(r.status, 404 | 422) && !r.rate_limited {
            continue;
        }
        let cmp: CompareJson = parse(&ok(r, "compare")?, "compare")?;
        // Here the build is the head: GitHub's `ahead` = commits after the tag.
        match cmp.status.as_str() {
            "identical" => return Ok(Some(Release::exact(&rel.tag))),
            "ahead" => {
                let firmware_same =
                    complete_files(cmp.files).is_ok_and(|f| !f.iter().any(touches_firmware));
                return Ok(Some(if firmware_same {
                    Release::equivalent(&rel.tag)
                } else {
                    Release::after(&rel.tag, cmp.ahead_by)
                }));
            }
            _ => {}
        }
    }
    Ok(None)
}

fn compare_latest<H: Http + ?Sized>(
    http: &mut H,
    repo: &str,
    releases: &Releases,
    mut running: Running,
) -> Result<Check> {
    let release = releases.firmware.first().ok_or_else(|| {
        Error::GitHub(format!(
            "no published firmware release (a {FIRMWARE_TAG_PREFIX}X.Y.Z tag) found for {repo}"
        ))
    })?;
    let tag = release.tag.clone();
    let latest_sha = resolve(http, repo, &tag)?.ok_or_else(|| {
        Error::GitHub(format!(
            "release tag {tag} doesn't resolve to a commit (HTTP 404)"
        ))
    })?;
    let latest = Latest {
        tag: tag.clone(),
        commit: latest_sha.clone(),
        published_at: release.published_at.clone(),
    };

    let mut note = None;
    if running.commit.is_none() {
        match parse_describe(running.version.as_deref().unwrap_or("")).0 {
            Describe::Tag(t) if t == tag => running.commit = Some(latest_sha.clone()),
            Describe::Tag(t) => match resolve(http, repo, &t)? {
                Some(sha) => running.commit = Some(sha),
                None => note = Some(format!("tag {t} is not on {repo} (a local tag?)")),
            },
            Describe::Unknown => {
                note = Some("the firmware reports no zmk_config_version".into());
            }
            Describe::Commit(c) => running.commit = Some(c),
        }
    }
    let Some(commit) = running.commit.clone() else {
        return Ok(unknown(running, latest, note));
    };

    if latest_sha.starts_with(&commit.to_ascii_lowercase()) {
        return Ok(Check {
            running,
            latest,
            status: Status::UpToDate,
            commits_behind: Some(0),
            commits_ahead: Some(0),
            files_changed: Some(0),
            firmware_files_changed: Some(0),
            note: None,
        });
    }
    // per_page=1: only the counts are needed, not the commit list.
    let r = http.get(
        &format!("/repos/{repo}/compare/{commit}...{latest_sha}?per_page=1"),
        ACCEPT_JSON,
    )?;
    if matches!(r.status, 404 | 422) && !r.rate_limited {
        let note = format!("commit {commit} is not on {repo} (an unpushed local build?)");
        return Ok(unknown(running, latest, Some(note)));
    }
    let cmp: CompareJson = parse(&ok(r, "compare")?, "compare")?;
    let (status, behind, ahead) = from_compare(&cmp.status, cmp.ahead_by, cmp.behind_by)?;
    let mut c = Check {
        running,
        latest,
        status,
        commits_behind: Some(behind),
        commits_ahead: Some(ahead),
        files_changed: None,
        firmware_files_changed: None,
        note: None,
    };
    if cmp.status == "identical" {
        (c.files_changed, c.firmware_files_changed) = (Some(0), Some(0));
        return Ok(c);
    }
    // GitHub lists the files from the merge base to the head. That is the
    // release's side, empty when the keyboard is ahead (the merge base is
    // the release). The build's side needs the reverse compare, except when
    // the keyboard is behind (the merge base is the build).
    let mut changed = Changed::default();
    let mut undecided = None;
    if matches!(cmp.status.as_str(), "ahead" | "diverged") {
        match complete_files(cmp.files) {
            Ok(f) => changed.add(f),
            Err(why) => undecided = Some(why),
        }
    }
    if undecided.is_none() && matches!(cmp.status.as_str(), "behind" | "diverged") {
        match build_side_files(http, repo, &c.latest.commit, &commit) {
            Ok(f) => changed.add(f),
            Err(why) => undecided = Some(why),
        }
    }
    match undecided {
        None => {
            let n = changed.files.len();
            c.files_changed = Some(n as u32);
            c.firmware_files_changed = Some(changed.firmware.len() as u32);
            // A PR-branch build of a merged PR, or docs-only commits: the
            // firmware is the release's, so there's nothing to flash.
            if changed.firmware.is_empty() {
                c.status = Status::UpToDate;
                let (built, tag) = (short(&commit), &c.latest.tag);
                c.note = Some(if n == 0 {
                    format!(
                        "built from {built}; same source as {tag}: the commits in between change no files"
                    )
                } else {
                    format!(
                        "built from {built}; differs from {tag} only outside the firmware inputs ({})",
                        plural(n, "file")
                    )
                });
            }
        }
        Some(why) => {
            c.note = Some(format!(
                "can't tell whether a firmware input changed: {why}; going by the commits"
            ));
        }
    }
    // GitHub's `diverged`, folded into behind: say what the release lacks.
    if c.status == Status::Behind && ahead > 0 {
        let n = format!(
            "also has {} not in {} (unreleased changes)",
            plural(ahead as usize, "commit"),
            c.latest.tag
        );
        c.add_note(n);
    }
    Ok(c)
}

/// `1 file`, `35 files`.
fn plural(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// The files the build's own commits change since its merge base with the
/// release: the reverse compare `latest...commit`. A failure (rate limit
/// included) is a reason, not an error: the verdict then goes by commits.
fn build_side_files<H: Http + ?Sized>(
    http: &mut H,
    repo: &str,
    latest_sha: &str,
    commit: &str,
) -> std::result::Result<Vec<FileJson>, String> {
    let r = http
        .get(
            &format!("/repos/{repo}/compare/{latest_sha}...{commit}?per_page=1"),
            ACCEPT_JSON,
        )
        .map_err(|e| e.to_string())?;
    let body = ok(r, "reverse compare").map_err(|e| e.to_string())?;
    let cmp: CompareJson = parse(&body, "reverse compare").map_err(|e| e.to_string())?;
    complete_files(cmp.files)
}

/// GitHub's compare of `base=running...head=latest`, seen from the keyboard.
/// GitHub's `ahead` means the *head* (the release) is ahead, so the keyboard
/// is **behind** by `ahead_by`, and the other way round.
/// Returns `(status, commits_behind, commits_ahead)`.
pub fn from_compare(status: &str, ahead_by: u32, behind_by: u32) -> Result<(Status, u32, u32)> {
    let s = match status {
        "identical" => Status::UpToDate,
        "ahead" => Status::Behind,
        "behind" => Status::Ahead,
        "diverged" => Status::Behind,
        other => {
            return Err(Error::GitHub(format!(
                "unexpected compare status {other:?}"
            )));
        }
    };
    Ok((s, ahead_by, behind_by))
}

/// The 7-character abbreviation git and GitHub show.
pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

fn unknown(running: Running, latest: Latest, note: Option<String>) -> Check {
    Check {
        running,
        latest,
        status: Status::Unknown,
        commits_behind: None,
        commits_ahead: None,
        files_changed: None,
        firmware_files_changed: None,
        note,
    }
}

/// A tag's (or any ref's) full commit SHA; `None` on 404.
fn resolve<H: Http + ?Sized>(http: &mut H, repo: &str, git_ref: &str) -> Result<Option<String>> {
    let r = http.get(
        &format!("/repos/{repo}/commits/{}", encode_ref(git_ref)),
        ACCEPT_SHA,
    )?;
    if matches!(r.status, 404 | 422) && !r.rate_limited {
        return Ok(None);
    }
    let body = ok(r, "commit lookup")?;
    let sha = body.trim();
    if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(Some(sha.to_ascii_lowercase()))
    } else {
        Err(Error::GitHub(format!(
            "commit lookup for {git_ref}: expected a SHA, got {:?}",
            sha.chars().take(60).collect::<String>()
        )))
    }
}

/// The body of a 2xx reply, or an error naming the request and the cause.
fn ok(r: Reply, what: &str) -> Result<String> {
    if r.rate_limited {
        return Err(Error::GitHub(format!(
            "{what}: rate limited (HTTP {}); set GITHUB_TOKEN or GH_TOKEN for a higher limit",
            r.status
        )));
    }
    if r.status == 401 {
        return Err(Error::GitHub(format!(
            "{what}: HTTP 401, the token in GH_TOKEN/GITHUB_TOKEN was rejected"
        )));
    }
    if !(200..300).contains(&r.status) {
        #[derive(Deserialize)]
        struct Message {
            message: String,
        }
        let msg = serde_json::from_str::<Message>(&r.body)
            .map(|m| format!(": {}", m.message))
            .unwrap_or_default();
        return Err(Error::GitHub(format!("{what}: HTTP {}{msg}", r.status)));
    }
    Ok(r.body)
}

fn parse<T: for<'de> Deserialize<'de>>(body: &str, what: &str) -> Result<T> {
    serde_json::from_str(body).map_err(|e| Error::GitHub(format!("{what}: unexpected JSON: {e}")))
}

/// Percent-encodes a ref for a URL path (tags are usually plain `v3.1.0`).
fn encode_ref(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::client::ResponseSubsystem;
    use crate::commands::info::tests::{device_info_reply, sample_response, subsystem};
    use crate::proto::{custom, device_info as di};
    use crate::transport::mock::Mock;
    use std::collections::HashMap;
    use std::time::Duration;

    pub const RELEASE_SHA: &str = "5c53b30fce99031611442b1a3ccbbe7052327020";

    /// Canned replies by path; an unknown path is a 404 (and is recorded).
    #[derive(Default)]
    pub struct FakeHttp {
        pub replies: HashMap<String, Reply>,
        pub requested: Vec<String>,
    }

    impl FakeHttp {
        /// `rdlu/sofleii` with v3.1.0 at [`RELEASE_SHA`] as the newest
        /// firmware release, and no sofleii release.
        pub fn released() -> Self {
            Self::with_releases(&[("v3.1.0", false, false)])
        }

        /// Like [`released`](Self::released) (v3.1.0 resolves to
        /// [`RELEASE_SHA`]), with this release list.
        pub fn with_releases(tags: &[(&str, bool, bool)]) -> Self {
            let mut f = Self::default();
            f.reply(LIST, 200, &release_list(tags));
            f.reply("/repos/rdlu/sofleii/commits/v3.1.0", 200, RELEASE_SHA);
            f
        }

        pub fn reply(&mut self, path: &str, status: u16, body: &str) {
            self.replies.insert(
                path.into(),
                Reply {
                    status,
                    body: body.into(),
                    rate_limited: false,
                },
            );
        }

        /// `base...latest` (and the reverse, which a `behind` or
        /// `diverged` verdict also asks for), both changing the keymap.
        fn compare(&mut self, base: &str, status: &str, ahead_by: u32, behind_by: u32) {
            self.compare_files(
                base,
                status,
                ahead_by,
                behind_by,
                &["config/eyelash_sofle.keymap"],
            );
        }

        /// Like [`compare`](Self::compare), with these changed files on
        /// both sides.
        fn compare_files(
            &mut self,
            base: &str,
            status: &str,
            ahead_by: u32,
            behind_by: u32,
            files: &[&str],
        ) {
            let files = files_json(files);
            self.reply(
                &format!("/repos/rdlu/sofleii/compare/{base}...{RELEASE_SHA}?per_page=1"),
                200,
                &format!(r#"{{"status":"{status}","ahead_by":{ahead_by},"behind_by":{behind_by},"total_commits":{ahead_by},"commits":[],"files":{files}}}"#),
            );
            let rev = match status {
                "ahead" => "behind",
                "behind" => "ahead",
                s => s,
            };
            self.reply(
                &format!("/repos/rdlu/sofleii/compare/{RELEASE_SHA}...{base}?per_page=1"),
                200,
                &format!(r#"{{"status":"{rev}","ahead_by":{behind_by},"behind_by":{ahead_by},"commits":[],"files":{files}}}"#),
            );
        }
    }

    /// A compare's `files` array with these filenames.
    fn files_json(names: &[&str]) -> String {
        let items: Vec<String> = names
            .iter()
            .map(|n| {
                format!(r#"{{"filename":"{n}","status":"modified","additions":1,"deletions":0}}"#)
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    impl Http for FakeHttp {
        fn get(&mut self, path: &str, _accept: &str) -> Result<Reply> {
            self.requested.push(path.into());
            Ok(self.replies.get(path).cloned().unwrap_or(Reply {
                status: 404,
                body: r#"{"message":"Not Found"}"#.into(),
                rate_limited: false,
            }))
        }
    }

    fn running(version: &str) -> Running {
        running_from(&Build {
            available: true,
            zmk_config_version: (!version.is_empty()).then(|| version.into()),
            zmk_config_dirty: (!version.is_empty()).then(|| version.ends_with("-dirty")),
            ..Default::default()
        })
    }

    #[test]
    fn describe_forms() {
        let c = |s: &str| Describe::Commit(s.into());
        for (raw, want, dirty) in [
            ("5c53b30", c("5c53b30"), false),
            ("v3.0.0-68-gf9a665f", c("f9a665f"), false),
            ("v3.0.0-68-gf9a665f-dirty", c("f9a665f"), true),
            ("v3.1.0", Describe::Tag("v3.1.0".into()), false),
            ("v3.1.0-dirty", Describe::Tag("v3.1.0".into()), true),
            ("f9a665f-dirty", c("f9a665f"), true),
            ("v2.0.0-rc1-5-gabc1234", c("abc1234"), false),
            ("v2.0.0-rc1", Describe::Tag("v2.0.0-rc1".into()), false),
            ("", Describe::Unknown, false),
        ] {
            assert_eq!(parse_describe(raw), (want, dirty), "{raw:?}");
        }
    }

    #[test]
    fn running_keeps_the_raw_version() {
        assert_eq!(
            running("v3.0.0-68-gf9a665f-dirty"),
            Running {
                version: Some("v3.0.0-68-gf9a665f-dirty".into()),
                commit: Some("f9a665f".into()),
                dirty: true,
                release: None,
            }
        );
        assert_eq!(
            running(""),
            Running {
                version: None,
                commit: None,
                dirty: false,
                release: None,
            }
        );
    }

    #[test]
    fn compare_direction_is_seen_from_the_keyboard() {
        // base = running, head = latest: GitHub "ahead" = the release is
        // ahead, so the keyboard is behind by ahead_by.
        assert_eq!(from_compare("ahead", 3, 0).unwrap(), (Status::Behind, 3, 0));
        assert_eq!(from_compare("behind", 0, 2).unwrap(), (Status::Ahead, 0, 2));
        assert_eq!(
            from_compare("diverged", 4, 1).unwrap(),
            (Status::Behind, 4, 1)
        );
        assert_eq!(
            from_compare("identical", 0, 0).unwrap(),
            (Status::UpToDate, 0, 0)
        );
        assert!(from_compare("sideways", 0, 0).is_err());
    }

    #[test]
    fn exit_codes() {
        assert_eq!(Status::UpToDate.exit_code(), 0);
        assert_eq!(Status::Ahead.exit_code(), 0);
        assert_eq!(Status::Behind.exit_code(), 1);
        assert_eq!(Status::Unknown.exit_code(), 1);
        let e = Error::Unsupported(NO_DEVICE_INFO.into());
        assert_eq!(e.exit_code(), exit::TRANSPORT);
        assert_eq!(Error::GitHub("x".into()).exit_code(), exit::TRANSPORT);
    }

    #[test]
    fn combined_exit_code_is_the_worse_checked_part() {
        let mut http = FakeHttp::released();
        let fw = |v: &str, http: &mut FakeHttp| check(http, DEFAULT_REPO, running(v)).unwrap();
        let up = fw("5c53b30", &mut http);
        let unknown = fw("", &mut http);
        let cli = |mine: &str, latest: Option<&str>| {
            let l = latest.map(|v| Listed {
                tag: format!("sofleii-v{v}"),
                published_at: None,
            });
            cli_check(mine, l.as_ref())
        };
        let both = |f: Option<&Check>, c: Option<CliCheck>| {
            Checked {
                firmware: f.cloned(),
                cli: c,
                checked_at: std::time::SystemTime::UNIX_EPOCH,
                from_cache: false,
            }
            .exit_code()
        };
        // Both fine, or no sofleii release yet: 0.
        assert_eq!(both(Some(&up), Some(cli("0.1.0", Some("0.1.0")))), 0);
        assert_eq!(both(Some(&up), Some(cli("0.1.0", None))), 0);
        // Either behind: 1.
        assert_eq!(both(Some(&up), Some(cli("0.1.0", Some("0.2.0")))), 1);
        assert_eq!(both(Some(&unknown), Some(cli("0.1.0", None))), 1);
        // --cli / --firmware: only that part counts.
        assert_eq!(both(None, Some(cli("0.1.0", None))), 0);
        assert_eq!(both(None, Some(cli("0.1.0", Some("0.2.0")))), 1);
        assert_eq!(both(Some(&up), None), 0);
        assert_eq!(both(Some(&unknown), None), 1);
    }

    #[test]
    fn ci_build_at_the_release_is_up_to_date_without_compare() {
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap();
        assert_eq!(c.status, Status::UpToDate);
        assert_eq!((c.commits_behind, c.commits_ahead), (Some(0), Some(0)));
        assert_eq!(c.latest.tag, "v3.1.0");
        assert_eq!(c.latest.commit, RELEASE_SHA);
        assert!(!http.requested.iter().any(|p| p.contains("/compare/")));
    }

    #[test]
    fn behind_and_ahead_via_compare() {
        for (gh, ahead_by, behind_by, want) in [
            ("ahead", 3, 0, Status::Behind),
            ("behind", 0, 2, Status::Ahead),
            ("diverged", 4, 1, Status::Behind),
        ] {
            let mut http = FakeHttp::released();
            http.compare("f9a665f", gh, ahead_by, behind_by);
            let c = check(&mut http, DEFAULT_REPO, running("v3.0.0-68-gf9a665f-dirty")).unwrap();
            assert_eq!(c.status, want);
            assert_eq!(c.commits_behind, Some(ahead_by));
            assert_eq!(c.commits_ahead, Some(behind_by));
            assert!(c.running.dirty);
            // Only a diverged build gets the unreleased-commits note.
            let want_note = (gh == "diverged")
                .then(|| "also has 1 commit not in v3.1.0 (unreleased changes)".to_string());
            assert_eq!(c.note, want_note);
        }
    }

    #[test]
    fn merged_branch_build_with_the_same_tree_is_up_to_date() {
        // Live case: the PR head f9a665f, merged as 5c53b30 with no changes.
        let mut http = FakeHttp::released();
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/f9a665f...{RELEASE_SHA}?per_page=1"),
            200,
            r#"{"status":"ahead","ahead_by":1,"behind_by":0,"commits":[],"files":[]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("f9a665f")).unwrap();
        assert_eq!(c.status, Status::UpToDate);
        assert_eq!((c.commits_behind, c.commits_ahead), (Some(1), Some(0)));
        assert!(c.note.unwrap().contains("change no files"));
    }

    #[test]
    fn exact_tag_is_resolved() {
        // The release tag itself: no extra lookup needed.
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("v3.1.0-dirty")).unwrap();
        assert_eq!(c.status, Status::UpToDate);
        assert_eq!(c.running.commit.as_deref(), Some(RELEASE_SHA));
        assert!(c.running.dirty);

        // An older tag: resolved, then compared.
        let old = "1111111111111111111111111111111111111111";
        let mut http = FakeHttp::released();
        http.reply("/repos/rdlu/sofleii/commits/v3.0.0", 200, old);
        http.compare(old, "ahead", 70, 0);
        let c = check(&mut http, DEFAULT_REPO, running("v3.0.0")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.commits_behind, Some(70));

        // A tag GitHub doesn't know.
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("v9.9.9")).unwrap();
        assert_eq!(c.status, Status::Unknown);
        assert!(c.note.unwrap().contains("v9.9.9"));
    }

    #[test]
    fn unknown_when_empty_or_not_on_github() {
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("")).unwrap();
        assert_eq!(c.status, Status::Unknown);
        assert_eq!((c.commits_behind, c.commits_ahead), (None, None));

        // Compare 404s: an unpushed local commit.
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("v3.1.0-1-gdeadbee")).unwrap();
        assert_eq!(c.status, Status::Unknown);
        assert!(c.note.unwrap().contains("deadbee"));
    }

    #[test]
    fn api_failures_are_errors() {
        // Bogus repo: the list 404s.
        let mut http = FakeHttp::released();
        let e = check(&mut http, "rdlu/nope", running("5c53b30")).unwrap_err();
        assert_eq!(e.kind(), "github");
        assert!(e.to_string().contains("no published release"), "{e}");

        // Only sofleii releases (or none): no firmware to compare with.
        let mut http = FakeHttp::with_releases(&[("sofleii-v0.1.0", false, false)]);
        let e = check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap_err();
        assert!(
            e.to_string().contains("no published firmware release"),
            "{e}"
        );

        // Rate limited.
        let mut http = FakeHttp::released();
        http.replies.insert(
            LIST.into(),
            Reply {
                status: 403,
                body: r#"{"message":"API rate limit exceeded"}"#.into(),
                rate_limited: true,
            },
        );
        let e = check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap_err();
        assert!(e.to_string().contains("GITHUB_TOKEN"), "{e}");
        assert_eq!(e.exit_code(), exit::TRANSPORT);

        // Other statuses carry GitHub's message.
        let mut http = FakeHttp::released();
        http.reply(LIST, 500, r#"{"message":"Server Error"}"#);
        let e = check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap_err();
        assert!(e.to_string().contains("HTTP 500: Server Error"), "{e}");
    }

    #[test]
    fn json_has_explicit_nulls() {
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("")).unwrap();
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "running": {"version": null, "commit": null, "dirty": false, "release": null},
                "latest": {"tag": "v3.1.0", "commit": RELEASE_SHA,
                           "published_at": "2026-10-19T19:20:31Z"},
                "status": "unknown",
                "commits_behind": null,
                "commits_ahead": null,
                "files_changed": null,
                "firmware_files_changed": null,
                "note": "the firmware reports no zmk_config_version"
            })
        );
    }

    fn keyboard(replies: Vec<ResponseSubsystem>) -> Client<Mock> {
        Client::with_first_id(Mock::replying(replies), Duration::from_secs(1), 1)
    }

    fn subsystems(list: Vec<custom::CustomSubsystemInfo>) -> ResponseSubsystem {
        ResponseSubsystem::Custom(custom::Response {
            response_type: Some(custom::response::ResponseType::ListCustomSubsystems(
                custom::ListCustomSubsystemResponse { subsystems: list },
            )),
        })
    }

    #[test]
    fn run_reads_the_keyboard_then_github() {
        let mut resp = sample_response();
        resp.build.as_mut().unwrap().zmk_config_version = "5c53b30".into();
        resp.build.as_mut().unwrap().zmk_config_dirty = false;
        let mut c = keyboard(vec![
            subsystems(vec![
                subsystem(0, "cormoran_ble"),
                subsystem(3, "zmk__device_info"),
            ]),
            device_info_reply(3, di::response::ResponseType::DeviceInfo(resp)),
        ]);
        let check = run(&mut c, &mut FakeHttp::released(), DEFAULT_REPO).unwrap();
        assert_eq!(check.status, Status::UpToDate);
        assert_eq!(check.running.version.as_deref(), Some("5c53b30"));
    }

    #[test]
    fn run_without_device_info_is_unsupported_and_skips_github() {
        let mut c = keyboard(vec![subsystems(vec![subsystem(0, "cormoran_ble")])]);
        let mut http = FakeHttp::released();
        let e = run(&mut c, &mut http, DEFAULT_REPO).unwrap_err();
        assert_eq!(e.kind(), "unsupported");
        assert_eq!(e.exit_code(), exit::TRANSPORT);
        assert!(http.requested.is_empty());
    }

    #[test]
    fn refs_are_path_encoded() {
        assert_eq!(encode_ref("v3.1.0"), "v3.1.0");
        assert_eq!(encode_ref("release/v1 #2"), "release/v1%20%232");
    }

    fn rel(tag: &str, exact: bool, commits_after: u32) -> Option<Release> {
        Some(Release {
            tag: tag.into(),
            exact,
            commits_after,
            equivalent: false,
        })
    }

    /// The release by its firmware inputs (`built from …`).
    fn equiv(tag: &str) -> Option<Release> {
        Some(Release {
            tag: tag.into(),
            exact: true,
            commits_after: 0,
            equivalent: true,
        })
    }

    #[test]
    fn describe_names_the_release() {
        assert_eq!(
            describe_release("v3.0.0-68-gf9a665f"),
            rel("v3.0.0", false, 68)
        );
        assert_eq!(
            describe_release("v3.0.0-68-gf9a665f-dirty"),
            rel("v3.0.0", false, 68)
        );
        assert_eq!(
            describe_release("v2.0.0-rc1-5-gabc1234"),
            rel("v2.0.0-rc1", false, 5)
        );
        assert_eq!(describe_release("v3.1.0"), rel("v3.1.0", true, 0));
        assert_eq!(describe_release("v3.1.0-dirty"), rel("v3.1.0", true, 0));
        assert_eq!(describe_release("5c53b30"), None);
        assert_eq!(describe_release(""), None);
        assert_eq!(rel("v3.0.0", false, 68).unwrap().label(), "v3.0.0+68");
        assert_eq!(rel("v3.1.0", true, 0).unwrap().label(), "v3.1.0");
    }

    /// Requests after the release list + `commits/{tag}`, which every check makes.
    fn extra(http: &FakeHttp) -> Vec<&str> {
        http.requested[2..].iter().map(String::as_str).collect()
    }

    #[test]
    fn up_to_date_and_ahead_need_no_extra_call() {
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap();
        assert_eq!(c.running.release, rel("v3.1.0", true, 0));
        assert_eq!(http.requested.len(), 2);

        // Same tree (the live f9a665f case): the release by its inputs,
        // compare only.
        let mut http = FakeHttp::released();
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/f9a665f...{RELEASE_SHA}?per_page=1"),
            200,
            r#"{"status":"ahead","ahead_by":1,"behind_by":0,"files":[]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("f9a665f")).unwrap();
        assert_eq!(c.running.release, equiv("v3.1.0"));
        assert_eq!(http.requested.len(), 3);

        // Ahead: GitHub's list is the release's side (empty), so the
        // build's own files take the reverse compare.
        let mut http = FakeHttp::released();
        http.compare("abc1234", "behind", 0, 3);
        let c = check(&mut http, DEFAULT_REPO, running("abc1234")).unwrap();
        assert_eq!(c.status, Status::Ahead);
        assert_eq!(c.running.release, rel("v3.1.0", false, 3));
        assert_eq!(c.firmware_files_changed, Some(1));
        assert_eq!(
            extra(&http),
            vec![
                format!("/repos/rdlu/sofleii/compare/abc1234...{RELEASE_SHA}?per_page=1").as_str(),
                format!("/repos/rdlu/sofleii/compare/{RELEASE_SHA}...abc1234?per_page=1").as_str(),
            ]
        );
    }

    #[test]
    fn behind_with_a_describe_string_needs_no_scan() {
        let mut http = FakeHttp::released();
        http.compare("f9a665f", "ahead", 2, 0);
        let c = check(&mut http, DEFAULT_REPO, running("v3.0.0-68-gf9a665f-dirty")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.running.release, rel("v3.0.0", false, 68));
        assert_eq!(extra(&http).len(), 1, "{:?}", http.requested);
    }

    /// A release list as GitHub returns it (trimmed); v3.1.0 carries the
    /// real release's `published_at`.
    pub fn release_list(tags: &[(&str, bool, bool)]) -> String {
        let items: Vec<String> = tags
            .iter()
            .map(|(t, draft, pre)| {
                let published = if *t == "v3.1.0" {
                    r#""2026-10-19T19:20:31Z""#.to_string()
                } else if *draft {
                    "null".into()
                } else {
                    r#""2026-10-01T00:00:00Z""#.into()
                };
                format!(
                    r#"{{"tag_name":"{t}","draft":{draft},"prerelease":{pre},"published_at":{published},"assets":[]}}"#
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    pub const LIST: &str = "/repos/rdlu/sofleii/releases?per_page=50";

    #[test]
    fn behind_bare_sha_scans_older_releases_skipping_drafts_and_prereleases() {
        let mut http = FakeHttp::with_releases(&[
            ("v3.2.0", true, false),
            ("sofleii-v0.1.0", false, false),
            ("v3.1.0", false, false),
            ("v3.1.0-rc1", false, true),
            ("v3.0.0", false, false),
            ("v2.8.0", false, false),
        ]);
        http.compare("1a2b3c4", "ahead", 30, 0);
        http.reply(
            "/repos/rdlu/sofleii/compare/v3.0.0...1a2b3c4?per_page=1",
            200,
            r#"{"status":"ahead","ahead_by":12,"behind_by":0,"files":[{"filename":"config/eyelash_sofle.keymap"}]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.running.release, rel("v3.0.0", false, 12));
        // compare with latest, one compare (v3.0.0) from the same list; stops there.
        assert_eq!(
            extra(&http),
            vec![
                format!("/repos/rdlu/sofleii/compare/1a2b3c4...{RELEASE_SHA}?per_page=1").as_str(),
                "/repos/rdlu/sofleii/compare/v3.0.0...1a2b3c4?per_page=1",
            ]
        );
        assert_eq!(http.requested.iter().filter(|p| *p == LIST).count(), 1);
    }

    #[test]
    fn scan_finds_an_exact_older_release_by_tree() {
        let mut http =
            FakeHttp::with_releases(&[("v3.1.0", false, false), ("v3.0.0", false, false)]);
        http.compare("1a2b3c4", "diverged", 5, 1);
        http.reply(
            "/repos/rdlu/sofleii/compare/v3.0.0...1a2b3c4?per_page=1",
            200,
            r#"{"status":"ahead","ahead_by":1,"behind_by":0,"files":[]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(
            c.note.as_deref(),
            Some("also has 1 commit not in v3.1.0 (unreleased changes)")
        );
        assert_eq!(c.running.release, equiv("v3.0.0"));
    }

    #[test]
    fn scan_is_capped_and_can_find_nothing() {
        // 12 older ones listed, none fit: every compare is "diverged" or a 404.
        let tags: Vec<String> = (0..12).map(|i| format!("v2.{i}.0")).collect();
        let mut list: Vec<(&str, bool, bool)> = vec![("v3.1.0", false, false)];
        list.extend(tags.iter().map(|t| (t.as_str(), false, false)));
        let mut http = FakeHttp::with_releases(&list);
        http.compare("1a2b3c4", "diverged", 5, 1);
        for t in &tags[..3] {
            http.reply(
                &format!("/repos/rdlu/sofleii/compare/{t}...1a2b3c4?per_page=1"),
                200,
                r#"{"status":"diverged","ahead_by":2,"behind_by":2,"files":[{"filename":"x"}]}"#,
            );
        }
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.running.release, None);
        // compare with latest both ways + 10 compares, not 12.
        assert_eq!(extra(&http).len(), 12, "{:?}", http.requested);
        // Newest first by semver: v2.11.0 before v2.9.0.
        assert!(
            extra(&http)[2].contains("/v2.11.0..."),
            "{:?}",
            http.requested
        );
    }

    #[test]
    fn scan_failure_keeps_the_verdict_with_a_note() {
        let mut http =
            FakeHttp::with_releases(&[("v3.1.0", false, false), ("v3.0.0", false, false)]);
        http.compare("1a2b3c4", "ahead", 3, 0);
        http.replies.insert(
            "/repos/rdlu/sofleii/compare/v3.0.0...1a2b3c4?per_page=1".into(),
            Reply {
                status: 403,
                body: "{}".into(),
                rate_limited: true,
            },
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.running.release, None);
        assert!(c.note.unwrap().contains("rate limited"));
    }

    #[test]
    fn unknown_uses_the_describe_string_only() {
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("v3.1.0-1-gdeadbee")).unwrap();
        assert_eq!(c.status, Status::Unknown);
        assert_eq!(c.running.release, rel("v3.1.0", false, 1));
        let mut http = FakeHttp::released();
        let c = check(&mut http, DEFAULT_REPO, running("deadbee")).unwrap();
        assert_eq!(c.running.release, None);
        // The list once, the tag, the failed compare: no scan.
        assert_eq!(http.requested.len(), 3, "{:?}", http.requested);
    }

    #[test]
    fn never_asks_for_releases_latest() {
        let mut http = FakeHttp::released();
        check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap();
        assert!(!http.requested.iter().any(|p| p.contains("releases/latest")));
    }

    #[test]
    fn firmware_ignores_sofleii_tags_drafts_and_prereleases() {
        // A sofleii release newer than everything (GitHub would call it
        // "latest" without --latest=false) and a newer draft/prerelease.
        let mut http = FakeHttp::with_releases(&[
            ("sofleii-v0.1.0", false, false),
            ("v3.3.0", true, false),
            ("v3.2.0-rc1", false, true),
            ("canary-zephyr-4.1", false, true),
            ("nightly", false, false),
            ("v3.1.0", false, false),
            ("v3.0.0", false, false),
        ]);
        let c = check(&mut http, DEFAULT_REPO, running("5c53b30")).unwrap();
        assert_eq!(c.latest.tag, "v3.1.0");
        assert_eq!(c.status, Status::UpToDate);
    }

    #[test]
    fn releases_split_by_family_newest_first_by_semver() {
        let mut http = FakeHttp::with_releases(&[
            // GitHub's created_at order: a backported v2.8.1 after v3.1.0.
            ("v2.8.1", false, false),
            ("sofleii-v0.1.0", false, false),
            ("sofleii-v0.2.0", false, false),
            ("sofleii-v0.3.0", true, false),
            ("sofleii-v0.3.0-rc.1", false, true),
            ("sofleii-vNext", false, false),
            ("v3.1.0", false, false),
            ("v10.0", false, false),
        ]);
        let r = list_releases(&mut http, DEFAULT_REPO).unwrap();
        let tags = |v: &[Listed]| v.iter().map(|l| l.tag.clone()).collect::<Vec<_>>();
        assert_eq!(tags(&r.firmware), ["v3.1.0", "v2.8.1"]);
        assert_eq!(tags(&r.cli), ["sofleii-v0.2.0", "sofleii-v0.1.0"]);
    }

    #[test]
    fn semver_order() {
        let v = |s: &str| Version::parse(s).unwrap_or_else(|| panic!("{s}"));
        assert!(v("0.2.0") > v("0.1.9"));
        assert!(v("0.10.0") > v("0.9.0"));
        assert!(v("1.0.0") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.10") > v("1.0.0-rc.2"));
        assert!(v("1.0.0-beta") > v("1.0.0-2"));
        assert!(v("1.0.0-rc.1.1") > v("1.0.0-rc.1"));
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
        for bad in [
            "", "1", "1.2", "1.2.3.4", "v1.2.3", "1.x.3", "1.2.3-", "01a.2.3",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    fn listed(tag: &str) -> Listed {
        Listed {
            tag: tag.into(),
            published_at: Some("2026-10-20T12:00:00Z".into()),
        }
    }

    #[test]
    fn cli_version_comparison() {
        for (mine, latest, want, code) in [
            ("0.1.0", "sofleii-v0.1.0", Status::UpToDate, 0),
            ("0.1.0", "sofleii-v0.2.0", Status::Behind, 1),
            ("0.1.0", "sofleii-v0.1.1", Status::Behind, 1),
            ("0.2.0", "sofleii-v0.1.0", Status::Ahead, 0),
            ("0.2.0-dev", "sofleii-v0.1.0", Status::Ahead, 0),
            ("0.2.0-dev", "sofleii-v0.2.0", Status::Behind, 1),
        ] {
            let c = cli_check(mine, Some(&listed(latest)));
            assert_eq!(c.status, want, "{mine} vs {latest}");
            assert_eq!(c.exit_code(), code, "{mine} vs {latest}");
            assert_eq!(c.tag.as_deref(), Some(latest));
            assert_eq!(c.latest.as_deref(), latest.strip_prefix("sofleii-v"));
            assert_eq!(c.note, None);
        }
        let c = cli_check("garbage", Some(&listed("sofleii-v0.1.0")));
        assert_eq!(c.status, Status::Unknown);
        assert!(c.note.unwrap().contains("semver"));
    }

    #[test]
    fn no_cli_release_is_unknown_with_a_note_and_exit_0() {
        let c = cli_check(CLI_VERSION, None);
        assert_eq!(c.status, Status::Unknown);
        assert_eq!(c.exit_code(), 0);
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::json!({
                "version": CLI_VERSION, "latest": null, "tag": null,
                "published_at": null, "status": "unknown", "note": NO_CLI_RELEASE
            })
        );
    }

    #[test]
    fn firmware_inputs_match_the_build_workflow_paths() {
        for p in [
            "config/eyelash_sofle.keymap",
            "config/boards/shields/eyelash_sofle/eyelash_sofle.overlay",
            "build.yaml",
            "zmk-rgb-layer/src/rgb_layer.c",
            ".github/workflows/build.yml",
        ] {
            assert!(is_firmware_input(p), "{p}");
        }
        for p in [
            "configs/x",
            "config",
            "build.yaml.bak",
            "sub/config/x",
            "zmk-rgb-layer-old/x",
            ".github/workflows/sofleii.yml",
            "sofleii/src/commands/update.rs",
            "CLAUDE.md",
        ] {
            assert!(!is_firmware_input(p), "{p}");
        }
        // The constant mirrors the workflow's `paths:` filter exactly.
        let wf = include_str!("../../../.github/workflows/build.yml");
        let paths: Vec<&str> = wf
            .split_once("paths:")
            .unwrap()
            .1
            .lines()
            .skip(1)
            .map_while(|l| l.trim().strip_prefix("- "))
            .map(|l| l.trim_matches('"'))
            .collect();
        let ours: Vec<String> = FIRMWARE_INPUTS
            .iter()
            .map(|p| match p.strip_suffix('/') {
                Some(d) => format!("{d}/**"),
                None => p.to_string(),
            })
            .collect();
        assert_eq!(paths, ours);
    }

    /// The 35 files of `compare/0858966...96f3a2d` (2026-10-21).
    const LIVE_35: &[&str] = &[
        ".github/scripts/package-sofleii.sh",
        ".github/workflows/sofleii-release.yml",
        ".github/workflows/sofleii.yml",
        "CLAUDE.md",
        "site/content/docs/reference/troubleshooting.md",
        "site/content/docs/sofleii.md",
        "site/content/docs/studio.md",
        "sofleii/README.md",
        "sofleii/build.rs",
        "sofleii/scripts/zmk_keys_table.py",
        "sofleii/src/client/mod.rs",
        "sofleii/src/client/rpc.rs",
        "sofleii/src/commands/keymap.rs",
        "sofleii/src/commands/mod.rs",
        "sofleii/src/commands/update.rs",
        "sofleii/src/keymap/diff.rs",
        "sofleii/src/keymap/dts.rs",
        "sofleii/src/keymap/expr.rs",
        "sofleii/src/keymap/keys.rs",
        "sofleii/src/keymap/keys_table.rs",
        "sofleii/src/keymap/mod.rs",
        "sofleii/src/keymap/render.rs",
        "sofleii/src/lib.rs",
        "sofleii/src/main.rs",
        "sofleii/src/output.rs",
        "sofleii/src/session/jobs.rs",
        "sofleii/src/session/mod.rs",
        // Spelt in two parts: tests/layering.rs keeps the front end's
        // module name out of the core.
        concat!("sofleii/src/t", "ui/snapshot.rs"),
        concat!("sofleii/src/t", "ui/tests.rs"),
        concat!("sofleii/src/t", "ui/view.rs"),
        "sofleii/src/version.rs",
        "sofleii/tests/cli.rs",
        "sofleii/tests/fixtures/keymap_live.hex",
        "sofleii/tests/golden.rs",
        "sofleii/tests/keymap_capture.rs",
    ];

    #[test]
    fn live_pr_branch_build_differing_only_outside_firmware_is_up_to_date() {
        // The #90 PR head 0858966 against release v3.2.0 (its merge commit):
        // GitHub says ahead by 8 with 35 files, none a firmware input.
        assert_eq!(LIVE_35.len(), 35);
        let mut http = FakeHttp::released();
        http.compare_files("0858966", "ahead", 8, 0, LIVE_35);
        let c = check(&mut http, DEFAULT_REPO, running("0858966")).unwrap();
        assert_eq!(c.status, Status::UpToDate);
        assert_eq!(c.status.exit_code(), 0);
        assert_eq!((c.commits_behind, c.commits_ahead), (Some(8), Some(0)));
        assert_eq!(
            (c.files_changed, c.firmware_files_changed),
            (Some(35), Some(0))
        );
        assert_eq!(
            c.note.as_deref(),
            Some(
                "built from 0858966; differs from v3.1.0 only outside the firmware inputs (35 files)"
            )
        );
        assert_eq!(c.running.release, equiv("v3.1.0"));
        // Keyboard behind: the merge base is the build, one compare does it.
        assert_eq!(extra(&http).len(), 1, "{:?}", http.requested);
        let checked = Checked {
            firmware: Some(c.clone()),
            cli: None,
            checked_at: std::time::SystemTime::UNIX_EPOCH,
            from_cache: false,
        };
        assert_eq!(checked.exit_code(), 0);
        assert_eq!(
            crate::output::update_headline(&c),
            "up to date: v3.1.0 (built from 0858966)"
        );
    }

    #[test]
    fn one_firmware_input_among_the_changes_is_behind() {
        for fw in [
            "config/eyelash_sofle.keymap",
            "build.yaml",
            "zmk-rgb-layer/src/rgb_layer.c",
            ".github/workflows/build.yml",
        ] {
            let mut files = LIVE_35.to_vec();
            files.push(fw);
            let mut http = FakeHttp::released();
            http.compare_files("0858966", "ahead", 9, 0, &files);
            let c = check(&mut http, DEFAULT_REPO, running("0858966")).unwrap();
            assert_eq!(c.status, Status::Behind, "{fw}");
            assert_eq!(c.status.exit_code(), 1, "{fw}");
            assert_eq!(
                (c.files_changed, c.firmware_files_changed),
                (Some(36), Some(1)),
                "{fw}"
            );
            assert_eq!(c.note, None, "{fw}");
            assert_eq!(c.running.release, None, "{fw}");
        }
    }

    #[test]
    fn a_rename_out_of_config_is_a_firmware_change() {
        let mut http = FakeHttp::released();
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/0858966...{RELEASE_SHA}?per_page=1"),
            200,
            r#"{"status":"ahead","ahead_by":1,"behind_by":0,"files":[
                {"filename":"old/eyelash_sofle.conf","previous_filename":"config/eyelash_sofle.conf"}]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("0858966")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.firmware_files_changed, Some(1));
    }

    #[test]
    fn a_truncated_or_missing_file_list_goes_by_the_commits() {
        let many: Vec<String> = (0..COMPARE_FILE_CAP)
            .map(|i| format!("docs/{i}.md"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let mut http = FakeHttp::released();
        http.compare_files("0858966", "ahead", 8, 0, &many);
        let c = check(&mut http, DEFAULT_REPO, running("0858966")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.status.exit_code(), 1);
        assert_eq!((c.files_changed, c.firmware_files_changed), (None, None));
        let note = c.note.unwrap();
        assert!(
            note.starts_with("can't tell whether a firmware input changed"),
            "{note}"
        );
        assert!(note.contains("at most 300 files"), "{note}");

        // No `files` key at all.
        let mut http = FakeHttp::released();
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/0858966...{RELEASE_SHA}?per_page=1"),
            200,
            r#"{"status":"ahead","ahead_by":8,"behind_by":0}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("0858966")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.files_changed, None);
        assert!(c.note.unwrap().contains("listed no files"));
    }

    #[test]
    fn diverged_without_a_firmware_change_is_up_to_date() {
        let mut http = FakeHttp::released();
        http.compare_files("1a2b3c4", "diverged", 4, 2, &["README.md", "site/x.md"]);
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::UpToDate);
        assert_eq!(c.status.exit_code(), 0);
        assert_eq!((c.commits_behind, c.commits_ahead), (Some(4), Some(2)));
        // Both sides list the same two paths: counted once.
        assert_eq!(
            (c.files_changed, c.firmware_files_changed),
            (Some(2), Some(0))
        );
        assert_eq!(c.running.release, equiv("v3.1.0"));
        assert!(
            c.note
                .unwrap()
                .contains("only outside the firmware inputs (2 files)")
        );
        // Both directions asked, no scan.
        assert_eq!(extra(&http).len(), 2, "{:?}", http.requested);
    }

    #[test]
    fn diverged_with_a_firmware_change_on_the_build_side_is_behind() {
        // The release's side is docs only; the branch build changed the keymap.
        let mut http = FakeHttp::released();
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/1a2b3c4...{RELEASE_SHA}?per_page=1"),
            200,
            r#"{"status":"diverged","ahead_by":4,"behind_by":1,"files":[{"filename":"README.md"}]}"#,
        );
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/{RELEASE_SHA}...1a2b3c4?per_page=1"),
            200,
            r#"{"status":"diverged","ahead_by":1,"behind_by":4,"files":[{"filename":"config/eyelash_sofle.keymap"}]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(
            (c.files_changed, c.firmware_files_changed),
            (Some(2), Some(1))
        );
        assert_eq!(
            c.note.as_deref(),
            Some("also has 1 commit not in v3.1.0 (unreleased changes)")
        );
    }

    #[test]
    fn diverged_reverse_compare_failure_goes_by_the_commits() {
        let mut http = FakeHttp::released();
        http.reply(
            &format!("/repos/rdlu/sofleii/compare/1a2b3c4...{RELEASE_SHA}?per_page=1"),
            200,
            r#"{"status":"diverged","ahead_by":4,"behind_by":1,"files":[]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.files_changed, None);
        let note = c.note.unwrap();
        assert!(note.contains("reverse compare: HTTP 404"), "{note}");
        assert!(
            note.ends_with("also has 1 commit not in v3.1.0 (unreleased changes)"),
            "{note}"
        );
    }

    #[test]
    fn ahead_with_docs_only_commits_is_the_release() {
        let mut http = FakeHttp::released();
        http.compare_files("abc1234", "behind", 0, 3, &["sofleii/README.md"]);
        let c = check(&mut http, DEFAULT_REPO, running("abc1234")).unwrap();
        assert_eq!(c.status, Status::UpToDate);
        assert_eq!(c.running.release, equiv("v3.1.0"));
        assert_eq!((c.commits_behind, c.commits_ahead), (Some(0), Some(3)));
    }

    #[test]
    fn scan_maps_an_older_equivalent_build_and_skips_behind() {
        let mut http = FakeHttp::with_releases(&[
            ("v3.1.0", false, false),
            ("v3.0.0", false, false),
            ("v2.8.0", false, false),
        ]);
        http.compare("1a2b3c4", "ahead", 30, 0);
        // Older than v3.0.0: GitHub's list there is always empty, which
        // used to read as "same tree".
        http.reply(
            "/repos/rdlu/sofleii/compare/v3.0.0...1a2b3c4?per_page=1",
            200,
            r#"{"status":"behind","ahead_by":0,"behind_by":5,"files":[]}"#,
        );
        http.reply(
            "/repos/rdlu/sofleii/compare/v2.8.0...1a2b3c4?per_page=1",
            200,
            r#"{"status":"ahead","ahead_by":3,"behind_by":0,"files":[{"filename":"site/a.md"}]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.status, Status::Behind);
        assert_eq!(c.running.release, equiv("v2.8.0"));
        assert!(
            crate::output::update_headline(&c)
                .starts_with("v3.1.0 available, running v2.8.0 (built from 1a2b3c4)"),
        );

        // A firmware change after the tag: `tag+N`.
        let mut http =
            FakeHttp::with_releases(&[("v3.1.0", false, false), ("v3.0.0", false, false)]);
        http.compare("1a2b3c4", "ahead", 30, 0);
        http.reply(
            "/repos/rdlu/sofleii/compare/v3.0.0...1a2b3c4?per_page=1",
            200,
            r#"{"status":"ahead","ahead_by":3,"behind_by":0,"files":[{"filename":"config/x.conf"}]}"#,
        );
        let c = check(&mut http, DEFAULT_REPO, running("1a2b3c4")).unwrap();
        assert_eq!(c.running.release, rel("v3.0.0", false, 3));
    }

    #[test]
    fn json_of_an_equivalent_build() {
        let mut http = FakeHttp::released();
        http.compare_files("0858966", "ahead", 8, 0, LIVE_35);
        let c = check(&mut http, DEFAULT_REPO, running("0858966")).unwrap();
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::json!({
                "running": {"version": "0858966", "commit": "0858966", "dirty": false,
                            "release": {"tag": "v3.1.0", "exact": true, "commits_after": 0,
                                        "equivalent": true}},
                "latest": {"tag": "v3.1.0", "commit": RELEASE_SHA,
                           "published_at": "2026-10-19T19:20:31Z"},
                "status": "up_to_date",
                "commits_behind": 8,
                "commits_ahead": 0,
                "files_changed": 35,
                "firmware_files_changed": 0,
                "note": "built from 0858966; differs from v3.1.0 only outside the firmware inputs (35 files)"
            })
        );
        // A Check cached by 0.1.0 (no new fields) still reads.
        let old = r#"{"running":{"version":"x","commit":null,"dirty":false,
            "release":{"tag":"v3.1.0","exact":true,"commits_after":0}},
            "latest":{"tag":"v3.1.0","commit":"c","published_at":null},
            "status":"up_to_date","commits_behind":0,"commits_ahead":0,"note":null}"#;
        let c: Check = serde_json::from_str(old).unwrap();
        assert_eq!(c.files_changed, None);
        assert!(!c.running.release.unwrap().equivalent);
    }
}
