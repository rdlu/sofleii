//! sofleii core: Studio RPC over a pluggable transport, plus the commands
//! that the CLI (and later the TUI) front ends render.
//!
//! Layering (each layer only depends on the ones above it):
//! `proto` → `framing` → `transport` → `client` → `commands` → `output`.
//! `commands` returns plain data and never prints; front ends own rendering.
//! `bluez` (system D-Bus) is a side input: it implements
//! [`commands::battery::Gatt`] so `commands` stays free of D-Bus details.

pub mod bluez;
pub mod client;
pub mod commands;
pub mod error;
pub mod framing;
pub mod output;
pub mod proto;
pub mod transport;

pub use error::{Error, Result};
