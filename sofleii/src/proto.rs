//! prost-generated message types (see `build.rs` and `proto/README.md`).
#![allow(clippy::all, missing_docs)]

pub mod zmk {
    pub mod studio {
        include!(concat!(env!("OUT_DIR"), "/zmk.studio.rs"));
    }
    pub mod core {
        include!(concat!(env!("OUT_DIR"), "/zmk.core.rs"));
    }
    pub mod meta {
        include!(concat!(env!("OUT_DIR"), "/zmk.meta.rs"));
    }
    pub mod custom {
        include!(concat!(env!("OUT_DIR"), "/zmk.custom.rs"));
    }
    pub mod keymap {
        include!(concat!(env!("OUT_DIR"), "/zmk.keymap.rs"));
    }
    pub mod behaviors {
        include!(concat!(env!("OUT_DIR"), "/zmk.behaviors.rs"));
    }
    pub mod ble_management {
        include!(concat!(env!("OUT_DIR"), "/zmk.ble_management.rs"));
    }
    pub mod device_info {
        include!(concat!(env!("OUT_DIR"), "/zmk.device_info.rs"));
    }
}

pub use zmk::*;
