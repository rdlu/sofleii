//! The flash model for the UF2 bootloader drive (#98): what is on a
//! nice!nano before we overwrite it. Pure data and pure functions: finding
//! the drive and reading its files live in [`crate::commands::flash`].
//!
//! Every nice!nano mounts as the same `NICENANO` drive, both halves
//! included, and its `INFO_UF2.TXT` only describes the bootloader. What
//! tells them apart is `CURRENT.UF2`, which the Adafruit nRF52 bootloader
//! generates on the fly from the application flash:
//!
//! - [`uf2`]: UF2 blocks back into a flash image.
//! - [`identify`]: which keyboard and half that image belongs to, from the
//!   identity record our firmware carries (`zmk-sofleii-id`), the strings of
//!   older builds without it, and the remains of the firmware a
//!   `settings_reset` replaced.

pub mod identify;
pub mod uf2;

pub use identify::{Firmware, Half, Identity, identify};
pub use uf2::Image;
