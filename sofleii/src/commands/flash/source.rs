//! Fetching the firmware for a flash (#100): the latest or a tagged
//! release (no token), a CI build (token: GitHub requires auth for artifact
//! downloads) or local files. The JSON shapes and the classification live
//! in [`crate::flash::source`]; [`Fetch`] keeps the network out of tests.

use std::fs;
use std::path::PathBuf;

use crate::commands::update::{ACCEPT_JSON, Http, list_releases};
use crate::error::{Error, Result};
use crate::flash::source::{self as src, Bundle, Origin};

/// [`Http`] plus binary downloads.
pub trait Fetch: Http {
    /// GETs an absolute URL (a release asset, an artifact archive) and
    /// returns the body. Sends the token only to api.github.com.
    fn download(&mut self, url: &str) -> Result<Vec<u8>>;
    /// Whether a token is configured (`GH_TOKEN`/`GITHUB_TOKEN`).
    fn has_token(&self) -> bool;
}

/// Where to take the firmware from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The newest `sofleii-fw-v*` release (the default).
    LatestRelease,
    /// A release by tag.
    Release(String),
    /// The newest successful build on a branch.
    Ci { branch: String },
    /// One build run.
    Run(u64),
    /// Local `.uf2` files.
    Files(Vec<PathBuf>),
}

/// The firmware `source` names, classified into a [`Bundle`].
pub fn fetch<F: Fetch + ?Sized>(source: &Source, repo: &str, net: &mut F) -> Result<Bundle> {
    match source {
        Source::Files(paths) => {
            let mut files = Vec::new();
            for p in paths {
                let bytes =
                    fs::read(p).map_err(|e| Error::io(format!("reading {}", p.display()), e))?;
                let name = p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                files.push((name, bytes));
            }
            bundle(Origin::Files, files)
        }
        Source::LatestRelease => {
            let releases = list_releases(net, repo)?;
            let Some(newest) = releases.firmware.first() else {
                return Err(Error::GitHub(format!(
                    "{repo} has no published sofleii-fw-v* release"
                )));
            };
            release(net, repo, &newest.tag.clone())
        }
        Source::Release(tag) => release(net, repo, tag),
        Source::Ci { branch } => {
            need_token(net)?;
            let body = get_json(net, &src::runs_path(repo, branch), || {
                format!("no build workflow in {repo}")
            })?;
            let Some(run) = src::newest_run(&body).map_err(Error::GitHub)? else {
                return Err(Error::GitHub(format!(
                    "no successful firmware build on branch {branch}: the build runs on \
                     pushes to main, and on other branches only when started by hand \
                     (gh workflow run build.yml --ref {branch})"
                )));
            };
            ci(net, repo, run)
        }
        Source::Run(id) => {
            need_token(net)?;
            let body = get_json(net, &src::run_path(repo, *id), || {
                format!("no run {id} in {repo}")
            })?;
            let run = src::run(&body).map_err(Error::GitHub)?;
            if run.conclusion.as_deref() != Some("success") {
                return Err(Error::GitHub(format!(
                    "run {id} did not succeed ({})",
                    run.conclusion.as_deref().unwrap_or("still running")
                )));
            }
            ci(net, repo, run)
        }
    }
}

fn bundle(origin: Origin, files: Vec<(String, Vec<u8>)>) -> Result<Bundle> {
    Bundle::from_files(origin, files).map_err(Error::Usage)
}

fn need_token<F: Fetch + ?Sized>(net: &F) -> Result<()> {
    if net.has_token() {
        Ok(())
    } else {
        Err(Error::Usage(
            "CI builds need a GitHub token (GitHub requires one to download artifacts): \
             set GH_TOKEN or GITHUB_TOKEN, e.g. GH_TOKEN=$(gh auth token)"
                .into(),
        ))
    }
}

/// GETs an API path; 200 is the body, 404 is `missing()`, anything else a
/// GitHub error (a rate limit says so).
fn get_json<F: Fetch + ?Sized>(
    net: &mut F,
    path: &str,
    missing: impl FnOnce() -> String,
) -> Result<String> {
    let r = net.get(path, ACCEPT_JSON)?;
    match r.status {
        200 => Ok(r.body),
        _ if r.rate_limited => Err(Error::GitHub(
            "rate limited; set GH_TOKEN or GITHUB_TOKEN to lift the limit".into(),
        )),
        404 => Err(Error::GitHub(missing())),
        401 | 403 => Err(Error::GitHub(format!(
            "HTTP {} for {path}: the token can't read this (it needs the repo's actions:read)",
            r.status
        ))),
        s => Err(Error::GitHub(format!("HTTP {s} for {path}"))),
    }
}

