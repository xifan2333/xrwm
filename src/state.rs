//! Central application state machine for xrwm.

use std::collections::HashMap;
use std::path::PathBuf;

use wayland_backend::client::ObjectId;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_registry;

use crate::animation::AnimationController;

use crate::layout::{LayoutConfig, Rect};
use crate::protocol::{
    river_layer_shell_output_v1::RiverLayerShellOutputV1, river_layer_shell_v1::RiverLayerShellV1,
    river_output_v1::RiverOutputV1, river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::RiverWindowV1, river_xkb_bindings_v1::RiverXkbBindingsV1,
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use crate::render::Rgba;
use crate::seat::{LayerShellFocus, PointerAction, SeatItem, SeatOp};
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::tag::TagState;

pub use crate::window::{
    BorderRole, LAYER_POLICIES, LayerPolicy, MIN_WINDOW_DIMENSION, WindowItem, WindowLayer,
    WindowVisibility,
};

pub use crate::render::{hex_to_river_rgba, parse_hex_color};
pub use crate::status::{
    StatusBroadcaster, StatusListener, StatusSubscription, SubscriberSnapshot, broadcast_status,
    format_json_status, format_pinned_status, format_scratchpad_status, format_tag_status,
    format_waybar_status, format_window_status,
};

pub use crate::rule::{WindowRule, apply_rules_to_window, glob_match};

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

#[derive(Debug)]
pub struct OutputItem {
    pub proxy: RiverOutputV1,
    pub wl_output: Option<wayland_client::protocol::wl_output::WlOutput>,
    pub name: Option<String>,
    pub ls_output: Option<RiverLayerShellOutputV1>,
    pub removed: bool,
    pub usable_area: Rect,
    pub has_custom_usable_area: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub tag_state: TagState,
    pub previous_focused_tags: TagMask,
    pub monocle: bool,
    pub floating_hidden_tags: TagMask,
    /// Active screen capture sessions (River v5) recording/sharing this output.
    pub capture_sessions: u32,
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
    pub border_color_focused: Rgba,
    pub border_color_unfocused: Rgba,
    pub border_color_urgent: Rgba,
    pub rules: Vec<WindowRule>,
    pub bindings: crate::binds::BindingManager,
    pub next_view_id: u32,
    pub attach_mode: AttachMode,
    pub cursor_warp: crate::seat::CursorWarp,
    pub focus_follows_cursor: crate::seat::FocusFollowsCursor,
    pub spawn_tagmask: TagMask,
    pub cursor_hide_timeout: u64,
    pub cursor_hide_when_typing: bool,
    pub cursor_hidden: bool,
    pub last_pointer_activity: std::time::Instant,

    pub anim: AnimationController,
    pub tag_slide_dir: Option<crate::animation::SlideDirection>,
    pub tag_anim_old_mask: TagMask,
    pub broadcaster: StatusBroadcaster,
    pub adapter: crate::adapter::CompositorAdapter,
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
            border_color_focused: Rgba::from_rgb8(0x7a, 0xa2, 0xf7),
            border_color_unfocused: Rgba::from_rgb8(0x41, 0x48, 0x68),
            border_color_urgent: Rgba::from_rgb8(0xf7, 0x76, 0x8e),
            rules: Vec::new(),
            bindings: crate::binds::BindingManager::new(),
            next_view_id: 1,
            attach_mode: AttachMode::default(),
            cursor_warp: crate::seat::CursorWarp::default(),
            focus_follows_cursor: crate::seat::FocusFollowsCursor::default(),
            spawn_tagmask: u32::MAX,
            cursor_hide_timeout: 0,
            cursor_hide_when_typing: false,
            cursor_hidden: false,
            last_pointer_activity: std::time::Instant::now(),
            anim: AnimationController::default(),
            tag_slide_dir: None,
            tag_anim_old_mask: TAG_NONE,
            broadcaster: StatusBroadcaster::new(),
            adapter: crate::adapter::CompositorAdapter::new(),
            should_exit: false,
        }
    }

    #[inline]
    pub fn manage_dirty(&self) {
        if let Some(wm) = &self.river_wm {
            wm.manage_dirty();
        }
    }

    /// Handles session_locked event from the compositor.
    pub fn handle_session_locked(&mut self) {
        if self.bindings.handle_session_locked() {
            self.manage_dirty();
        }
    }

    /// Handles session_unlocked event from the compositor.
    pub fn handle_session_unlocked(&mut self) {
        if self.bindings.handle_session_unlocked() {
            self.manage_dirty();
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
        self.adapter.register_window(item.id, item.proxy.clone());
        let focused_id = self.focused_window_id();
        let target_out = item.output.clone();
        let is_floating = item.floating;
        let is_workspace_floating = item.layer() == WindowLayer::WorkspaceFloating;
        let item_tags = item.tags;
        let item_out = item.output.clone();

        let output_indices: Vec<usize> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                (target_out.is_none() || w.output == target_out) && w.floating == is_floating
            })
            .map(|(i, _)| i)
            .collect();

        if output_indices.is_empty() {
            self.windows.push(item);
        } else {
            let focused_pos = focused_id.and_then(|id| {
                output_indices
                    .iter()
                    .position(|&idx| self.windows[idx].id == id)
            });

            let local_insert_idx = self
                .attach_mode
                .calculate_insert_index(focused_pos, output_indices.len());

            let global_insert_idx = if local_insert_idx < output_indices.len() {
                output_indices[local_insert_idx]
            } else if let Some(&last_idx) = output_indices.last() {
                last_idx + 1
            } else {
                self.windows.len()
            };

            self.windows.insert(global_insert_idx, item);
        }

        if is_workspace_floating {
            if let Some(ref out_id) = item_out {
                if let Some(out) = self.outputs.get_mut(out_id) {
                    out.floating_hidden_tags &= !item_tags;
                }
            } else {
                for out in self.outputs.values_mut() {
                    out.floating_hidden_tags &= !item_tags;
                }
            }
        }
    }

    /// Clears floating collapse mask for tags occupied by the window if it is in WorkspaceFloating layer.
    pub fn uncollapse_floating_for_window_by_proxy(&mut self, proxy: &RiverWindowV1) {
        let win_info = self
            .windows
            .iter()
            .find(|w| &w.proxy == proxy)
            .map(|w| (w.layer(), w.tags, w.output.clone()));
        if let Some((layer, tags, out_id)) = win_info
            && layer == WindowLayer::WorkspaceFloating
        {
            if let Some(ref id) = out_id {
                if let Some(out) = self.outputs.get_mut(id) {
                    out.floating_hidden_tags &= !tags;
                }
            } else {
                for out in self.outputs.values_mut() {
                    out.floating_hidden_tags &= !tags;
                }
            }
        }
    }

    /// Translates floating geometry of a window from `src_area` to `dst_area`.
    pub fn translate_float_geometry(
        geo: Rect,
        src_area: Option<Rect>,
        dst_area: Option<Rect>,
    ) -> Rect {
        WindowItem::translate_float_geometry(geo, src_area, dst_area)
    }

    /// Migrates a window's output and transforms both active and saved floating geometry.
    pub fn migrate_window_to_output(
        w: &mut WindowItem,
        dest_out_id: Option<ObjectId>,
        src_area: Option<Rect>,
        dest_area: Option<Rect>,
    ) {
        w.migrate_to_output(dest_out_id, src_area, dest_area);
    }

    /// Applies matching window rules to a window item.
    ///
    /// Distinguishes between initial allocation rules (`tags`, `output`, `dimensions`, `position`,
    /// `fullscreen`), which are only applied prior to initial management so user manual adjustments
    /// are preserved across title/app_id updates, and dynamic rules (`float`, `ssd`/`csd`), which
    /// update whenever metadata changes.
    #[inline]
    pub fn apply_rules_to_window(
        rules: &[WindowRule],
        w: &mut WindowItem,
        usable_area: Option<Rect>,
        outputs: &HashMap<ObjectId, OutputItem>,
    ) {
        crate::rule::apply_rules_to_window(rules, w, usable_area, outputs);
    }

    /// Resolves the authoritative tag state for a given output ID.
    ///
    /// Fallback chain:
    /// 1. If explicit output ID is provided and exists in `outputs`, returns its `tag_state`.
    /// 2. If single output or output not found, returns `self.tag_state` (global/focused snapshot).
    #[inline]
    pub fn tag_state_for_output(&self, output_id: Option<&ObjectId>) -> TagState {
        if self.outputs.len() <= 1 {
            self.tag_state
        } else {
            output_id
                .and_then(|id| self.outputs.get(id))
                .map(|o| o.tag_state)
                .unwrap_or(self.tag_state)
        }
    }

    /// Resolves the tag state of the currently focused output.
    #[inline]
    pub fn focused_output_tag_state(&self) -> TagState {
        let focused_id = self.get_focused_output_id();
        self.tag_state_for_output(focused_id.as_ref())
    }

    /// Resolves the floating hidden tag mask for the given output.
    #[inline]
    pub fn floating_hidden_tags_for_output(&self, output_id: Option<&ObjectId>) -> TagMask {
        if self.outputs.len() <= 1 {
            self.outputs
                .values()
                .next()
                .map(|o| o.floating_hidden_tags)
                .unwrap_or(0)
        } else {
            output_id
                .and_then(|id| self.outputs.get(id))
                .or_else(|| self.outputs.values().next())
                .map(|o| o.floating_hidden_tags)
                .unwrap_or(0)
        }
    }

    /// Determines whether floating windows on the given output's active tags are currently collapsed/hidden.
    #[inline]
    pub fn is_floating_hidden_for_output(&self, output_id: Option<&ObjectId>) -> bool {
        let hidden_tags = self.floating_hidden_tags_for_output(output_id);
        let tag_state = self.tag_state_for_output(output_id);
        (hidden_tags & tag_state.focused) != 0
    }

    /// Returns whether floating windows on the currently focused output are collapsed/hidden.
    #[inline]
    pub fn is_focused_output_floating_hidden(&self) -> bool {
        let focused_id = self.get_focused_output_id();
        self.is_floating_hidden_for_output(focused_id.as_ref())
    }

    /// Resolves window visibility against output tag state and floating collapse mask.
    #[inline]
    pub fn resolve_window_visibility(
        tag_state: TagState,
        floating_hidden_tags: TagMask,
        w: &WindowItem,
    ) -> WindowVisibility {
        let active_tags = w.tags() & tag_state.focused;
        if active_tags == 0 {
            WindowVisibility::HiddenByTags
        } else if w.layer().policy().collapsible && (active_tags & !floating_hidden_tags) == 0 {
            WindowVisibility::HiddenByCollapse
        } else {
            WindowVisibility::Visible
        }
    }

    /// Resolves the current window visibility using the application state.
    #[inline]
    pub fn window_visibility(&self, w: &WindowItem) -> WindowVisibility {
        let out_id = w.output();
        let tag_state = self.tag_state_for_output(out_id);
        let floating_hidden_tags = self.floating_hidden_tags_for_output(out_id);
        Self::resolve_window_visibility(tag_state, floating_hidden_tags, w)
    }

    /// Evaluates window visibility using the given outputs map and fallback tag state.
    #[inline]
    pub fn window_visibility_with(
        outputs: &HashMap<ObjectId, OutputItem>,
        default_tag_state: TagState,
        w: &WindowItem,
    ) -> WindowVisibility {
        let (out_tag_state, floating_hidden_tags) = if outputs.len() <= 1 {
            (
                default_tag_state,
                outputs
                    .values()
                    .next()
                    .map(|o| o.floating_hidden_tags)
                    .unwrap_or(0),
            )
        } else {
            let o = w.output.as_ref().and_then(|id| outputs.get(id));
            let floating_out = o.or_else(|| outputs.values().next());
            (
                o.map(|o| o.tag_state).unwrap_or(default_tag_state),
                floating_out.map(|o| o.floating_hidden_tags).unwrap_or(0),
            )
        };

        Self::resolve_window_visibility(out_tag_state, floating_hidden_tags, w)
    }

    /// Returns whether the window is visible according to the tag and floating state of its output.
    #[inline]
    pub fn is_window_visible(&self, w: &WindowItem) -> bool {
        self.window_visibility(w).is_visible()
    }

    /// Evaluates window visibility using the given outputs map and fallback tag state.
    #[inline]
    pub fn is_window_visible_with(
        outputs: &HashMap<ObjectId, OutputItem>,
        default_tag_state: TagState,
        w: &WindowItem,
    ) -> bool {
        Self::window_visibility_with(outputs, default_tag_state, w).is_visible()
    }

    pub fn offscreen_hiding_position(&self) -> (i32, i32) {
        let min_x = self.outputs.values().map(|o| o.x as i64).min().unwrap_or(0);
        let min_y = self.outputs.values().map(|o| o.y as i64).min().unwrap_or(0);
        let hide_x = (min_x - 100_000).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let hide_y = (min_y - 100_000).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        (hide_x, hide_y)
    }

    pub fn reconcile_focus(&mut self) {
        let focused_out = self.get_focused_output_id();

        for seat in self.seats.values_mut() {
            if seat.layer_focus != LayerShellFocus::None {
                continue;
            }

            // Check if current focused window is still alive and visible
            let current_is_valid = if let Some(ref focused) = seat.focused {
                self.windows.iter().any(|w| {
                    &w.proxy == focused
                        && !w.closed
                        && Self::is_window_visible_with(&self.outputs, self.tag_state, w)
                })
            } else {
                false
            };

            if current_is_valid {
                continue;
            }

            // Otherwise, select a deterministic replacement candidate from visible, alive windows.
            // Priority:
            // 1. Visible alive windows on the current focused output
            // 2. Visible alive windows on any output
            let candidate = self
                .windows
                .iter()
                .find(|w| {
                    !w.closed
                        && Self::is_window_visible_with(&self.outputs, self.tag_state, w)
                        && (focused_out.is_none() || w.output == focused_out)
                })
                .or_else(|| {
                    self.windows.iter().find(|w| {
                        !w.closed && Self::is_window_visible_with(&self.outputs, self.tag_state, w)
                    })
                });

            if let Some(cand) = candidate {
                seat.set_focused_window(Some(cand.proxy.clone()));
            } else {
                seat.set_focused_window(None);
            }
        }
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
                .find(|w| &w.proxy == proxy && !w.closed && self.is_window_visible(w))
                .map(|w| w.id)
        } else {
            None
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
        for (out_id, out) in &mut self.outputs {
            let mut mask = TAG_NONE;
            for w in &self.windows {
                if !w.closed && w.tags != crate::tag::TAG_ALL && w.output == Some(out_id.clone()) {
                    mask |= w.tags;
                }
            }
            out.tag_state.occupied = mask;
        }

        let mut mask = TAG_NONE;
        for w in &self.windows {
            if !w.closed && w.tags != crate::tag::TAG_ALL {
                mask |= w.tags;
            }
        }
        self.tag_state.occupied = mask;
    }

    fn propose_initial_dimensions(&mut self) {
        // Windows skipped by layout still need a proposal before River can map them.
        for w in &mut self.windows {
            w.propose_initial_dimensions();
        }
    }

    pub fn handle_manage_start(&mut self, _proxy: &RiverWindowManagerV1, qh: &QueueHandle<Self>) {
        self.reconcile_focus();
        self.bindings
            .sync_compositor(self.river_xkb.as_ref(), &mut self.seats, qh);

        // 0. Process any pending close requests inside the manage sequence
        for w in &mut self.windows {
            w.process_pending_close();
        }

        // Apply any pending fullscreen requests (synchronizing compositor state and client notifications)
        for w in &mut self.windows {
            let out_proxy = w
                .output
                .as_ref()
                .and_then(|id| self.outputs.get(id))
                .map(|o| &o.proxy)
                .or_else(|| self.outputs.values().next().map(|o| &o.proxy));
            w.apply_pending_fullscreen(out_proxy);
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
            for w in self.windows.iter().filter(|w| w.closed) {
                self.adapter.remove_window(w.id);
            }
            for c in &closed {
                c.destroy();
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
            match action {
                PointerAction::Move => {
                    if let Some(win_proxy) = seat.hovered.clone() {
                        start_move.push((id.clone(), win_proxy));
                    }
                }
                PointerAction::Resize => {
                    if let Some(win_proxy) = seat.hovered.clone() {
                        start_resize.push((id.clone(), win_proxy));
                    }
                }
                PointerAction::Command(cmd) => {
                    if let Some(win_proxy) = seat.hovered.clone() {
                        seat.set_focused_window(Some(win_proxy.clone()));
                        if let Some(win) = self.windows.iter().find(|w| w.proxy == win_proxy)
                            && let Some(ref out_id) = win.output
                        {
                            self.focused_output = Some(out_id.clone());
                        }
                    }
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
                if floating {
                    seat.start_floating_move(win_proxy, x, y);
                } else {
                    seat.start_tiled_move(win_proxy, win_id);
                }
            }
        }

        for (id, win_proxy) in start_resize {
            let win_info = self.windows.iter().find(|w| w.proxy == win_proxy).map(|w| {
                (
                    w.id,
                    w.floating,
                    Rect::new(w.x, w.y, w.effective_width(), w.effective_height()),
                )
            });
            if let (Some((win_id, floating, rect)), Some(seat)) =
                (win_info, self.seats.get_mut(&id))
            {
                if floating {
                    seat.start_floating_resize(win_proxy, rect, self.pointer);
                } else {
                    let tag_state = self.tag_state;
                    let win_output = self
                        .windows
                        .iter()
                        .find(|w| w.id == win_id)
                        .and_then(|w| w.output.clone());
                    let tiled_wins: Vec<u32> = self
                        .windows
                        .iter()
                        .filter(|w| {
                            !w.closed
                                && !w.floating
                                && tag_state.is_view_visible(w.tags)
                                && w.output == win_output
                        })
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
                    let is_stack_split = is_stack && stack_count >= 2;
                    let start_ratio = if is_stack_split {
                        self.layout_config.stack_split_ratio
                    } else {
                        self.layout_config.split_ratio
                    };
                    seat.start_tiled_resize(
                        self.layout_config.main_location,
                        is_stack_split,
                        start_ratio,
                    );
                }
            }
        }

        let focused_out_area = self
            .get_focused_output_id()
            .and_then(|id| self.outputs.get(&id))
            .map(|o| o.usable_area)
            .or_else(|| self.outputs.values().next().map(|o| o.usable_area));

        // 3. Interactive pointer operations (Move / Resize / Tiled updates) & Focus synchronization
        for seat in self.seats.values_mut() {
            seat.sync_focus();
            seat.apply_motion(&mut self.windows, &mut self.layout_config, focused_out_area);
        }

        // 3b. End any released pointer operations in this manage sequence
        let mut op_was_released = false;
        let outputs = &self.outputs;
        let tag_state = self.tag_state;
        let pointer = self.pointer;
        for seat in self.seats.values_mut() {
            if seat.commit_release(&mut self.windows, pointer, |w| {
                Self::is_window_visible_with(outputs, tag_state, w)
            }) {
                op_was_released = true;
            }
        }

        // 4. Arrange windows for each output
        if self.outputs.is_empty() {
            self.propose_initial_dimensions();
            self.sync_occupied_tags();
            self.broadcast_status();
            _proxy.manage_finish();
            return;
        }

        let plan = crate::layout::compute_arrange_plan(self);
        crate::layout::execute_arrange_plan(self, &plan, op_was_released);

        self.propose_initial_dimensions();
        self.sync_occupied_tags();
        self.broadcast_status();
        _proxy.manage_finish();
    }

    pub fn handle_render_start(&mut self, proxy: &RiverWindowManagerV1) {
        if !self.outputs.is_empty() {
            let plan = crate::render::compute_frame_plan(self);
            crate::render::execute_frame_plan(&mut self.windows, &plan);
        }
        proxy.render_finish();
    }

    #[inline]
    pub fn broadcast_status(&mut self) {
        crate::status::broadcast_status(self);
    }

    #[inline]
    pub fn format_json_status(&self) -> String {
        crate::status::format_json_status(self)
    }

    #[inline]
    pub fn format_waybar_status(&self) -> String {
        crate::status::format_waybar_status(self)
    }

    #[inline]
    pub fn format_tag_status(&self, tag: u8) -> String {
        crate::status::format_tag_status(self, tag)
    }

    #[inline]
    pub fn format_window_status(&self) -> String {
        crate::status::format_window_status(self)
    }

    #[inline]
    pub fn format_scratchpad_status(&self) -> String {
        crate::status::format_scratchpad_status(self)
    }

    #[inline]
    pub fn format_pinned_status(&self) -> String {
        crate::status::format_pinned_status(self)
    }

    /// Resolves the current snapshot representation for a subscriber to detect state changes.
    /// Computes the current deduplication snapshot for the given status subscription type.
    pub fn current_subscriber_snapshot(&self, sub: &StatusSubscription) -> SubscriberSnapshot {
        match sub {
            StatusSubscription::Tag(tag) => {
                let mask = 1u32
                    .checked_shl((tag.saturating_sub(1)) as u32)
                    .unwrap_or(0);
                let focused_tag_state = self.focused_output_tag_state();
                SubscriberSnapshot::Tag {
                    focused: (focused_tag_state.focused & mask) != 0,
                    occupied: (focused_tag_state.occupied & mask) != 0,
                }
            }
            StatusSubscription::Window => {
                let focused_id = self.focused_window_id();
                let focused_win =
                    focused_id.and_then(|id| self.windows.iter().find(|w| w.id == id));
                SubscriberSnapshot::Window {
                    focused_id,
                    title: focused_win.and_then(|w| w.title.clone()),
                    app_id: focused_win.and_then(|w| w.app_id.clone()),
                    floating: focused_win.map(|w| w.floating).unwrap_or(false),
                }
            }
            StatusSubscription::WaybarLegacy => {
                let active = self.tag_state.focused_tag_indices();
                let tags_text = active
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let focused_id = self.focused_window_id();
                let focused_win =
                    focused_id.and_then(|id| self.windows.iter().find(|w| w.id == id));
                let capture_sessions = crate::status::total_capture_sessions(self);
                let is_floating_hidden = self.is_focused_output_floating_hidden();
                let hidden_floating_count = if is_floating_hidden {
                    let focused_out = self.get_focused_output_id();
                    self.windows
                        .iter()
                        .filter(|w| {
                            !w.is_closed()
                                && (focused_out.is_none()
                                    || self.outputs.len() <= 1
                                    || w.output() == focused_out.as_ref())
                                && self.window_visibility(w) == WindowVisibility::HiddenByCollapse
                        })
                        .count()
                } else {
                    0
                };
                SubscriberSnapshot::WaybarLegacy {
                    active_tags: tags_text,
                    focused_title: focused_win.and_then(|w| w.title.clone()),
                    active_mode: self.bindings.active_mode.clone(),
                    monocle: self.layout_config.monocle,
                    floating_hidden: is_floating_hidden,
                    hidden_floating_count,
                    capturing: capture_sessions > 0,
                    capture_sessions,
                }
            }
            StatusSubscription::FullJson => {
                let s = self.format_json_status();
                use std::hash::{DefaultHasher, Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                s.hash(&mut hasher);
                SubscriberSnapshot::FullJson {
                    payload_hash: hasher.finish(),
                }
            }
            StatusSubscription::Scratchpad => {
                let is_focused = self.tag_state.is_scratchpad_focused();
                let is_occupied = self.tag_state.is_scratchpad_occupied();
                let window_count = self
                    .windows
                    .iter()
                    .filter(|w| {
                        !w.closed
                            && (w.tags & crate::tag::TAG_SCRATCHPAD) != 0
                            && w.tags != crate::tag::TAG_ALL
                    })
                    .count();
                SubscriberSnapshot::Scratchpad {
                    focused: is_focused,
                    occupied: is_occupied,
                    window_count,
                }
            }
            StatusSubscription::Pinned => {
                let pinned_windows: Vec<&crate::state::WindowItem> = self
                    .windows
                    .iter()
                    .filter(|w| !w.closed && w.tags == crate::tag::TAG_ALL)
                    .collect();
                let window_count = pinned_windows.len();
                let active = window_count > 0;
                let tooltip = if window_count == 0 {
                    "No pinned windows".to_string()
                } else {
                    let titles: Vec<&str> = pinned_windows
                        .iter()
                        .map(|w| {
                            w.app_id
                                .as_deref()
                                .or(w.title.as_deref())
                                .unwrap_or("window")
                        })
                        .collect();
                    format!("Pinned ({window_count}): {}", titles.join(", "))
                };
                SubscriberSnapshot::Pinned {
                    active,
                    window_count,
                    tooltip,
                }
            }
        }
    }
}

/// Spawns an init script located at `init_script` directly as a file parameter
/// to bash (or via its shebang), avoiding `bash -c` string re-tokenization.
pub fn spawn_init_script_at(init_script: &std::path::Path) -> std::io::Result<std::process::Child> {
    reap_zombies();
    std::process::Command::new("bash")
        .arg(init_script)
        .spawn()
        .or_else(|_| std::process::Command::new(init_script).spawn())
}

/// Spawns the xrwm init script from a configuration directory (`<config_dir>/xrwm/init`).
///
/// Returns `Some(Ok(child))` if spawned, `Some(Err(err))` if spawning failed,
/// or `None` if the init script file does not exist.
pub fn spawn_init_script_from_config(
    config_dir: &std::path::Path,
) -> Option<std::io::Result<std::process::Child>> {
    let init_script = config_dir.join("xrwm").join("init");
    if init_script.is_file() {
        tracing::info!(
            init_script = %init_script.display(),
            "Spawning xrwm init script"
        );
        let res = spawn_init_script_at(&init_script);
        if let Err(ref e) = res {
            tracing::error!(
                init_script = %init_script.display(),
                error = %e,
                "Failed to spawn xrwm init script"
            );
        }
        Some(res)
    } else {
        None
    }
}

/// Spawns the xrwm init script from `$XDG_CONFIG_HOME/xrwm/init` (or `~/.config/xrwm/init`).
///
/// Reaps zombie processes before spawning, logs errors on failure, and avoids
/// `bash -c` string re-tokenization so directories with spaces are supported.
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
    let _ = spawn_init_script_from_config(&config_dir);
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

pub static PROCESS_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
