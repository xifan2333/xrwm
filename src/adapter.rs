//! Compositor Adapter bridging pure DomainModel state and Wayland protocol proxies.
//!
//! Maintains numeric ID <-> Wayland protocol handle mappings, executes algebraic
//! `DomainEffect` commands via Wayland RPCs, and isolates proxy lifecycles from domain logic.

use std::collections::HashMap;

use wayland_backend::client::ObjectId;

use crate::domain::DomainEffect;
use crate::protocol::{
    river_output_v1::RiverOutputV1, river_seat_v1::RiverSeatV1,
    river_window_manager_v1::RiverWindowManagerV1, river_window_v1::RiverWindowV1,
};
use crate::status::StatusBroadcaster;

/// Protocol driver and proxy registry executing domain effects against River Wayland proxies.
#[derive(Debug, Default)]
pub struct CompositorAdapter {
    pub window_proxies: HashMap<u32, RiverWindowV1>,
    pub output_proxies: HashMap<ObjectId, RiverOutputV1>,
    pub seat_proxies: HashMap<ObjectId, RiverSeatV1>,
    pub river_wm: Option<RiverWindowManagerV1>,
    pub broadcaster: StatusBroadcaster,
}

impl CompositorAdapter {
    /// Creates an empty CompositorAdapter.
    pub fn new() -> Self {
        Self {
            window_proxies: HashMap::new(),
            output_proxies: HashMap::new(),
            seat_proxies: HashMap::new(),
            river_wm: None,
            broadcaster: StatusBroadcaster::new(),
        }
    }

    /// Registers a window protocol proxy under its numeric ID.
    pub fn register_window(&mut self, id: u32, proxy: RiverWindowV1) {
        self.window_proxies.insert(id, proxy);
    }

    /// Removes and returns a window protocol proxy by its numeric ID.
    pub fn remove_window(&mut self, id: u32) -> Option<RiverWindowV1> {
        self.window_proxies.remove(&id)
    }

    /// Returns a reference to a window protocol proxy, if registered.
    pub fn window_proxy(&self, id: u32) -> Option<&RiverWindowV1> {
        self.window_proxies.get(&id)
    }

    /// Registers an output protocol proxy under its Wayland ObjectId.
    pub fn register_output(&mut self, id: ObjectId, proxy: RiverOutputV1) {
        self.output_proxies.insert(id, proxy);
    }

    /// Removes and returns an output protocol proxy by its ObjectId.
    pub fn remove_output(&mut self, id: &ObjectId) -> Option<RiverOutputV1> {
        self.output_proxies.remove(id)
    }

    /// Returns a reference to an output protocol proxy, if registered.
    pub fn output_proxy(&self, id: &ObjectId) -> Option<&RiverOutputV1> {
        self.output_proxies.get(id)
    }

    /// Registers a seat protocol proxy under its Wayland ObjectId.
    pub fn register_seat(&mut self, id: ObjectId, proxy: RiverSeatV1) {
        self.seat_proxies.insert(id, proxy);
    }

    /// Removes and returns a seat protocol proxy by its ObjectId.
    pub fn remove_seat(&mut self, id: &ObjectId) -> Option<RiverSeatV1> {
        self.seat_proxies.remove(id)
    }

    /// Returns a reference to a seat protocol proxy, if registered.
    pub fn seat_proxy(&self, id: &ObjectId) -> Option<&RiverSeatV1> {
        self.seat_proxies.get(id)
    }

    /// Binds the active RiverWindowManagerV1 protocol proxy.
    pub fn set_river_wm(&mut self, proxy: RiverWindowManagerV1) {
        self.river_wm = Some(proxy);
    }

    /// Executes a single domain effect against registered Wayland proxies.
    pub fn execute_effect(&mut self, effect: &DomainEffect, state: &mut crate::state::AppState) {
        match effect {
            DomainEffect::ProposeDimensions {
                window_id,
                width,
                height,
            } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    proxy.propose_dimensions(
                        (*width).min(i32::MAX as u32) as i32,
                        (*height).min(i32::MAX as u32) as i32,
                    );
                }
            }
            DomainEffect::InformFullscreen {
                window_id,
                output_id,
            } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    let out_proxy = output_id
                        .as_ref()
                        .and_then(|id| self.output_proxies.get(id));
                    if let Some(out) = out_proxy {
                        proxy.fullscreen(out);
                    }
                    proxy.inform_fullscreen();
                }
            }
            DomainEffect::InformNotFullscreen { window_id } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    proxy.exit_fullscreen();
                    proxy.inform_not_fullscreen();
                }
            }
            DomainEffect::InformResizeStart { window_id } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    proxy.inform_resize_start();
                }
            }
            DomainEffect::InformResizeEnd { window_id } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    proxy.inform_resize_end();
                }
            }
            DomainEffect::CloseWindow { window_id } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    proxy.close();
                }
            }
            DomainEffect::DestroyWindow { window_id } => {
                if let Some(proxy) = self.window_proxies.remove(window_id) {
                    proxy.destroy();
                }
            }
            DomainEffect::SetTiled { window_id, tiled } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    if *tiled {
                        proxy.set_tiled(crate::protocol::river_window_v1::Edges::all());
                    } else {
                        proxy.set_tiled(crate::protocol::river_window_v1::Edges::empty());
                    }
                }
            }
            DomainEffect::SetDecorationMode { window_id, ssd } => {
                if let Some(proxy) = self.window_proxies.get(window_id) {
                    if *ssd {
                        proxy.use_ssd();
                    } else {
                        proxy.use_csd();
                    }
                }
            }
            DomainEffect::SetSeatFocus { seat_id, window_id } => {
                if let Some(seat_proxy) = self.seat_proxies.get(seat_id) {
                    if let Some(win_id) = window_id {
                        if let Some(win_proxy) = self.window_proxies.get(win_id) {
                            seat_proxy.focus_window(win_proxy);
                        } else {
                            seat_proxy.clear_focus();
                        }
                    } else {
                        seat_proxy.clear_focus();
                    }
                }
            }
            DomainEffect::SeatOpStart { seat_id } => {
                if let Some(seat_proxy) = self.seat_proxies.get(seat_id) {
                    seat_proxy.op_start_pointer();
                }
            }
            DomainEffect::SeatOpEnd { seat_id } => {
                if let Some(seat_proxy) = self.seat_proxies.get(seat_id) {
                    seat_proxy.op_end();
                }
            }
            DomainEffect::ManageFinish => {
                if let Some(ref wm) = self.river_wm {
                    wm.manage_finish();
                }
            }
            DomainEffect::BroadcastStatus => {
                state.broadcast_status();
            }
            DomainEffect::ExecuteAction { tokens } => {
                state.execute_action_tokens(tokens);
            }
        }
    }

    /// Executes an ordered sequence of domain effects.
    pub fn execute_effects(
        &mut self,
        effects: &[DomainEffect],
        state: &mut crate::state::AppState,
    ) {
        for effect in effects {
            self.execute_effect(effect, state);
        }
    }
}