fn release<F: Fetch + ?Sized>(net: &mut F, repo: &str, tag: &str) -> Result<Bundle> {
    let body = get_json(net, &src::release_path(repo, tag), || {
        format!("no release tagged {tag} in {repo}")
    })?;
    let assets = src::release_assets(&body).map_err(Error::GitHub)?;
    if assets.is_empty() {
        return Err(Error::GitHub(format!("release {tag} has no .uf2 files")));
    }
    let mut files = Vec::new();
    for a in assets {
        let bytes = net.download(&a.browser_download_url)?;
        files.push((a.name, bytes));
    }
    bundle(Origin::Release { tag: tag.into() }, files)
}

fn ci<F: Fetch + ?Sized>(net: &mut F, repo: &str, run: src::Run) -> Result<Bundle> {
    let body = get_json(net, &src::artifacts_path(repo, run.id), || {
        format!("no artifacts for run {}", run.id)
    })?;
    let artifact =
        src::firmware_artifact(&body).map_err(|e| Error::GitHub(format!("run {}: {e}", run.id)))?;
    let zip = net.download(&artifact.archive_download_url)?;
    let files = src::unzip(&zip).map_err(|e| Error::GitHub(format!("run {}: {e}", run.id)))?;
    bundle(
        Origin::Ci {
            run_id: run.id,
            branch: run.head_branch,
            commit: run.head_sha,
            created_at: run.created_at,
        },
        files,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::io::{Cursor, Write};

    use super::*;
    use crate::commands::update::Reply;
    use crate::flash::source::tests::release_files;
    use crate::flash::{Firmware, Role};

    /// Canned API replies by path, downloads by URL.
    #[derive(Default)]
    pub struct Fake {
        pub api: HashMap<String, (u16, String)>,
        pub files: HashMap<String, Vec<u8>>,
        pub token: bool,
        pub downloaded: Vec<String>,
    }

    impl Http for Fake {
        fn get(&mut self, path: &str, _: &str) -> Result<Reply> {
            let (status, body) = self.api.get(path).cloned().unwrap_or((404, "{}".into()));
            Ok(Reply {
                status,
                body,
                rate_limited: false,
            })
        }
    }

    impl Fetch for Fake {
        fn download(&mut self, url: &str) -> Result<Vec<u8>> {
            self.downloaded.push(url.into());
            self.files
                .get(url)
                .cloned()
                .ok_or_else(|| Error::GitHub(format!("no {url}")))
        }
        fn has_token(&self) -> bool {
            self.token
        }
    }

    const REPO: &str = "rdlu/sofleii";

    fn with_release(f: &mut Fake, tag: &str, record: bool) {
        let mut assets = Vec::new();
        for (name, bytes) in release_files(record) {
            let url = format!("https://github.com/{REPO}/releases/download/{tag}/{name}");
            assets.push(format!(
                r#"{{"name":"{name}","browser_download_url":"{url}"}}"#
            ));
            f.files.insert(url, bytes);
        }
        f.api.insert(
            src::release_path(REPO, tag),
            (
                200,
                format!(r#"{{"tag_name":"{tag}","assets":[{}]}}"#, assets.join(",")),
            ),
        );
    }

    #[test]
    fn latest_and_tagged_releases() {
        let mut f = Fake::default();
        f.api.insert(
            format!("/repos/{REPO}/releases?per_page=50"),
            (
                200,
                r#"[{"tag_name":"sofleii-tui-v0.2.0","draft":false,"prerelease":false,"published_at":null},
                    {"tag_name":"sofleii-fw-v3.3.0","draft":false,"prerelease":false,"published_at":null},
                    {"tag_name":"sofleii-fw-v3.2.0","draft":false,"prerelease":false,"published_at":null}]"#
                    .into(),
            ),
        );
        with_release(&mut f, "sofleii-fw-v3.3.0", true);
        with_release(&mut f, "sofleii-fw-v3.2.0", false);
        let b = fetch(&Source::LatestRelease, REPO, &mut f).unwrap();
        assert_eq!(
            b.origin,
            Origin::Release {
                tag: "sofleii-fw-v3.3.0".into()
            }
        );
        assert_eq!(
            b.get(Role::Left).unwrap().identity.firmware,
            Firmware::Sofleii
        );
        assert!(b.right.is_some() && b.settings_reset.is_some());
        // The PDF asset is never downloaded.
        assert_eq!(f.downloaded.len(), 3, "{:?}", f.downloaded);
        let b = fetch(&Source::Release("sofleii-fw-v3.2.0".into()), REPO, &mut f).unwrap();
        assert_eq!(
            b.get(Role::Right).unwrap().identity.firmware,
            Firmware::Legacy
        );
        let e = fetch(&Source::Release("sofleii-fw-v9.9.9".into()), REPO, &mut f).unwrap_err();
        assert!(
            e.to_string()
                .contains("no release tagged sofleii-fw-v9.9.9"),
            "{e}"
        );
    }

    fn artifact_zip() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, bytes) in release_files(true).into_iter().take(3) {
            z.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(&bytes).unwrap();
        }
        z.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn ci_builds_need_a_token_and_a_green_run() {
        let mut f = Fake::default();
        let e = fetch(
            &Source::Ci {
                branch: "main".into(),
            },
            REPO,
            &mut f,
        )
        .unwrap_err();
        assert_eq!(e.exit_code(), crate::error::exit::USAGE);
        assert!(e.to_string().contains("GH_TOKEN"), "{e}");
        f.token = true;
        f.api.insert(
            src::runs_path(REPO, "feat/x"),
            (200, r#"{"total_count":0,"workflow_runs":[]}"#.into()),
        );
        let e = fetch(
            &Source::Ci {
                branch: "feat/x".into(),
            },
            REPO,
            &mut f,
        )
        .unwrap_err();
        assert!(
            e.to_string()
                .contains("gh workflow run build.yml --ref feat/x"),
            "{e}"
        );
        f.api.insert(
            src::runs_path(REPO, "main"),
            (
                200,
                r#"{"workflow_runs":[{"id":7,"head_sha":"97be6e0","head_branch":"main","conclusion":"success","created_at":"2026-10-24T22:30:00Z"}]}"#.into(),
            ),
        );
        f.api.insert(
            src::artifacts_path(REPO, 7),
            (
                200,
                r#"{"artifacts":[{"name":"firmware","archive_download_url":"https://api.github.com/a/7/zip","expired":false}]}"#.into(),
            ),
        );
        f.files
            .insert("https://api.github.com/a/7/zip".into(), artifact_zip());
        let b = fetch(
            &Source::Ci {
                branch: "main".into(),
            },
            REPO,
            &mut f,
        )
        .unwrap();
        assert_eq!(
            b.origin,
            Origin::Ci {
                run_id: 7,
                branch: Some("main".into()),
                commit: "97be6e0".into(),
                created_at: Some("2026-10-24T22:30:00Z".into())
            }
        );
        assert!(b.left.is_some() && b.right.is_some() && b.settings_reset.is_some());
        f.api.insert(
            src::run_path(REPO, 8),
            (
                200,
                r#"{"id":8,"head_sha":"x","head_branch":"main","conclusion":"failure"}"#.into(),
            ),
        );
        let e = fetch(&Source::Run(8), REPO, &mut f).unwrap_err();
        assert!(e.to_string().contains("did not succeed (failure)"), "{e}");
    }

    #[test]
    fn local_files() {
        let d = crate::commands::flash::drive::tests::tmp("files");
        let mut paths = Vec::new();
        for (name, bytes) in release_files(true).into_iter().take(2) {
            let p = d.join(name);
            fs::write(&p, bytes).unwrap();
            paths.push(p);
        }
        let b = fetch(&Source::Files(paths), REPO, &mut Fake::default()).unwrap();
        assert_eq!(b.origin, Origin::Files);
        assert!(b.left.is_some() && b.right.is_some() && b.settings_reset.is_none());
        let e = fetch(
            &Source::Files(vec![d.join("missing.uf2")]),
            REPO,
            &mut Fake::default(),
        )
        .unwrap_err();
        assert_eq!(e.kind(), "io");
        let _ = fs::remove_dir_all(&d);
    }
}
