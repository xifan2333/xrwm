//! Core Window Manager module (AppState, Seat, Actions, and Dispatchers).

pub mod actions;
pub mod dispatch;
pub mod seat;
pub mod state;

pub use seat::PointerAction;
pub use seat::PointerBinding;
pub use seat::SeatItem;
pub use seat::SeatOp;
pub use state::AppState;
pub use state::OutputItem;
pub use state::WindowItem;
pub use state::WindowRule;
pub use state::hex_to_river_rgba;
pub use state::spawn_init_script;
