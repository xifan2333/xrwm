// Protocol bindings generated via wayland-scanner macros

// 1. River Window Management Protocol (river-window-management-v1)
pub mod river_wm {
    pub extern crate bitflags;
    pub extern crate wayland_backend;
    pub extern crate wayland_client;
    pub use wayland_client::protocol::{wl_output, wl_seat, wl_surface};

    pub mod __interfaces {
        pub use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/river-window-management-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("./protocols/river-window-management-v1.xml");
}

// 2. River Layer Shell Protocol (river-layer-shell-v1)
pub mod river_layer_shell {
    pub extern crate wayland_backend;
    pub extern crate wayland_client;
    pub use super::river_wm::{river_output_v1, river_seat_v1};

    pub mod __interfaces {
        pub use super::super::river_wm::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/river-layer-shell-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("./protocols/river-layer-shell-v1.xml");
}

// 3. River XKB Bindings Protocol (river-xkb-bindings-v1)
pub mod river_xkb {
    pub extern crate bitflags;
    pub extern crate wayland_backend;
    pub extern crate wayland_client;
    pub use super::river_wm::river_seat_v1;
    pub use wayland_client::protocol::wl_seat;

    pub mod __interfaces {
        pub use super::super::river_wm::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/river-xkb-bindings-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("./protocols/river-xkb-bindings-v1.xml");
}

// 4. River Input Management Protocol (river-input-management-v1)
pub mod river_input {
    pub extern crate wayland_backend;
    pub extern crate wayland_client;
    pub use wayland_client::protocol::wl_output;

    pub mod __interfaces {
        pub use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/river-input-management-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("./protocols/river-input-management-v1.xml");
}

// 5. River XKB Config Protocol (river-xkb-config-v1)
pub mod river_xkb_config {
    pub extern crate wayland_backend;
    pub extern crate wayland_client;
    pub use super::river_input::river_input_device_v1;

    pub mod __interfaces {
        pub use super::super::river_input::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/river-xkb-config-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("./protocols/river-xkb-config-v1.xml");
}
