//! Seat, pointer operations, and gesture tracking.

use std::collections::HashMap;

use wayland_backend::client::ObjectId;

use crate::protocol::{
    river_layer_shell_seat_v1::RiverLayerShellSeatV1,
    river_pointer_binding_v1::RiverPointerBindingV1,
    river_seat_v1::RiverSeatV1,
    river_window_v1::{Edges, RiverWindowV1},
    wl_pointer::WlPointer,
    wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CursorWarp {
    #[default]
    Disabled,
    OnOutputChange,
    OnFocusChange,
}

impl CursorWarp {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" | "none" => Ok(Self::Disabled),
            "on-output-change" | "output" => Ok(Self::OnOutputChange),
            "on-focus-change" | "focus" => Ok(Self::OnFocusChange),
            _ => Err(format!(
                "Invalid cursor warp mode: '{s}', expected disabled|on-output-change|on-focus-change"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FocusFollowsCursor {
    Disabled,
    #[default]
    Normal,
    Always,
}

impl FocusFollowsCursor {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" | "false" => Ok(Self::Disabled),
            "normal" | "on" | "true" => Ok(Self::Normal),
            "always" => Ok(Self::Always),
            _ => Err(format!(
                "Invalid focus-follows-cursor mode: '{s}', expected disabled|normal|always"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerShellFocus {
    #[default]
    None,
    NonExclusive,
    Exclusive,
}

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
            "toggle-float" => Self::Command(vec!["toggle-float".to_string()]),
            _ => Self::Command(tokens.to_vec()),
        }
    }
}

#[derive(Debug)]
pub struct PointerBinding {
    pub proxy: RiverPointerBindingV1,
    pub mode: String,
    pub modifiers: crate::protocol::river_seat_v1::Modifiers,
    pub button: u32,
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
    TiledResize {
        start_ratio: f32,
    },
    TiledStackResize {
        start_ratio: f32,
    },
    TiledMove {
        proxy: RiverWindowV1,
        start_win_id: u32,
    },
}

#[derive(Debug)]
pub struct SeatItem {
    pub proxy: RiverSeatV1,
    pub ls_seat: Option<RiverLayerShellSeatV1>,
    pub removed: bool,
    pub focused: Option<RiverWindowV1>,
    pub last_focused_window: Option<ObjectId>,
    pub layer_focus: LayerShellFocus,
    pub hovered: Option<RiverWindowV1>,
    pub interacted: Option<RiverWindowV1>,
    pub pending_action: PointerAction,
    pub op: SeatOp,
    pub op_dx: i32,
    pub op_dy: i32,
    pub op_release: bool,
    pub cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pub wl_pointer: Option<WlPointer>,
    pub pending_warp: Option<(i32, i32)>,
    pub pointer_bindings: HashMap<ObjectId, PointerBinding>,
}

impl SeatItem {
    pub fn new(proxy: RiverSeatV1) -> Self {
        Self {
            proxy,
            ls_seat: None,
            removed: false,
            focused: None,
            last_focused_window: None,
            layer_focus: LayerShellFocus::None,
            hovered: None,
            interacted: None,
            pending_action: PointerAction::None,
            op: SeatOp::None,
            op_dx: 0,
            op_dy: 0,
            op_release: false,
            cursor_shape_device: None,
            wl_pointer: None,
            pending_warp: None,
            pointer_bindings: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_warp_parse() {
        assert_eq!(CursorWarp::parse("disabled").unwrap(), CursorWarp::Disabled);
        assert_eq!(
            CursorWarp::parse("on-output-change").unwrap(),
            CursorWarp::OnOutputChange
        );
        assert_eq!(
            CursorWarp::parse("on-focus-change").unwrap(),
            CursorWarp::OnFocusChange
        );
        assert_eq!(
            CursorWarp::parse("output").unwrap(),
            CursorWarp::OnOutputChange
        );
        assert!(CursorWarp::parse("invalid").is_err());
    }

    #[test]
    fn test_focus_follows_cursor_parse() {
        assert_eq!(
            FocusFollowsCursor::parse("disabled").unwrap(),
            FocusFollowsCursor::Disabled
        );
        assert_eq!(
            FocusFollowsCursor::parse("normal").unwrap(),
            FocusFollowsCursor::Normal
        );
        assert_eq!(
            FocusFollowsCursor::parse("always").unwrap(),
            FocusFollowsCursor::Always
        );
        assert!(FocusFollowsCursor::parse("invalid").is_err());
    }
}
