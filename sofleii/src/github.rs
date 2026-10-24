//! GitHub REST API client for `update check` and the flash sources: a
//! blocking `ureq` agent (rustls with the bundled webpki roots; no tokio,
//! no `gh`, no system OpenSSL), implementing
//! [`commands::update::Http`](crate::commands::update::Http) and
//! [`commands::flash::source::Fetch`](crate::commands::flash::source::Fetch).

use std::time::Duration;

use crate::commands::flash::source::Fetch;
use crate::commands::update::{Http, Reply};
use crate::{Error, Result};

pub const API: &str = "https://api.github.com";

/// A firmware download (a release's `.uf2`, a CI artifact zip of three) is
/// a few MB; anything past this is not firmware.
const DOWNLOAD_MAX: u64 = 32 << 20;

pub struct GitHub {
    agent: ureq::Agent,
    /// From `GH_TOKEN`, else `GITHUB_TOKEN` (gh's order); never required,
    /// it only lifts the 60 requests/hour anonymous rate limit.
    token: Option<String>,
}

impl GitHub {
    /// `timeout` bounds each whole request (connect, TLS, body).
    pub fn new(timeout: Duration) -> Self {
        let token = ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()));
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // A 404 from compare is an answer (commit not on GitHub), not a failure.
            .http_status_as_error(false)
            .user_agent(concat!("sofleii/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
            token,
        }
    }
}

impl Http for GitHub {
    fn get(&mut self, path: &str, accept: &str) -> Result<Reply> {
        let mut req = self
            .agent
            .get(format!("{API}{path}"))
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", t.trim()));
        }
        let mut resp = req
            .call()
            .map_err(|e| Error::GitHub(format!("GET {API}{path}: {e}")))?;
        let status = resp.status().as_u16();
        let remaining = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::GitHub(format!("GET {API}{path}: reading the body: {e}")))?;
        Ok(Reply {
            status,
            body,
            rate_limited: status == 429 || (status == 403 && remaining == Some(0)),
        })
    }
}

impl Fetch for GitHub {
    fn download(&mut self, url: &str) -> Result<Vec<u8>> {
        let mut req = self.agent.get(url);
        // Artifact archives need the token; release assets don't, and the
        // token never leaves api.github.com (ureq drops it on the redirect
        // to the storage host).
        if url.starts_with(&format!("{API}/"))
            && let Some(t) = &self.token
        {
            req = req.header("Authorization", format!("Bearer {}", t.trim()));
        }
        let mut resp = req
            .call()
            .map_err(|e| Error::GitHub(format!("GET {url}: {e}")))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(Error::GitHub(format!("GET {url}: HTTP {status}")));
        }
        resp.body_mut()
            .with_config()
            .limit(DOWNLOAD_MAX)
            .read_to_vec()
            .map_err(|e| Error::GitHub(format!("GET {url}: reading the body: {e}")))
    }

    fn has_token(&self) -> bool {
        self.token.is_some()
    }
}
