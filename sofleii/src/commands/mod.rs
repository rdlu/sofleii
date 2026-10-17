//! Read commands. Each returns plain, serialisable data; rendering lives in
//! [`crate::output`] and front ends (CLI now, TUI later). Nothing here
//! depends on UI code or prints to stdout.

pub mod battery;
pub mod info;
pub mod profiles;
pub mod status;
