//! GitHub REST API client for `update check`: a blocking `ureq` agent
//! (rustls with the bundled webpki roots; no tokio, no `gh`, no system
//! OpenSSL), implementing [`commands::update::Http`](crate::commands::update::Http).

use std::time::Duration;

use crate::commands::update::{Http, Reply};
use crate::{Error, Result};

pub const API: &str = "https://api.github.com";

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
