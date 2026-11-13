//! Seat, pointer operations, and gesture tracking.

use std::collections::HashMap;

use wayland_backend::client::ObjectId;
use wayland_client::Proxy;

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

/// Mutually exclusive state machine tracking interactive pointer operations and cursor shapes on a seat.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SeatGestureState {
    /// No interactive pointer operation in progress.
    #[default]
    Idle,
    /// Interactive floating window drag-move operation.
    FloatingMove {
        proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
        dx: i32,
        dy: i32,
    },
    /// Interactive floating window corner/edge resize operation.
    FloatingResize {
        proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
        start_width: u32,
        start_height: u32,
        edges: Edges,
        dx: i32,
        dy: i32,
    },
    /// Interactive tiled master split ratio resizing.
    TiledResize { start_ratio: f32, dx: i32 },
    /// Interactive tiled secondary stack split ratio resizing.
    TiledStackResize { start_ratio: f32, dy: i32 },
    /// Interactive tiled window drag-swap operation.
    TiledDragSwap {
        proxy: RiverWindowV1,
        start_win_id: u32,
    },
}

impl SeatGestureState {
    /// Returns true if no interactive gesture is active.
    #[inline]
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Returns true if an interactive gesture is actively in progress.
    #[inline]
    pub fn is_active(&self) -> bool {
        !self.is_idle()
    }

