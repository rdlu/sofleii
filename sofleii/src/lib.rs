//! sofleii core: Studio RPC over a pluggable transport, plus the commands
//! that the CLI and TUI front ends render.
//!
//! Layering (each layer only depends on the ones above it):
//! `proto` → `framing` → `transport` → `client` → `commands` → `connect`
//! → `output` → `tui`. `commands` returns plain data and never prints;
//! front ends own rendering. `connect` opens the link the flags ask for
//! (the `auto` plan, USB probe and BLE fallback), with no clap types, so
//! the CLI and a TUI session thread share it. Core diagnostics go through the `log` facade, and each front
//! end installs its logger (`logger` for the CLI's stderr, the TUI's log
//! pane in `tui::log_pane`).
//! `gatt` is BlueZ's object tree as plain data (plus the keyboard lookup);
//! `bluez` (system D-Bus) fills it, implements [`commands::battery::Gatt`]
//! so `commands` stays free of D-Bus details, and carries the BLE transport
//! (`transport::ble`). `github` (blocking HTTPS via `ureq`) implements
//! [`commands::update::Http`] the same way for `update check`.

pub mod bluez;
pub mod client;
pub mod commands;
pub mod connect;
pub mod error;
pub mod framing;
pub mod gatt;
pub mod github;
pub mod logger;
pub mod output;
pub mod proto;
pub mod transport;
pub mod tui;

pub use error::{Error, Result};
