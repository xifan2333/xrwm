//! Core Window Manager module (AppState, Seat, Actions, and Dispatchers).

pub mod actions;
pub mod dispatch;
pub mod seat;
pub mod state;

pub use seat::{PointerAction, PointerBinding, SeatItem, SeatOp};
pub use state::{
    AppState, OutputItem, WindowItem, WindowRule, hex_to_river_rgba, spawn_init_script,
};
