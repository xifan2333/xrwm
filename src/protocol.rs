// SPDX-License-Identifier: GPL-3.0-only
// Official River 0.4 protocol client bindings generated via wayland-scanner macros

pub extern crate wayland_client;
pub use wayland_client::protocol::*;

pub mod interfaces {
    pub mod rwm {
        pub use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/river-window-management-v1.xml");
    }

    pub mod rxkb {
        use super::rwm::*;
        wayland_scanner::generate_interfaces!("./protocols/river-xkb-bindings-v1.xml");
    }

    pub mod rlayer {
        use super::rwm::*;
        wayland_scanner::generate_interfaces!("./protocols/river-layer-shell-v1.xml");
    }
}

use self::interfaces::rlayer::*;
use self::interfaces::rwm::*;
use self::interfaces::rxkb::*;

wayland_scanner::generate_client_code!("./protocols/river-window-management-v1.xml");
wayland_scanner::generate_client_code!("./protocols/river-xkb-bindings-v1.xml");
wayland_scanner::generate_client_code!("./protocols/river-layer-shell-v1.xml");
