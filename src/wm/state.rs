//! Central application state machine for xrwm.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use wayland_backend::client::ObjectId;
use wayland_client::protocol::wl_registry;
use wayland_client::{Proxy, QueueHandle};

use crate::animation::AnimationController;
use crate::animation::calculate_clip_box;
use crate::animation::interpolate_rect;
use crate::layout::{Layout, LayoutConfig, MasterStackLayout, Rect};
use crate::protocol::{
    river_layer_shell_output_v1::RiverLayerShellOutputV1,
    river_layer_shell_v1::RiverLayerShellV1,
    river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1,
    river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::{Edges, RiverWindowV1},
    river_xkb_bindings_v1::RiverXkbBindingsV1,
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::tag::TagState;
use crate::wm::binds::{ActiveKeyBinding, PendingKeyBinding};
use crate::wm::seat::{LayerShellFocus, PointerAction, SeatItem, SeatOp};

pub const MIN_WINDOW_DIMENSION: u32 = 100;

pub fn parse_hex_color(hex_str: &str) -> Result<(u32, u32, u32, u32), String> {
    let h = hex_str
        .trim_start_matches("0x")
        .trim_start_matches('#')
        .trim();

    if !h.is_ascii() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "Color '{hex_str}' contains non-hexadecimal characters"
        ));
    }

    match h.len() {
        6 => {
            let r =
                u32::from_str_radix(&h[0..2], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let g =
                u32::from_str_radix(&h[2..4], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let b =
                u32::from_str_radix(&h[4..6], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let a = u32::MAX;
            Ok((r, g, b, a))
        }
        8 => {
            let r =
                u32::from_str_radix(&h[0..2], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let g =
                u32::from_str_radix(&h[2..4], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let b =
                u32::from_str_radix(&h[4..6], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let a =
                u32::from_str_radix(&h[6..8], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let premultiply = |channel: u32| ((channel as u64 * a as u64) / u32::MAX as u64) as u32;
            Ok((premultiply(r), premultiply(g), premultiply(b), a))
        }
        _ => Err(format!(
            "Color '{hex_str}' has invalid length (expected 6 or 8 hex digits, got {})",
            h.len()
        )),
    }
}

pub fn hex_to_river_rgba(hex_str: &str) -> (u32, u32, u32, u32) {
    parse_hex_color(hex_str).unwrap_or((u32::MAX, u32::MAX, u32::MAX, u32::MAX))
}

pub fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        if let Some(inner) = prefix.strip_prefix('*') {
            return text.contains(inner);
        }
        return text.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return text.ends_with(suffix);
    }
    pattern == text
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AttachMode {
    #[default]
    Top,
    Bottom,
    Above,
    Below,
    After(u32),
}

impl AttachMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        match parts.first().map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("top") => Ok(Self::Top),
            Some("bottom") => Ok(Self::Bottom),
            Some("above") => Ok(Self::Above),
            Some("below") => Ok(Self::Below),
            Some("after") => {
                if let Some(n_str) = parts.get(1) {
                    let n = n_str
                        .parse::<u32>()
                        .map_err(|_| format!("Invalid count for after: {n_str}"))?;
                    Ok(Self::After(n))
                } else {
                    Err("Usage: after <N>".to_string())
                }
            }
            _ => Err(format!(
                "Invalid attach mode: '{s}', expected top|bottom|above|below|after <N>"
            )),
        }
    }

    /// Computes the index in `windows` where a new window should be inserted.
    pub fn calculate_insert_index(&self, focused_idx: Option<usize>, total_len: usize) -> usize {
        match self {
            AttachMode::Top => 0,
            AttachMode::Bottom => total_len,
            AttachMode::Above => focused_idx.unwrap_or(0),
            AttachMode::Below => focused_idx
                .map(|i| (i + 1).min(total_len))
                .unwrap_or(total_len),
            AttachMode::After(n) => (*n as usize).min(total_len),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowRule {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub float: Option<bool>,
    pub ssd: Option<bool>,
    pub tags: Option<TagMask>,
    pub dimensions: Option<(u32, u32)>,
    pub position: Option<(i32, i32)>,
    pub fullscreen: Option<bool>,
    pub output: Option<String>,
}

#[derive(Debug)]
pub struct WindowItem {
    pub id: u32,
    pub proxy: RiverWindowV1,
    pub node: RiverNodeV1,
    pub initial_managed: bool,
    pub initial_rendered: bool,
    pub closed: bool,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub tags: TagMask,
    pub floating: bool,
    pub fullscreen: bool,
    pub pending_close: bool,
    pub pending_fullscreen_change: bool,
    pub float_geo: Option<Rect>,
    pub ssd: bool,
    pub last_applied_ssd: Option<bool>,
    pub output: Option<ObjectId>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    // Animation & visual geometry tracking
    pub visual_geo: Option<Rect>,
    pub anim_start_geo: Option<Rect>,
    pub anim_target_geo: Option<Rect>,
    // None means no proposal has been sent; zero lets the client choose its size.
    pub last_proposed_w: Option<u32>,
    pub last_proposed_h: Option<u32>,
}

#[derive(Debug)]
pub struct OutputItem {
    pub proxy: RiverOutputV1,
    pub ls_output: Option<RiverLayerShellOutputV1>,
    pub removed: bool,
    pub usable_area: Rect,
    pub has_custom_usable_area: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub struct AppState {
    pub river_wm: Option<RiverWindowManagerV1>,
    pub river_xkb: Option<RiverXkbBindingsV1>,
    pub river_layer: Option<RiverLayerShellV1>,
    pub wl_registry: Option<wl_registry::WlRegistry>,
    pub cursor_shape_manager: Option<WpCursorShapeManagerV1>,
    pub pointer: (i32, i32),
    pub windows: Vec<WindowItem>,
    pub outputs: HashMap<ObjectId, OutputItem>,
    pub focused_output: Option<ObjectId>,
    pub seats: HashMap<ObjectId, SeatItem>,

    pub tag_state: TagState,
    pub previous_focused_tags: TagMask,
    pub layout_config: LayoutConfig,
    pub border_width: u32,
    pub border_color_focused: String,
    pub border_color_unfocused: String,
    pub border_color_urgent: String,
    pub rules: Vec<WindowRule>,
    pub pending_key_bindings: Vec<PendingKeyBinding>,
    pub key_bindings: HashMap<ObjectId, ActiveKeyBinding>,
    pub pending_pointer_bindings: Vec<crate::wm::binds::PendingPointerBinding>,
    pub active_mode: String,
    pub modes: Vec<String>,
    pub mode_dirty: bool,
    pub next_view_id: u32,
    pub attach_mode: AttachMode,
    pub cursor_warp: crate::wm::seat::CursorWarp,
    pub focus_follows_cursor: crate::wm::seat::FocusFollowsCursor,
    pub spawn_tagmask: TagMask,
    pub cursor_hide_timeout: u64,
    pub cursor_hide_when_typing: bool,
    pub cursor_hidden: bool,
    pub last_pointer_activity: std::time::Instant,

    pub anim: AnimationController,
    pub tag_slide_dir: Option<crate::animation::SlideDirection>,
    pub tag_anim_old_mask: TagMask,
    pub status_listeners: Vec<(UnixStream, Option<String>)>,
    pub should_exit: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            river_wm: None,
            river_xkb: None,
            river_layer: None,
            wl_registry: None,
            cursor_shape_manager: None,
            pointer: (0, 0),
            windows: Vec::new(),
            outputs: HashMap::new(),
            focused_output: None,
            seats: HashMap::new(),
            tag_state: TagState::new(),
            previous_focused_tags: 1,
            layout_config: LayoutConfig::default(),
            border_width: 2,
            border_color_focused: "0x7aa2f7".to_string(),
            border_color_unfocused: "0x414868".to_string(),
            border_color_urgent: "0xf7768e".to_string(),
            rules: Vec::new(),
            pending_key_bindings: Vec::new(),
            key_bindings: HashMap::new(),
            pending_pointer_bindings: Vec::new(),
            active_mode: "normal".to_string(),
            modes: vec!["normal".to_string(), "locked".to_string()],
            mode_dirty: false,
            next_view_id: 1,
            attach_mode: AttachMode::default(),
            cursor_warp: crate::wm::seat::CursorWarp::default(),
            focus_follows_cursor: crate::wm::seat::FocusFollowsCursor::default(),
            spawn_tagmask: u32::MAX,
            cursor_hide_timeout: 0,
            cursor_hide_when_typing: false,
            cursor_hidden: false,
            last_pointer_activity: std::time::Instant::now(),
            anim: AnimationController::default(),
            tag_slide_dir: None,
            tag_anim_old_mask: TAG_NONE,
            status_listeners: Vec::new(),
            should_exit: false,
        }
    }

    #[inline]
    pub fn manage_dirty(&self) {
        if let Some(wm) = &self.river_wm {
            wm.manage_dirty();
        }
    }

    /// Hides the cursor across all seats if not already hidden.
    pub fn hide_cursor(&mut self) {
        if self.cursor_hidden {
            return;
        }
        self.cursor_hidden = true;
        for seat in self.seats.values_mut() {
            if let Some(ref pointer) = seat.wl_pointer {
                pointer.set_cursor(0, None, 0, 0);
            }
        }
    }

    /// Unhides and restores the default cursor across all seats if hidden.
    pub fn unhide_cursor(&mut self) {
        self.last_pointer_activity = std::time::Instant::now();
        if !self.cursor_hidden {
            return;
        }
        self.cursor_hidden = false;
        for seat in self.seats.values_mut() {
            if let Some(ref dev) = seat.cursor_shape_device {
                dev.set_shape(
                    0,
                    crate::protocol::wp_cursor_shape_device_v1::Shape::Default,
                );
            }
        }
    }

    /// Resolves the currently focused output ID with graceful fallbacks.
    pub fn get_focused_output_id(&self) -> Option<ObjectId> {
        if let Some(ref id) = self.focused_output
            && self.outputs.contains_key(id)
        {
            return Some(id.clone());
        }
        if let Some(win_id) = self.focused_window_id()
            && let Some(w) = self.windows.iter().find(|w| w.id == win_id)
            && let Some(ref out) = w.output
            && self.outputs.contains_key(out)
        {
            return Some(out.clone());
        }
        self.outputs.keys().next().cloned()
    }

    /// Attaches a new window according to the current `attach_mode`.
    pub fn attach_window(&mut self, item: WindowItem) {
        let focused_id = self.focused_window_id();
        let focused_idx = focused_id.and_then(|id| self.windows.iter().position(|w| w.id == id));
        let idx = self
            .attach_mode
            .calculate_insert_index(focused_idx, self.windows.len());
        self.windows.insert(idx, item);
    }

    pub fn apply_rules_to_window(
        rules: &[WindowRule],
        w: &mut WindowItem,
        usable_area: Option<Rect>,
        outputs: &HashMap<ObjectId, OutputItem>,
    ) {
        for r in rules {
            let app_matches = match &r.app_id {
                Some(pat) => glob_match(pat, w.app_id.as_deref().unwrap_or("")),
                None => true,
            };
            let title_matches = match &r.title {
                Some(pat) => glob_match(pat, w.title.as_deref().unwrap_or("")),
                None => true,
            };
            if app_matches && title_matches {
                if let Some(float) = r.float {
                    w.floating = float;
                }
                if let Some(ssd) = r.ssd {
                    w.ssd = ssd;
                }
                if let Some(tags) = r.tags {
                    w.tags = tags;
                }
                if let Some(fs) = r.fullscreen {
                    w.fullscreen = fs;
                    w.pending_fullscreen_change = true;
                }
                if let Some(ref out_str) = r.output {
                    let matched_out = outputs
                        .iter()
                        .find(|(id, _)| id.to_string() == *out_str)
                        .or_else(|| {
                            if let Ok(num) = out_str.parse::<usize>()
                                && num >= 1
                                && num <= outputs.len()
                            {
                                outputs.keys().nth(num - 1).map(|id| (id, &outputs[id]))
                            } else {
                                None
                            }
                        });
                    if let Some((id, _)) = matched_out {
                        w.output = Some(id.clone());
                    }
                }
                if let Some((width, height)) = r.dimensions {
                    w.width = width;
                    w.height = height;
                    if let Some(usable) = usable_area {
                        let cx = (usable.x as i64
                            + ((usable.width as i64 - width as i64) / 2).max(0))
                        .clamp(i32::MIN as i64, i32::MAX as i64)
                            as i32;
                        let cy = (usable.y as i64
                            + ((usable.height as i64 - height as i64) / 2).max(0))
                        .clamp(i32::MIN as i64, i32::MAX as i64)
                            as i32;
                        w.x = cx;
                        w.y = cy;
                        w.float_geo = Some(Rect::new(cx, cy, width, height));
                    }
                }
                if let Some((px, py)) = r.position {
                    w.x = px;
                    w.y = py;
                    w.float_geo = Some(Rect::new(px, py, w.width, w.height));
                }
            }
        }
    }

    pub fn offscreen_hiding_position(&self) -> (i32, i32) {
        let min_x = self.outputs.values().map(|o| o.x as i64).min().unwrap_or(0);
        let min_y = self.outputs.values().map(|o| o.y as i64).min().unwrap_or(0);
        let hide_x = (min_x - 100_000).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let hide_y = (min_y - 100_000).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        (hide_x, hide_y)
    }

    pub fn focused_window_id(&self) -> Option<u32> {
        let win = self.seats.values().find_map(|s| {
            if s.layer_focus == LayerShellFocus::None {
                s.focused.as_ref()
            } else {
                None
            }
        });
        if let Some(proxy) = win {
            self.windows
                .iter()
                .find(|w| &w.proxy == proxy)
                .map(|w| w.id)
        } else if self
            .seats
            .values()
            .all(|s| s.layer_focus != LayerShellFocus::None)
            && !self.seats.is_empty()
        {
            None
        } else {
            self.windows.first().map(|w| w.id)
        }
    }

    pub fn hovered_window_id(&self) -> Option<u32> {
        self.seats
            .values()
            .find_map(|s| s.hovered.as_ref())
            .and_then(|proxy| self.windows.iter().find(|w| &w.proxy == proxy))
            .map(|w| w.id)
    }

    pub fn sync_occupied_tags(&mut self) {
        let mut mask = TAG_NONE;
        for w in &self.windows {
            if !w.closed {
                mask |= w.tags;
            }
        }
        self.tag_state.occupied = mask;
    }

    fn propose_initial_dimensions(&mut self) {
        // Windows skipped by layout still need a proposal before River can map them.
        for w in self.windows.iter_mut().filter(|w| {
            !w.fullscreen && (w.last_proposed_w.is_none() || w.last_proposed_h.is_none())
        }) {
            w.proxy.propose_dimensions(
                w.width.min(i32::MAX as u32) as i32,
                w.height.min(i32::MAX as u32) as i32,
            );
            w.last_proposed_w = Some(w.width);
            w.last_proposed_h = Some(w.height);
        }
    }

    pub fn handle_manage_start(&mut self, _proxy: &RiverWindowManagerV1, qh: &QueueHandle<Self>) {
        // Register any pending keybindings during the manage sequence
        if !self.pending_key_bindings.is_empty()
            && let Some(ref xkb_mgr) = self.river_xkb
        {
            for pending in self.pending_key_bindings.drain(..) {
                for seat in self.seats.values() {
                    let binding = xkb_mgr.get_xkb_binding(
                        &seat.proxy,
                        pending.keysym,
                        pending.modifiers,
                        qh,
                        (),
                    );
                    if pending.mode == self.active_mode {
                        binding.enable();
                    } else {
                        binding.disable();
                    }
                    self.key_bindings.insert(
                        binding.id(),
                        ActiveKeyBinding {
                            proxy: binding,
                            mode: pending.mode.clone(),
                            modifiers: pending.modifiers,
                            keysym: pending.keysym,
                            action: pending.action.clone(),
                        },
                    );
                }
            }
        }

        // Sync mode activation (enable active mode bindings, disable others)
        if self.mode_dirty {
            for kb in self.key_bindings.values() {
                if kb.mode == self.active_mode {
                    kb.proxy.enable();
                } else {
                    kb.proxy.disable();
                }
            }
            for seat in self.seats.values() {
                for pb in seat.pointer_bindings.values() {
                    if pb.mode == self.active_mode {
                        pb.proxy.enable();
                    } else {
                        pb.proxy.disable();
                    }
                }
            }
            self.mode_dirty = false;
        }

        // Register any pending pointer bindings during the manage sequence
        if !self.pending_pointer_bindings.is_empty() {
            for pending in self.pending_pointer_bindings.drain(..) {
                for (seat_id, seat) in self.seats.iter_mut() {
                    let pb = seat.proxy.get_pointer_binding(
                        pending.button,
                        pending.modifiers,
                        qh,
                        seat_id.clone(),
                    );
                    if pending.mode == self.active_mode {
                        pb.enable();
                    } else {
                        pb.disable();
                    }
                    seat.pointer_bindings.insert(
                        pb.id(),
                        crate::wm::seat::PointerBinding {
                            proxy: pb,
                            mode: pending.mode.clone(),
                            modifiers: pending.modifiers,
                            button: pending.button,
                            action: pending.action.clone(),
                        },
                    );
                }
            }
        }

        // 0. Process any pending close requests inside the manage sequence
        for w in &mut self.windows {
            if w.pending_close {
                w.proxy.close();
                w.pending_close = false;
            }
        }

        // Apply any pending fullscreen requests
        for w in &mut self.windows {
            if w.pending_fullscreen_change {
                if w.fullscreen {
                    let out_proxy = w
                        .output
                        .as_ref()
                        .and_then(|id| self.outputs.get(id))
                        .map(|o| &o.proxy)
                        .or_else(|| self.outputs.values().next().map(|o| &o.proxy));
                    if let Some(out) = out_proxy {
                        w.proxy.fullscreen(out);
                    }
                } else {
                    w.proxy.exit_fullscreen();
                }
                w.pending_fullscreen_change = false;
            }
        }

        // 1. Remove closed windows, ending any interactive operation holding them
        let closed: Vec<RiverWindowV1> = self
            .windows
            .iter()
            .filter(|w| w.closed)
            .map(|w| w.proxy.clone())
            .collect();

        if !closed.is_empty() {
            for seat in self.seats.values_mut() {
                let op_target = match &seat.op {
                    SeatOp::None | SeatOp::TiledResize { .. } | SeatOp::TiledStackResize { .. } => {
                        None
                    }
                    SeatOp::Move { proxy, .. }
                    | SeatOp::Resize { proxy, .. }
                    | SeatOp::TiledMove { proxy, .. } => Some(proxy.clone()),
                };
                if let Some(target) = op_target
                    && closed.iter().any(|c| c == &target)
                {
                    if let SeatOp::Resize { proxy, .. } = &seat.op {
                        proxy.inform_resize_end();
                    }
                    seat.proxy.op_end();
                    seat.op = SeatOp::None;
                    seat.op_release = false;
                    seat.op_dx = 0;
                    seat.op_dy = 0;
                }
                if seat
                    .focused
                    .as_ref()
                    .is_some_and(|f| closed.iter().any(|c| c == f))
                {
                    seat.set_focused_window(None);
                }
                if seat
                    .hovered
                    .as_ref()
                    .is_some_and(|h| closed.iter().any(|c| c == h))
                {
                    seat.hovered = None;
                }
            }
            self.windows.retain(|w| !w.closed);
        }

        // 2. Consume pointer gestures before arranging (so float <-> tile takes effect in this cycle)
        let mut start_move: Vec<(ObjectId, RiverWindowV1)> = Vec::new();
        let mut start_resize: Vec<(ObjectId, RiverWindowV1)> = Vec::new();
        let mut pointer_commands: Vec<Vec<String>> = Vec::new();

        for (id, seat) in self.seats.iter_mut() {
            if let Some(win_proxy) = seat.interacted.take() {
                seat.set_focused_window(Some(win_proxy));
            }

            let action = std::mem::replace(&mut seat.pending_action, PointerAction::None);
            if action == PointerAction::None {
                continue;
            }
            let Some(win_proxy) = seat.hovered.clone() else {
                continue;
            };
            match action {
                PointerAction::Move => start_move.push((id.clone(), win_proxy)),
                PointerAction::Resize => start_resize.push((id.clone(), win_proxy)),
                PointerAction::Command(cmd) => {
                    seat.set_focused_window(Some(win_proxy));
                    pointer_commands.push(cmd);
                }
                PointerAction::None => {}
            }
        }

        for cmd in pointer_commands {
            self.execute_action_tokens(&cmd);
        }

        for (id, win_proxy) in start_move {
            let win_info = self
                .windows
                .iter()
                .find(|w| w.proxy == win_proxy)
                .map(|w| (w.id, w.floating, w.x, w.y));
            if let (Some((win_id, floating, x, y)), Some(seat)) =
                (win_info, self.seats.get_mut(&id))
            {
                if let Some(ref dev) = seat.cursor_shape_device {
                    dev.set_shape(
                        0,
                        crate::protocol::wp_cursor_shape_device_v1::Shape::Grabbing,
                    );
                }
                seat.proxy.op_start_pointer();
                if floating {
                    seat.op = SeatOp::Move {
                        proxy: win_proxy.clone(),
                        start_x: x,
                        start_y: y,
                    };
                } else {
                    seat.op = SeatOp::TiledMove {
                        proxy: win_proxy.clone(),
                        start_win_id: win_id,
                    };
                }
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        for (id, win_proxy) in start_resize {
            let win_info = self
                .windows
                .iter()
                .find(|w| w.proxy == win_proxy)
                .map(|w| (w.id, w.floating, w.x, w.y, w.width, w.height));
            if let (Some((win_id, floating, x, y, w, h)), Some(seat)) =
                (win_info, self.seats.get_mut(&id))
            {
                seat.proxy.op_start_pointer();
                if floating {
                    let cx = x + w as i32 / 2;
                    let cy = y + h as i32 / 2;
                    let mut edges = Edges::empty();
                    if self.pointer.0 < cx {
                        edges |= Edges::Left;
                    } else {
                        edges |= Edges::Right;
                    }
                    if self.pointer.1 < cy {
                        edges |= Edges::Top;
                    } else {
                        edges |= Edges::Bottom;
                    }

                    // Standard L-shaped corner cursor: Top-Left (NwResize), Top-Right (NeResize), Bottom-Left (SwResize), Bottom-Right (SeResize)
                    let shape = if edges.contains(Edges::Top) && edges.contains(Edges::Left) {
                        crate::protocol::wp_cursor_shape_device_v1::Shape::NwResize
                    } else if edges.contains(Edges::Top) && edges.contains(Edges::Right) {
                        crate::protocol::wp_cursor_shape_device_v1::Shape::NeResize
                    } else if edges.contains(Edges::Bottom) && edges.contains(Edges::Left) {
                        crate::protocol::wp_cursor_shape_device_v1::Shape::SwResize
                    } else {
                        crate::protocol::wp_cursor_shape_device_v1::Shape::SeResize
                    };
                    if let Some(ref dev) = seat.cursor_shape_device {
                        dev.set_shape(0, shape);
                    }

                    win_proxy.inform_resize_start();
                    seat.op = SeatOp::Resize {
                        proxy: win_proxy.clone(),
                        start_x: x,
                        start_y: y,
                        start_width: w,
                        start_height: h,
                        edges,
                    };
                } else {
                    let tag_state = self.tag_state;
                    let tiled_wins: Vec<u32> = self
                        .windows
                        .iter()
                        .filter(|w| !w.closed && !w.floating && tag_state.is_view_visible(w.tags))
                        .map(|w| w.id)
                        .collect();
                    let main_count =
                        (self.layout_config.main_count as usize).clamp(1, tiled_wins.len().max(1));
                    let is_stack = tiled_wins
                        .iter()
                        .position(|&id| id == win_id)
                        .map(|idx| idx >= main_count)
                        .unwrap_or(false);
                    let stack_count = tiled_wins.len().saturating_sub(main_count);

                    if is_stack && stack_count >= 2 {
                        let shape = match self.layout_config.main_location {
                            crate::layout::MainLocation::Left
                            | crate::layout::MainLocation::Right => {
                                crate::protocol::wp_cursor_shape_device_v1::Shape::NsResize
                            }
                            crate::layout::MainLocation::Top
                            | crate::layout::MainLocation::Bottom => {
                                crate::protocol::wp_cursor_shape_device_v1::Shape::EwResize
                            }
                        };
                        if let Some(ref dev) = seat.cursor_shape_device {
                            dev.set_shape(0, shape);
                        }
                        seat.op = SeatOp::TiledStackResize {
                            start_ratio: self.layout_config.stack_split_ratio,
                        };
                    } else {
                        let shape = match self.layout_config.main_location {
                            crate::layout::MainLocation::Left
                            | crate::layout::MainLocation::Right => {
                                crate::protocol::wp_cursor_shape_device_v1::Shape::EwResize
                            }
                            crate::layout::MainLocation::Top
                            | crate::layout::MainLocation::Bottom => {
                                crate::protocol::wp_cursor_shape_device_v1::Shape::NsResize
                            }
                        };
                        if let Some(ref dev) = seat.cursor_shape_device {
                            dev.set_shape(0, shape);
                        }
                        seat.op = SeatOp::TiledResize {
                            start_ratio: self.layout_config.split_ratio,
                        };
                    }
                }
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        // 3. Arrange windows for each output
        if self.outputs.is_empty() {
            self.propose_initial_dimensions();
            self.sync_occupied_tags();
            self.broadcast_status();
            _proxy.manage_finish();
            return;
        }

        let layout_engine = MasterStackLayout;
        let tag_state = self.tag_state;
        let mut any_geo_changed = false;

        let active_outputs: Vec<(ObjectId, Rect)> = self
            .outputs
            .iter()
            .map(|(id, o)| (id.clone(), o.usable_area))
            .collect();

        for (out_id, usable_area) in active_outputs {
            let mut tiled_indices: Vec<usize> = Vec::new();
            for (i, w) in self.windows.iter().enumerate() {
                let matches_output = match (&w.output, out_id.is_null()) {
                    (Some(wo), false) => wo == &out_id,
                    _ => true,
                };
                if matches_output
                    && !w.floating
                    && !w.fullscreen
                    && tag_state.is_view_visible(w.tags)
                {
                    tiled_indices.push(i);
                }
            }

            let rects =
                layout_engine.arrange(usable_area, tiled_indices.len(), &self.layout_config);

            for (slot, &idx) in tiled_indices.iter().enumerate() {
                if let Some(rect) = rects.get(slot) {
                    let w = &mut self.windows[idx];
                    let target = *rect;
                    if !w.initial_managed {
                        let start_w = (target.width * 7 / 10).max(10);
                        let start_h = (target.height * 7 / 10).max(10);
                        let start_x = target.x + (target.width as i32 - start_w as i32) / 2;
                        let start_y = target.y + (target.height as i32 - start_h as i32) / 2;
                        w.anim_start_geo = Some(Rect::new(start_x, start_y, start_w, start_h));
                        w.anim_target_geo = Some(target);
                        any_geo_changed = true;
                    } else if w.anim_target_geo != Some(target) {
                        w.anim_start_geo = w.visual_geo.or(w.anim_target_geo).or(Some(target));
                        w.anim_target_geo = Some(target);
                        any_geo_changed = true;
                    }
                    w.x = rect.x;
                    w.y = rect.y;
                    w.width = rect.width;
                    w.height = rect.height;

                    if w.last_proposed_w != Some(rect.width)
                        || w.last_proposed_h != Some(rect.height)
                    {
                        w.proxy.propose_dimensions(
                            rect.width.min(i32::MAX as u32) as i32,
                            rect.height.min(i32::MAX as u32) as i32,
                        );
                        w.last_proposed_w = Some(rect.width);
                        w.last_proposed_h = Some(rect.height);
                    }
                    w.proxy.set_tiled(Edges::all());
                }
            }
        }

        let is_any_pointer_op = self.seats.values().any(|s| !matches!(&s.op, SeatOp::None));

        // Floating windows
        for w in self
            .windows
            .iter_mut()
            .filter(|w| w.floating && !w.fullscreen)
        {
            let is_visible = tag_state.is_view_visible(w.tags);
            let target = Rect::new(w.x, w.y, w.width, w.height);
            if !is_any_pointer_op {
                if !w.initial_managed {
                    if is_visible {
                        let start_w = ((target.width as u64 * 7 / 10) as u32).max(10);
                        let start_h = ((target.height as u64 * 7 / 10) as u32).max(10);
                        let start_x = (target.x as i64 + (target.width as i64 - start_w as i64) / 2)
                            .clamp(i32::MIN as i64, i32::MAX as i64)
                            as i32;
                        let start_y = (target.y as i64
                            + (target.height as i64 - start_h as i64) / 2)
                            .clamp(i32::MIN as i64, i32::MAX as i64)
                            as i32;
                        w.anim_start_geo = Some(Rect::new(start_x, start_y, start_w, start_h));
                        w.anim_target_geo = Some(target);
                        any_geo_changed = true;
                    } else {
                        w.anim_start_geo = Some(target);
                        w.anim_target_geo = Some(target);
                        w.visual_geo = Some(target);
                    }
                } else if is_visible && w.anim_target_geo != Some(target) {
                    w.anim_start_geo = w.visual_geo.or(w.anim_target_geo).or(Some(target));
                    w.anim_target_geo = Some(target);
                    any_geo_changed = true;
                } else if !is_visible && w.anim_target_geo != Some(target) {
                    w.anim_start_geo = Some(target);
                    w.anim_target_geo = Some(target);
                    w.visual_geo = Some(target);
                }
            }
            if w.last_proposed_w != Some(w.width) || w.last_proposed_h != Some(w.height) {
                w.proxy.propose_dimensions(
                    w.width.min(i32::MAX as u32) as i32,
                    w.height.min(i32::MAX as u32) as i32,
                );
                w.last_proposed_w = Some(w.width);
                w.last_proposed_h = Some(w.height);
            }
            w.proxy.set_tiled(Edges::empty());
        }

        // Apply decoration requests (SSD / CSD)
        for w in &mut self.windows {
            if w.last_applied_ssd != Some(w.ssd) {
                if w.ssd {
                    w.proxy.use_ssd();
                } else {
                    w.proxy.use_csd();
                }
                w.last_applied_ssd = Some(w.ssd);
            }
        }

        // Mark all windows as initially managed
        for w in &mut self.windows {
            w.initial_managed = true;
        }

        let focused_out_area = self
            .get_focused_output_id()
            .and_then(|id| self.outputs.get(&id))
            .map(|o| o.usable_area)
            .or_else(|| self.outputs.values().next().map(|o| o.usable_area));

        // 4. Interactive pointer operations (Move / Resize) & Focus synchronization
        for seat in self.seats.values_mut() {
            if seat.layer_focus == LayerShellFocus::None {
                if let Some(target) = &seat.focused {
                    let target_id = target.id();
                    if seat.last_focused_window.as_ref() != Some(&target_id) {
                        seat.proxy.focus_window(target);
                        seat.last_focused_window = Some(target_id);
                    }
                } else if seat.last_focused_window.is_some() {
                    seat.proxy.clear_focus();
                    seat.last_focused_window = None;
                }
            } else {
                seat.last_focused_window = None;
            }
            match &seat.op {
                SeatOp::Move {
                    proxy,
                    start_x,
                    start_y,
                } => {
                    if let Some(w) = self.windows.iter_mut().find(|w| &w.proxy == proxy) {
                        w.x = start_x + seat.op_dx;
                        w.y = start_y + seat.op_dy;
                        w.float_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
                        w.visual_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
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
                    if let Some(w) = self.windows.iter_mut().find(|w| &w.proxy == proxy) {
                        let mut new_w = *start_width as i32;
                        let mut new_h = *start_height as i32;
                        let mut new_x = *start_x;
                        let mut new_y = *start_y;

                        if edges.contains(Edges::Right) {
                            new_w =
                                (*start_width as i32 + seat.op_dx).max(MIN_WINDOW_DIMENSION as i32);
                        } else if edges.contains(Edges::Left) {
                            new_w =
                                (*start_width as i32 - seat.op_dx).max(MIN_WINDOW_DIMENSION as i32);
                            new_x = *start_x + (*start_width as i32 - new_w);
                        }

                        if edges.contains(Edges::Bottom) {
                            new_h = (*start_height as i32 + seat.op_dy)
                                .max(MIN_WINDOW_DIMENSION as i32);
                        } else if edges.contains(Edges::Top) {
                            new_h = (*start_height as i32 - seat.op_dy)
                                .max(MIN_WINDOW_DIMENSION as i32);
                            new_y = *start_y + (*start_height as i32 - new_h);
                        }

                        w.x = new_x;
                        w.y = new_y;
                        w.width = new_w as u32;
                        w.height = new_h as u32;
                        w.float_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
                        w.visual_geo = Some(Rect::new(w.x, w.y, w.width, w.height));

                        if w.last_proposed_w != Some(w.width) || w.last_proposed_h != Some(w.height)
                        {
                            proxy.propose_dimensions(
                                w.width.min(i32::MAX as u32) as i32,
                                w.height.min(i32::MAX as u32) as i32,
                            );
                            w.last_proposed_w = Some(w.width);
                            w.last_proposed_h = Some(w.height);
                        }
                    }
                }
                SeatOp::TiledResize { start_ratio } => {
                    let Some(focused_out_area) = focused_out_area else {
                        continue;
                    };
                    let usable_w = focused_out_area.width as f32;
                    let usable_h = focused_out_area.height as f32;
                    match self.layout_config.main_location {
                        crate::layout::MainLocation::Left => {
                            let delta_ratio = (seat.op_dx as f32) / usable_w.max(1.0);
                            self.layout_config.split_ratio =
                                (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                        }
                        crate::layout::MainLocation::Right => {
                            let delta_ratio = -(seat.op_dx as f32) / usable_w.max(1.0);
                            self.layout_config.split_ratio =
                                (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                        }
                        crate::layout::MainLocation::Top => {
                            let delta_ratio = (seat.op_dy as f32) / usable_h.max(1.0);
                            self.layout_config.split_ratio =
                                (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                        }
                        crate::layout::MainLocation::Bottom => {
                            let delta_ratio = -(seat.op_dy as f32) / usable_h.max(1.0);
                            self.layout_config.split_ratio =
                                (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                        }
                    }
                }
                SeatOp::TiledStackResize { start_ratio } => {
                    let Some(focused_out_area) = focused_out_area else {
                        continue;
                    };
                    let usable_w = focused_out_area.width as f32;
                    let usable_h = focused_out_area.height as f32;
                    match self.layout_config.main_location {
                        crate::layout::MainLocation::Left | crate::layout::MainLocation::Right => {
                            let delta_ratio = (seat.op_dy as f32) / usable_h.max(1.0);
                            self.layout_config.stack_split_ratio =
                                (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                        }
                        crate::layout::MainLocation::Top | crate::layout::MainLocation::Bottom => {
                            let delta_ratio = (seat.op_dx as f32) / usable_w.max(1.0);
                            self.layout_config.stack_split_ratio =
                                (*start_ratio + delta_ratio).clamp(0.1, 0.9);
                        }
                    }
                }
                SeatOp::TiledMove { .. } => {}
                SeatOp::None => {}
            }
        }

        // 4b. End any released pointer operations in this manage sequence
        for seat in self.seats.values_mut() {
            if seat.op_release {
                if let SeatOp::Resize { proxy, .. } = &seat.op {
                    proxy.inform_resize_end();
                }
                seat.proxy.op_end();

                let target_proxy = match &seat.op {
                    SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => {
                        Some(proxy.clone())
                    }
                    _ => None,
                };
                if let Some(target) = target_proxy
                    && let Some(w) = self.windows.iter_mut().find(|w| w.proxy == target)
                {
                    let resting = Rect::new(w.x, w.y, w.width, w.height);
                    w.anim_target_geo = Some(resting);
                    w.anim_start_geo = Some(resting);
                    w.visual_geo = Some(resting);
                }

                if let SeatOp::TiledMove { start_win_id, .. } = seat.op {
                    let px = self.pointer.0;
                    let py = self.pointer.1;
                    let dropped_on = self
                        .windows
                        .iter()
                        .find(|other| {
                            !other.closed
                                && !other.floating
                                && other.id != start_win_id
                                && self.tag_state.is_view_visible(other.tags)
                                && px >= other.x
                                && px <= (other.x + other.width as i32)
                                && py >= other.y
                                && py <= (other.y + other.height as i32)
                        })
                        .map(|w| w.id);

                    if let Some(target_id) = dropped_on {
                        let i1 = self.windows.iter().position(|w| w.id == start_win_id);
                        let i2 = self.windows.iter().position(|w| w.id == target_id);
                        if let (Some(idx1), Some(idx2)) = (i1, i2) {
                            self.windows.swap(idx1, idx2);
                            tracing::info!(
                                "Tiled pointer drop: swapped window {start_win_id} with {target_id}"
                            );
                        }
                    }
                }

                if let Some(ref dev) = seat.cursor_shape_device {
                    dev.set_shape(
                        0,
                        crate::protocol::wp_cursor_shape_device_v1::Shape::Default,
                    );
                }

                seat.op = SeatOp::None;
                seat.op_release = false;
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        // Trigger animation if geometries changed
        if !is_any_pointer_op && any_geo_changed && self.anim.enabled {
            self.anim.start();
        }
        if self.anim.is_animating() {
            self.manage_dirty();
        } else {
            if self.anim.start_time.is_some() {
                self.anim.stop();
            }
            if self.tag_slide_dir.is_some() {
                self.tag_slide_dir = None;
                self.tag_anim_old_mask = TAG_NONE;
            }
        }

        // Apply any pending pointer warps during the manage sequence
        for seat in self.seats.values_mut() {
            if let Some((wx, wy)) = seat.pending_warp.take() {
                seat.proxy.pointer_warp(wx, wy);
            }
        }

        self.propose_initial_dimensions();
        self.sync_occupied_tags();
        self.broadcast_status();
        _proxy.manage_finish();
    }

    pub fn handle_render_start(&mut self, _proxy: &RiverWindowManagerV1) {
        if self.outputs.is_empty() {
            _proxy.render_finish();
            return;
        }
        let border_width = (self.border_width.min(i32::MAX as u32)) as i32;
        let (fr, fg, fb, fa) = hex_to_river_rgba(&self.border_color_focused);
        let (ur, ug, ub, ua) = hex_to_river_rgba(&self.border_color_unfocused);
        let focused_proxy = self.seats.values().find_map(|s| {
            if s.layer_focus == LayerShellFocus::None {
                s.focused.clone()
            } else {
                None
            }
        });
        let (hide_x, hide_y) = self.offscreen_hiding_position();

        let is_animating = self.anim.is_animating();
        let progress = self.anim.progress();
        let is_tag_animating = self.tag_slide_dir.is_some() && is_animating;

        let active_move_proxy = self.seats.values().find_map(|s| match &s.op {
            SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => Some(proxy.clone()),
            _ => None,
        });

        for w in &mut self.windows {
            let is_focused = focused_proxy.as_ref() == Some(&w.proxy);
            let (cr, cg, cb, ca) = if is_focused {
                (fr, fg, fb, fa)
            } else {
                (ur, ug, ub, ua)
            };
            if w.fullscreen {
                w.proxy.set_borders(Edges::empty(), 0, 0, 0, 0, 0);
            } else if w.ssd {
                w.proxy
                    .set_borders(Edges::all(), border_width, cr, cg, cb, ca);
            } else {
                w.proxy.set_borders(Edges::empty(), 0, 0, 0, 0, 0);
            }
            let usable_area = w
                .output
                .as_ref()
                .and_then(|id| self.outputs.get(id))
                .map(|o| o.usable_area)
                .or_else(|| self.outputs.values().next().map(|o| o.usable_area));
            let Some(usable_area) = usable_area else {
                continue;
            };

            let slide_offset = if let Some(dir) = self.tag_slide_dir {
                match dir {
                    crate::animation::SlideDirection::Right => usable_area.width as i32,
                    crate::animation::SlideDirection::Left => -(usable_area.width as i32),
                }
            } else {
                0
            };

            let is_in_current = self.tag_state.is_view_visible(w.tags);
            let is_in_old = is_tag_animating && (w.tags & self.tag_anim_old_mask) != 0;

            if is_in_current || is_in_old {
                let is_interactive = active_move_proxy.as_ref() == Some(&w.proxy);
                let target = Rect::new(w.x, w.y, w.width, w.height);

                let (render_geo, is_visible) = if is_interactive {
                    w.proxy.set_clip_box(0, 0, 0, 0);
                    (target, true)
                } else if is_tag_animating {
                    let is_shared = is_in_current && (w.tags & self.tag_anim_old_mask) != 0;
                    if is_shared {
                        w.proxy.set_clip_box(0, 0, 0, 0);
                        (target, true)
                    } else if is_in_old && !is_in_current {
                        // Old tag window sliding out
                        let end_x = target.x - slide_offset;
                        let cur_x = crate::animation::interpolate(target.x, end_x, progress);
                        let cur_geo = Rect::new(cur_x, target.y, target.width, target.height);
                        if let Some((cx, cy, cw, ch)) =
                            calculate_clip_box(cur_geo, usable_area, border_width)
                        {
                            w.proxy.set_clip_box(cx, cy, cw, ch);
                            (cur_geo, true)
                        } else {
                            (cur_geo, false)
                        }
                    } else {
                        // New tag window sliding in
                        let start_x = target.x + slide_offset;
                        let cur_x = crate::animation::interpolate(start_x, target.x, progress);
                        let cur_geo = Rect::new(cur_x, target.y, target.width, target.height);
                        if let Some((cx, cy, cw, ch)) =
                            calculate_clip_box(cur_geo, usable_area, border_width)
                        {
                            w.proxy.set_clip_box(cx, cy, cw, ch);
                            (cur_geo, true)
                        } else {
                            (cur_geo, false)
                        }
                    }
                } else if is_animating && w.anim_start_geo.is_some_and(|start| start != target) {
                    let start = w.anim_start_geo.unwrap_or(target);
                    let geo = interpolate_rect(start, target, progress);
                    if let Some((cx, cy, cw, ch)) =
                        calculate_clip_box(geo, usable_area, border_width)
                    {
                        w.proxy.set_clip_box(cx, cy, cw, ch);
                        (geo, true)
                    } else {
                        (geo, false)
                    }
                } else {
                    w.proxy.set_clip_box(0, 0, 0, 0);
                    w.anim_start_geo = Some(target);
                    w.anim_target_geo = Some(target);
                    (target, true)
                };

                w.visual_geo = Some(render_geo);
                if is_visible {
                    w.node.set_position(render_geo.x, render_geo.y);
                } else {
                    w.node.set_position(hide_x, hide_y);
                }
                w.initial_rendered = true;
            } else {
                w.node.set_position(hide_x, hide_y);
            }
        }

        // Enforce strict layered Z-ordering (matching river-classic where .float > .layout):
        //
        // Layer 1 (Bottom): Tiled windows.
        //   Place unfocused tiled windows first, then focused tiled window (if focused is tiled).
        let focused_proxy = self.seats.values().find_map(|s| s.focused.clone());

        for w in self
            .windows
            .iter()
            .filter(|w| !w.floating && self.tag_state.is_view_visible(w.tags))
        {
            if focused_proxy.as_ref() != Some(&w.proxy) {
                w.node.place_top();
            }
        }
        if let Some(ref focused) = focused_proxy
            && let Some(win) = self.windows.iter().find(|w| {
                &w.proxy == focused && !w.floating && self.tag_state.is_view_visible(w.tags)
            })
        {
            win.node.place_top();
        }

        // Layer 2 (Top): Floating windows.
        //   Place unfocused floating windows first, then focused floating window (if focused is floating).
        //   Floating windows are GUARANTEED to ALWAYS remain above all tiled windows!
        for w in self
            .windows
            .iter()
            .filter(|w| w.floating && self.tag_state.is_view_visible(w.tags))
        {
            if focused_proxy.as_ref() != Some(&w.proxy) {
                w.node.place_top();
            }
        }
        if let Some(ref focused) = focused_proxy
            && let Some(win) = self.windows.iter().find(|w| {
                &w.proxy == focused && w.floating && self.tag_state.is_view_visible(w.tags)
            })
        {
            win.node.place_top();
        }

        // Layer 3 (Absolute Top): Fullscreen windows
        for w in self
            .windows
            .iter()
            .filter(|w| w.fullscreen && self.tag_state.is_view_visible(w.tags))
        {
            w.node.place_top();
        }

        _proxy.render_finish();
    }

    pub fn broadcast_status(&mut self) {
        if self.status_listeners.is_empty() {
            return;
        }
        let broadcast_deadline = std::time::Instant::now() + std::time::Duration::from_millis(10);
        let json_status = self.format_json_status();
        let waybar_status = self.format_waybar_status();

        self.status_listeners.retain_mut(|(client, fmt)| {
            let now = std::time::Instant::now();
            if now >= broadcast_deadline {
                // Do not evict healthy listeners whose writes were not attempted due to deadline exhaustion
                return true;
            }
            let client_deadline =
                (now + std::time::Duration::from_millis(2)).min(broadcast_deadline);
            let text = if fmt.as_deref() == Some("waybar") {
                &waybar_status
            } else {
                &json_status
            };
            let mut msg = text.clone();
            msg.push('\n');
            let _ = client.set_nonblocking(true);
            crate::ipc::write_ipc_response(client, msg.as_bytes(), client_deadline).is_ok()
        });
    }

    pub fn format_json_status(&self) -> String {
        let focused_id = self.focused_window_id();
        let hovered_id = self.hovered_window_id();
        let focused_win = focused_id.and_then(|id| self.windows.iter().find(|w| w.id == id));

        let title = focused_win.and_then(|w| w.title.as_deref()).unwrap_or("");
        let app_id = focused_win.and_then(|w| w.app_id.as_deref()).unwrap_or("");
        let floating = focused_win.map(|w| w.floating).unwrap_or(false);

        let mut window_list: Vec<serde_json::Value> = self
            .windows
            .iter()
            .map(|w| {
                serde_json::json!({
                    "id": w.id,
                    "app_id": w.app_id.clone().unwrap_or_default(),
                    "title": w.title.clone().unwrap_or_default(),
                    "tags": w.tags,
                    "floating": w.floating,
                    "fullscreen": w.fullscreen,
                    "x": w.x,
                    "y": w.y,
                    "width": w.width,
                    "height": w.height,
                })
            })
            .collect();
        window_list.sort_by_key(|v| v["id"].as_u64().unwrap_or(0));

        let obj = serde_json::json!({
            "focused_tags": self.tag_state.focused,
            "occupied_tags": self.tag_state.occupied,
            "active_tag_numbers": self.tag_state.focused_tag_indices(),
            "occupied_tag_numbers": self.tag_state.occupied_tag_indices(),
            "focused_window": {
                "title": title,
                "app_id": app_id,
                "floating": floating,
            },
            "layout": if self.layout_config.monocle { "monocle" } else { "master-stack" },
            "main_location": format!("{:?}", self.layout_config.main_location).to_lowercase(),
            "mode": self.active_mode,
            "focused_window_id": focused_id,
            "hovered_window_id": hovered_id,
            "pointer": { "x": self.pointer.0, "y": self.pointer.1 },
            "windows": window_list,
        });

        serde_json::to_string(&obj).unwrap_or_default()
    }

    pub fn format_waybar_status(&self) -> String {
        let active = self.tag_state.focused_tag_indices();
        let tags_text = active
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        let focused_id = self.focused_window_id();
        let focused_win = focused_id.and_then(|id| self.windows.iter().find(|w| w.id == id));
        let title = focused_win.and_then(|w| w.title.as_deref()).unwrap_or("");

        let obj = serde_json::json!({
            "text": tags_text,
            "tooltip": format!("Mode: [{}], Focused: {}", self.active_mode, title),
            "class": if self.layout_config.monocle { "monocle" } else { "tiled" },
        });

        serde_json::to_string(&obj).unwrap_or_default()
    }
}

pub fn spawn_init_script() {
    reap_zombies();
    // In unit tests, avoid executing the host environment's personal init script
    if cfg!(test) {
        return;
    }
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    let init_script = config_dir.join("xrwm").join("init");

    if init_script.is_file() {
        tracing::info!("Spawning xrwm init script: {:?}", init_script);
        let _ = std::process::Command::new("bash")
            .arg("-c")
            .arg(&init_script)
            .spawn();
    }
}

/// Reap any dead child processes without blocking.
///
/// Drains all exited child processes via `waitpid(-1, WNOHANG)`, preventing
/// zombie processes from accumulating when commands are spawned via `spawn`,
/// `init`, or `reload`.
pub fn reap_zombies() {
    loop {
        match rustix::process::waitpid(None, rustix::process::WaitOptions::NOHANG) {
            Ok(Some(_status)) => {
                // Reaped an exited child; continue draining.
            }
            Ok(None) => {
                // No more exited children waiting to be reaped.
                break;
            }
            Err(rustix::io::Errno::CHILD) => {
                // No child processes exist.
                break;
            }
            Err(rustix::io::Errno::INTR) => {
                // Interrupted by signal; retry.
                continue;
            }
            Err(_) => {
                // Other errors; stop draining.
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_mode_parse() {
        assert_eq!(AttachMode::parse("top").unwrap(), AttachMode::Top);
        assert_eq!(AttachMode::parse("bottom").unwrap(), AttachMode::Bottom);
        assert_eq!(AttachMode::parse("above").unwrap(), AttachMode::Above);
        assert_eq!(AttachMode::parse("below").unwrap(), AttachMode::Below);
        assert_eq!(AttachMode::parse("after 2").unwrap(), AttachMode::After(2));
        assert_eq!(AttachMode::parse("after 0").unwrap(), AttachMode::After(0));
        assert!(AttachMode::parse("after").is_err());
        assert!(AttachMode::parse("after foo").is_err());
        assert!(AttachMode::parse("invalid").is_err());
    }

    #[test]
    fn test_attach_mode_insert_index() {
        // When empty
        assert_eq!(AttachMode::Top.calculate_insert_index(None, 0), 0);
        assert_eq!(AttachMode::Bottom.calculate_insert_index(None, 0), 0);
        assert_eq!(AttachMode::Above.calculate_insert_index(None, 0), 0);
        assert_eq!(AttachMode::Below.calculate_insert_index(None, 0), 0);
        assert_eq!(AttachMode::After(3).calculate_insert_index(None, 0), 0);

        // When 3 windows exist, focused at index 1
        let focused = Some(1);
        let len = 3;
        assert_eq!(AttachMode::Top.calculate_insert_index(focused, len), 0);
        assert_eq!(AttachMode::Bottom.calculate_insert_index(focused, len), 3);
        assert_eq!(AttachMode::Above.calculate_insert_index(focused, len), 1);
        assert_eq!(AttachMode::Below.calculate_insert_index(focused, len), 2);
        assert_eq!(AttachMode::After(0).calculate_insert_index(focused, len), 0);
        assert_eq!(AttachMode::After(1).calculate_insert_index(focused, len), 1);
        assert_eq!(AttachMode::After(2).calculate_insert_index(focused, len), 2);
        assert_eq!(AttachMode::After(5).calculate_insert_index(focused, len), 3);

        // When 3 windows exist, no focus
        assert_eq!(AttachMode::Above.calculate_insert_index(None, len), 0);
        assert_eq!(AttachMode::Below.calculate_insert_index(None, len), 3);
    }

    #[test]
    fn test_broadcast_status_preserves_healthy_subscribers() {
        let mut state = AppState::new();
        let (server1, _client1) = UnixStream::pair().unwrap();
        let (server2, _client2) = UnixStream::pair().unwrap();

        state.status_listeners.push((server1, None));
        state.status_listeners.push((server2, None));

        state.broadcast_status();
        assert_eq!(state.status_listeners.len(), 2);
    }

    #[test]
    fn test_broadcast_status_drops_broken_pipe_subscriber() {
        let mut state = AppState::new();
        let (server1, client1) = UnixStream::pair().unwrap();
        let (server2, _client2) = UnixStream::pair().unwrap();

        drop(client1); // peer disconnected

        state.status_listeners.push((server1, None));
        state.status_listeners.push((server2, None));

        state.broadcast_status();
        // Broken pipe subscriber should be evicted, while healthy one remains
        assert_eq!(state.status_listeners.len(), 1);
    }

    #[test]
    fn test_parse_hex_color_valid() {
        assert!(parse_hex_color("#61afef").is_ok());
        assert!(parse_hex_color("0x61afef").is_ok());
        assert!(parse_hex_color("61afef").is_ok());
        assert!(parse_hex_color("#61afef80").is_ok());
        assert!(parse_hex_color("0x61afef80").is_ok());
        assert!(parse_hex_color("61afef80").is_ok());

        let (r, g, b, a) = parse_hex_color("#ffffff").unwrap();
        assert_eq!(r, u32::MAX);
        assert_eq!(g, u32::MAX);
        assert_eq!(b, u32::MAX);
        assert_eq!(a, u32::MAX);

        // 8-digit hex with premultiplied alpha
        let (r, g, b, a) = parse_hex_color("#ff000080").unwrap();
        assert_eq!(a, 0x80 * (u32::MAX / 255));
        let expected_r = ((u32::MAX as u64 * a as u64) / u32::MAX as u64) as u32;
        assert_eq!(r, expected_r);
        assert_eq!(g, 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn test_parse_hex_color_invalid() {
        // Non-ASCII string (would panic with naive slicing)
        assert!(parse_hex_color("你好").is_err());
        assert_eq!(
            hex_to_river_rgba("你好"),
            (u32::MAX, u32::MAX, u32::MAX, u32::MAX)
        );

        // Invalid hex characters
        assert!(parse_hex_color("#12345z").is_err());
        assert!(parse_hex_color("0xhello!").is_err());

        // Too short
        assert!(parse_hex_color("").is_err());
        assert!(parse_hex_color("#123").is_err());
        assert!(parse_hex_color("0x").is_err());
        assert!(parse_hex_color("#12345").is_err());

        // Too long
        assert!(parse_hex_color("#1234567").is_err());
        assert!(parse_hex_color("#123456789").is_err());
    }
}
