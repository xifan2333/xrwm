//! xrwm - River 0.4+ dynamic tiling Wayland window manager library core.

pub mod actions;
pub mod animation;
pub mod binds;
pub mod dispatch;
pub mod ipc;
pub mod layout;
pub mod logging;
pub mod nav;
pub mod protocol;
pub mod rule;
pub mod seat;
pub mod state;
pub mod status;
pub mod tag;

pub use actions::*;
pub use animation::*;
pub use binds::*;
pub use ipc::*;
pub use layout::*;
pub use logging::*;
pub use nav::*;
pub use rule::*;
pub use seat::*;
pub use state::*;
pub use status::*;
pub use tag::*;
