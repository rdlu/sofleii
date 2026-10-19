//! Commands: reads, and the writes `profile` and `output`. Each returns
//! plain, serialisable data; rendering lives in [`crate::output`] and front
//! ends (CLI now, TUI later), and so does confirming destructive writes.
//! Nothing here depends on UI code, prompts or prints to stdout.

pub mod battery;
pub mod handover;
pub mod info;
pub mod output;
pub mod profile;
pub mod profiles;
pub mod status;
pub mod update;
