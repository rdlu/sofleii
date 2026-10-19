//! A session against the real keyboard, printing its events for 10 s.
//! Read-only: it only polls (status, info, batteries, the update check into
//! a throwaway cache) and sends no write.
//!
//! ```bash
//! cargo test --test session_live -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use sofleii::connect;
use sofleii::session::{Config, Session};

#[test]
#[ignore = "needs the keyboard"]
fn live_events_for_ten_seconds() {
    let cache = std::env::temp_dir().join(format!("sofleii-live-{}", std::process::id()));
    let cfg = Config {
        cache_dir: Some(cache.clone()),
        ..Config::default()
    };
    let (_handle, rx) = Session::spawn(cfg, connect::Config::default());
    let end = Instant::now() + Duration::from_secs(10);
    while let Ok(e) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
        let line = serde_json::to_string(&e).unwrap();
        let short: String = line.chars().take(300).collect();
        let more = if short.len() < line.len() { "…" } else { "" };
        println!("{short}{more}");
    }
    let _ = std::fs::remove_dir_all(cache);
}