    /// Returns the active window proxy being manipulated, if any.
    pub fn target_proxy(&self) -> Option<&RiverWindowV1> {
        match self {
            Self::FloatingMove { proxy, .. }
            | Self::FloatingResize { proxy, .. }
            | Self::TiledDragSwap { proxy, .. } => Some(proxy),
            Self::Idle | Self::TiledResize { .. } | Self::TiledStackResize { .. } => None,
        }
    }
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
    pub needs_key_binding_sync: bool,
    pub needs_pointer_binding_sync: bool,
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
            needs_key_binding_sync: true,
            needs_pointer_binding_sync: true,
        }
    }

    pub fn set_focused_window(&mut self, proxy: Option<RiverWindowV1>) {
        if proxy.is_some() && self.layer_focus == LayerShellFocus::NonExclusive {
            self.layer_focus = LayerShellFocus::None;
            self.last_focused_window = None;
        }
        self.focused = proxy;
    }

    /// Initiates an interactive floating move operation.
    pub fn start_floating_move(&mut self, proxy: RiverWindowV1, x: i32, y: i32) {
        if let Some(ref dev) = self.cursor_shape_device {
            dev.set_shape(
                0,
                crate::protocol::wp_cursor_shape_device_v1::Shape::Grabbing,
            );
        }
        self.proxy.op_start_pointer();
        self.op = SeatOp::Move {
            proxy,
            start_x: x,
            start_y: y,
        };
        self.op_dx = 0;
        self.op_dy = 0;
    }

    /// Initiates an interactive tiled window drag-swap operation.
    pub fn start_tiled_move(&mut self, proxy: RiverWindowV1, start_win_id: u32) {
        if let Some(ref dev) = self.cursor_shape_device {
            dev.set_shape(
                0,
                crate::protocol::wp_cursor_shape_device_v1::Shape::Grabbing,
            );
        }
        self.proxy.op_start_pointer();
        self.op = SeatOp::TiledMove {
            proxy,
            start_win_id,
        };
        self.op_dx = 0;
        self.op_dy = 0;
    }

    /// Initiates an interactive floating corner resize operation.
    pub fn start_floating_resize(
        &mut self,
        proxy: RiverWindowV1,
        rect: crate::layout::Rect,
        pointer: (i32, i32),
    ) {
        self.proxy.op_start_pointer();
        let cx = rect.x + rect.width as i32 / 2;
        let cy = rect.y + rect.height as i32 / 2;
        let mut edges = Edges::empty();
        if pointer.0 < cx {
            edges |= Edges::Left;
        } else {
            edges |= Edges::Right;
        }
        if pointer.1 < cy {
            edges |= Edges::Top;
        } else {
            edges |= Edges::Bottom;
        }

        let shape = if edges.contains(Edges::Top) && edges.contains(Edges::Left) {
            crate::protocol::wp_cursor_shape_device_v1::Shape::NwResize
        } else if edges.contains(Edges::Top) && edges.contains(Edges::Right) {
            crate::protocol::wp_cursor_shape_device_v1::Shape::NeResize
        } else if edges.contains(Edges::Bottom) && edges.contains(Edges::Left) {
            crate::protocol::wp_cursor_shape_device_v1::Shape::SwResize
        } else {
            crate::protocol::wp_cursor_shape_device_v1::Shape::SeResize
        };
        if let Some(ref dev) = self.cursor_shape_device {
            dev.set_shape(0, shape);
        }

        proxy.inform_resize_start();
        self.op = SeatOp::Resize {
            proxy,
            start_x: rect.x,
            start_y: rect.y,
            start_width: rect.width,
            start_height: rect.height,
            edges,
        };
        self.op_dx = 0;
        self.op_dy = 0;
    }

    /// Initiates an interactive tiled split ratio resizing.
    pub fn start_tiled_resize(
        &mut self,
        main_location: crate::layout::MainLocation,
        is_stack_split: bool,
        start_ratio: f32,
    ) {
        self.proxy.op_start_pointer();
        if is_stack_split {
            let shape = match main_location {
                crate::layout::MainLocation::Left | crate::layout::MainLocation::Right => {
                    crate::protocol::wp_cursor_shape_device_v1::Shape::NsResize
                }
                crate::layout::MainLocation::Top | crate::layout::MainLocation::Bottom => {
                    crate::protocol::wp_cursor_shape_device_v1::Shape::EwResize
                }
            };
            if let Some(ref dev) = self.cursor_shape_device {
                dev.set_shape(0, shape);
            }
            self.op = SeatOp::TiledStackResize { start_ratio };
        } else {
            let shape = match main_location {
                crate::layout::MainLocation::Left | crate::layout::MainLocation::Right => {
                    crate::protocol::wp_cursor_shape_device_v1::Shape::EwResize
                }
                crate::layout::MainLocation::Top | crate::layout::MainLocation::Bottom => {
                    crate::protocol::wp_cursor_shape_device_v1::Shape::NsResize
                }
            };
            if let Some(ref dev) = self.cursor_shape_device {
                dev.set_shape(0, shape);
            }
            self.op = SeatOp::TiledResize { start_ratio };
        }
        self.op_dx = 0;
        self.op_dy = 0;
    }

    /// Synchronizes compositor focus against seat focus state.
    pub fn sync_focus(&mut self) {
        if self.layer_focus == LayerShellFocus::None {
            if let Some(target) = &self.focused {
                let target_id = target.id();
                if self.last_focused_window.as_ref() != Some(&target_id) {
                    self.proxy.focus_window(target);
                    self.last_focused_window = Some(target_id);
                }
            } else if self.last_focused_window.is_some() {
                self.proxy.clear_focus();
                self.last_focused_window = None;
            }
        } else {
            self.last_focused_window = None;
        }
    }

    /// Applies motion deltas for active pointer operations.
    pub fn apply_motion(
        &self,
        windows: &mut [crate::window::WindowItem],
        layout_config: &mut crate::layout::LayoutConfig,
        focused_out_area: Option<crate::layout::Rect>,
    ) {
        match &self.op {
            SeatOp::Move {
                proxy,
                start_x,
                start_y,
            } => {
                if let Some(w) = windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.set_position(start_x + self.op_dx, start_y + self.op_dy);
                }
            }
            SeatOp::Resize {
                proxy,
                start_x,
                start_y,
                start_width,
                start_height,
                edges,
            } => {
                if let Some(w) = windows.iter_mut().find(|w| &w.proxy == proxy) {
                    let mut new_w = *start_width as i32;
                    let mut new_h = *start_height as i32;
                    let mut new_x = *start_x;
                    let mut new_y = *start_y;

                    if edges.contains(Edges::Right) {
                        new_w = (*start_width as i32 + self.op_dx)
                            .max(crate::window::MIN_WINDOW_DIMENSION as i32);
                    } else if edges.contains(Edges::Left) {
                        new_w = (*start_width as i32 - self.op_dx)
                            .max(crate::window::MIN_WINDOW_DIMENSION as i32);
                        new_x = *start_x + (*start_width as i32 - new_w);
                    }

                    if edges.contains(Edges::Bottom) {
                        new_h = (*start_height as i32 + self.op_dy)
                            .max(crate::window::MIN_WINDOW_DIMENSION as i32);
                    } else if edges.contains(Edges::Top) {
                        new_h = (*start_height as i32 - self.op_dy)
                            .max(crate::window::MIN_WINDOW_DIMENSION as i32);
                        new_y = *start_y + (*start_height as i32 - new_h);
                    }

                    w.update_floating_geometry(crate::layout::Rect::new(
                        new_x,
                        new_y,
                        new_w as u32,
                        new_h as u32,
                    ));
                }
            }
            SeatOp::TiledResize { start_ratio } => {
                let Some(focused_out_area) = focused_out_area else {
                    return;
                };
                let usable_w = focused_out_area.width as f32;
                let usable_h = focused_out_area.height as f32;
                match layout_config.main_location {
                    crate::layout::MainLocation::Left => {
                        let delta_ratio = (self.op_dx as f32) / usable_w.max(1.0);
                        layout_config.split_ratio = (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                    }
                    crate::layout::MainLocation::Right => {
                        let delta_ratio = -(self.op_dx as f32) / usable_w.max(1.0);
                        layout_config.split_ratio = (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                    }
                    crate::layout::MainLocation::Top => {
                        let delta_ratio = (self.op_dy as f32) / usable_h.max(1.0);
                        layout_config.split_ratio = (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                    }
                    crate::layout::MainLocation::Bottom => {
                        let delta_ratio = -(self.op_dy as f32) / usable_h.max(1.0);
                        layout_config.split_ratio = (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                    }
                }
            }
            SeatOp::TiledStackResize { start_ratio } => {
                let Some(focused_out_area) = focused_out_area else {
                    return;
                };
                let usable_w = focused_out_area.width as f32;
                let usable_h = focused_out_area.height as f32;
                match layout_config.main_location {
                    crate::layout::MainLocation::Left | crate::layout::MainLocation::Right => {
                        let delta_ratio = (self.op_dy as f32) / usable_h.max(1.0);
                        layout_config.stack_split_ratio =
                            (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                    }
                    crate::layout::MainLocation::Top | crate::layout::MainLocation::Bottom => {
                        let delta_ratio = (self.op_dx as f32) / usable_w.max(1.0);
                        layout_config.stack_split_ratio =
                            (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                    }
                }
            }
            SeatOp::TiledMove { .. } | SeatOp::None => {}
        }
    }

    /// Commits a released pointer operation and ends interaction.
    pub fn commit_release(
        &mut self,
        windows: &mut [crate::window::WindowItem],
        pointer: (i32, i32),
        is_window_visible: impl Fn(&crate::window::WindowItem) -> bool,
    ) -> bool {
        if !self.op_release {
            return false;
        }

        if let SeatOp::Resize { proxy, .. } = &self.op {
            proxy.inform_resize_end();
        }
        self.proxy.op_end();

        let target_proxy = match &self.op {
            SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => Some(proxy.clone()),
            _ => None,
        };
        if let Some(target) = target_proxy
            && let Some(w) = windows.iter_mut().find(|w| w.proxy == target)
        {
            w.finish_interaction();
        }

        if let SeatOp::TiledMove { start_win_id, .. } = self.op {
            let px = pointer.0;
            let py = pointer.1;
            let dropped_on = windows
                .iter()
                .find(|other| {
                    !other.closed
                        && other.layer().policy().tiled_layout
                        && other.id != start_win_id
                        && is_window_visible(other)
                        && px >= other.x
                        && px <= (other.x + other.effective_width() as i32)
                        && py >= other.y
                        && py <= (other.y + other.effective_height() as i32)
                })
                .map(|w| w.id);

            if let Some(target_id) = dropped_on {
                let i1 = windows.iter().position(|w| w.id == start_win_id);
                let i2 = windows.iter().position(|w| w.id == target_id);
                if let (Some(idx1), Some(idx2)) = (i1, i2) {
                    windows.swap(idx1, idx2);
                    tracing::debug!(
                        source_window_id = start_win_id,
                        target_window_id = target_id,
                        "Swapped tiled windows on pointer drop"
                    );
                }
            }
        }

        if let Some(ref dev) = self.cursor_shape_device {
            dev.set_shape(
                0,
                crate::protocol::wp_cursor_shape_device_v1::Shape::Default,
            );
        }

        self.op = SeatOp::None;
        self.op_release = false;
        self.op_dx = 0;
        self.op_dy = 0;
        true
    }
}
