//! Core Window Manager module (AppState, Seat, Actions, and Dispatchers).

pub mod actions;
pub mod binds;
pub mod dispatch;
pub mod rule;
pub mod seat;
pub mod state;

#[cfg(test)]
mod tests;

pub use rule::WindowRule;
pub use rule::apply_rules_to_window;
pub use rule::glob_match;
pub use seat::CursorWarp;
pub use seat::FocusFollowsCursor;
pub use seat::LayerShellFocus;
pub use seat::PointerAction;
pub use seat::PointerBinding;
pub use seat::SeatItem;
pub use seat::SeatOp;
pub use state::AppState;
pub use state::AttachMode;
pub use state::MIN_WINDOW_DIMENSION;
pub use state::OutputItem;
pub use state::WindowItem;
pub use state::hex_to_river_rgba;
pub use state::reap_zombies;
pub use state::spawn_init_script;
pub use state::spawn_init_script_at;
pub use state::spawn_init_script_from_config;
