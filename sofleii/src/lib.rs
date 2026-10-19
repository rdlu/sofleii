//! sofleii core: Studio RPC over a pluggable transport, plus the commands
//! that the CLI (and later the TUI) front ends render.
//!
//! Layering (each layer only depends on the ones above it):
//! `proto` → `framing` → `transport` → `client` → `commands` → `output`.
//! `commands` returns plain data and never prints; front ends own rendering.
//! `gatt` is BlueZ's object tree as plain data (plus the keyboard lookup);
//! `bluez` (system D-Bus) fills it, implements [`commands::battery::Gatt`]
//! so `commands` stays free of D-Bus details, and carries the BLE transport
//! (`transport::ble`). `github` (blocking HTTPS via `ureq`) implements
//! [`commands::update::Http`] the same way for `update check`.

pub mod bluez;
pub mod client;
pub mod commands;
pub mod error;
pub mod framing;
pub mod gatt;
pub mod github;
pub mod output;
pub mod proto;
pub mod transport;

pub use error::{Error, Result};
