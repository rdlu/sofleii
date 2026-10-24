//! Commands: reads (`keymap` included, and `flash`, which reads the UF2
//! bootloader drive instead of the keyboard), and the writes `profile` and `output`. Each returns
//! plain, serialisable data; rendering lives in [`crate::output`] and the
//! front ends (CLI, TUI), and so does confirming destructive writes.
//! Nothing here depends on UI code, prompts or prints to stdout.

pub mod battery;
pub mod flash;
pub mod handover;
pub mod info;
pub mod keymap;
pub mod output;
pub mod profile;
pub mod profiles;
pub mod split;
pub mod status;
pub mod update;
