//! Seat, pointer operations, and gesture tracking.

use std::collections::HashMap;

use wayland_backend::client::ObjectId;

use crate::protocol::{
    river_pointer_binding_v1::RiverPointerBindingV1,
    river_seat_v1::RiverSeatV1,
    river_window_v1::{Edges, RiverWindowV1},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerAction {
    None,
    Move,
    Resize,
    Command(Vec<String>),
}

impl PointerAction {
    pub fn from_tokens(tokens: &[String]) -> Self {
        if tokens.is_empty() {
            return Self::None;
        }
        match tokens[0].as_str() {
            "move-view" | "move" => Self::Move,
            "resize-view" | "resize" => Self::Resize,
            "toggle-float" | "toggle-floating" => Self::Command(vec!["toggle-float".to_string()]),
            _ => Self::Command(tokens.to_vec()),
        }
    }
}

#[derive(Debug)]
pub struct PointerBinding {
    pub proxy: RiverPointerBindingV1,
    pub mode: String,
    pub action: PointerAction,
}

#[derive(Debug)]
pub enum SeatOp {
    None,
    Move {
        proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
    },
    Resize {
        proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
        start_width: u32,
        start_height: u32,
        edges: Edges,
    },
}

#[derive(Debug)]
pub struct SeatItem {
    pub proxy: RiverSeatV1,
    pub removed: bool,
    pub focused: Option<RiverWindowV1>,
    pub hovered: Option<RiverWindowV1>,
    pub interacted: Option<RiverWindowV1>,
    pub pending_action: PointerAction,
    pub op: SeatOp,
    pub op_dx: i32,
    pub op_dy: i32,
    pub op_release: bool,
    pub pointer_bindings: HashMap<ObjectId, PointerBinding>,
}

impl SeatItem {
    pub fn new(proxy: RiverSeatV1) -> Self {
        Self {
            proxy,
            removed: false,
            focused: None,
            hovered: None,
            interacted: None,
            pending_action: PointerAction::None,
            op: SeatOp::None,
            op_dx: 0,
            op_dy: 0,
            op_release: false,
            pointer_bindings: HashMap::new(),
        }
    }
}
