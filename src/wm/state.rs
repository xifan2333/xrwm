//! Central application state machine for xrwm.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use wayland_backend::client::ObjectId;
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
};
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::tag::TagState;
use crate::wm::binds::{ActiveKeyBinding, PendingKeyBinding};
use crate::wm::seat::{PointerAction, SeatItem, SeatOp};

pub fn hex_to_river_rgba(hex_str: &str) -> (u32, u32, u32, u32) {
    let h = hex_str
        .trim_start_matches("0x")
        .trim_start_matches('#')
        .trim();
    if h.len() < 6 {
        return (u32::MAX, u32::MAX, u32::MAX, u32::MAX);
    }
    let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(255) * (u32::MAX / 255);
    let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(255) * (u32::MAX / 255);
    let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(255) * (u32::MAX / 255);
    let a = u32::MAX;
    (r, g, b, a)
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

#[derive(Debug, Clone)]
pub struct WindowRule {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub float: Option<bool>,
    pub ssd: Option<bool>,
    pub tags: Option<TagMask>,
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug)]
pub struct WindowItem {
    pub id: u32,
    pub proxy: RiverWindowV1,
    pub node: RiverNodeV1,
    pub new: bool,
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
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    // Animation & visual geometry tracking
    pub visual_geo: Option<Rect>,
    pub anim_start_geo: Option<Rect>,
    pub anim_target_geo: Option<Rect>,
    pub last_proposed_w: u32,
    pub last_proposed_h: u32,
}

#[derive(Debug)]
pub struct OutputItem {
    pub proxy: RiverOutputV1,
    pub ls_output: Option<RiverLayerShellOutputV1>,
    pub removed: bool,
    pub usable_area: Rect,
}

pub struct AppState {
    pub river_wm: Option<RiverWindowManagerV1>,
    pub river_xkb: Option<RiverXkbBindingsV1>,
    pub river_layer: Option<RiverLayerShellV1>,
    pub pointer: (i32, i32),
    pub windows: Vec<WindowItem>,
    pub outputs: HashMap<ObjectId, OutputItem>,
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
    pub active_mode: String,
    pub modes: Vec<String>,
    pub mode_dirty: bool,
    pub next_view_id: u32,

    pub anim: AnimationController,
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
            pointer: (0, 0),
            windows: Vec::new(),
            outputs: HashMap::new(),
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
            active_mode: "normal".to_string(),
            modes: vec!["normal".to_string(), "locked".to_string()],
            mode_dirty: false,
            next_view_id: 1,
            anim: AnimationController::default(),
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

    pub fn apply_rules_to_window(
        rules: &[WindowRule],
        w: &mut WindowItem,
        usable_area: Option<Rect>,
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
                if let Some((width, height)) = r.dimensions {
                    w.width = width;
                    w.height = height;
                    if let Some(usable) = usable_area {
                        let cx = usable.x + ((usable.width as i32 - width as i32) / 2).max(0);
                        let cy = usable.y + ((usable.height as i32 - height as i32) / 2).max(0);
                        w.x = cx;
                        w.y = cy;
                        w.float_geo = Some(Rect::new(cx, cy, width, height));
                    }
                }
            }
        }
    }

    pub fn focused_window_id(&self) -> Option<u32> {
        self.seats
            .values()
            .find_map(|s| s.focused.as_ref())
            .and_then(|proxy| self.windows.iter().find(|w| &w.proxy == proxy))
            .map(|w| w.id)
            .or_else(|| self.windows.first().map(|w| w.id))
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
            self.mode_dirty = false;
        }

        // 0. Process any pending close requests inside the manage sequence
        for w in &mut self.windows {
            if w.pending_close {
                w.proxy.close();
                w.pending_close = false;
            }
        }

        // Apply any pending fullscreen requests
        let default_output = self.outputs.values().next().map(|o| o.proxy.clone());
        for w in &mut self.windows {
            if w.pending_fullscreen_change {
                if w.fullscreen {
                    if let Some(ref out) = default_output {
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
                    SeatOp::None => None,
                    SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => {
                        Some(proxy.clone())
                    }
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
                    seat.focused = None;
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
        let mut toggle_floating: Vec<RiverWindowV1> = Vec::new();

        for (id, seat) in self.seats.iter_mut() {
            if let Some(win_proxy) = seat.interacted.take() {
                seat.focused = Some(win_proxy);
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
                PointerAction::ToggleFloating => toggle_floating.push(win_proxy),
                PointerAction::None => {}
            }
        }

        let default_area = Rect::new(0, 30, 1280, 770);
        let layout_engine = MasterStackLayout;

        let (out_id, usable_area) = self
            .outputs
            .iter()
            .next()
            .map(|(id, o)| (id.clone(), o.usable_area))
            .unwrap_or((ObjectId::null(), default_area));

        for win_proxy in toggle_floating {
            if let Some(w) = self.windows.iter_mut().find(|w| w.proxy == win_proxy) {
                w.floating = !w.floating;
                if w.floating {
                    if let Some(saved) = w.float_geo {
                        w.x = saved.x;
                        w.y = saved.y;
                        w.width = saved.width;
                        w.height = saved.height;
                    } else {
                        w.float_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
                    }
                } else {
                    w.float_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
                }
                tracing::debug!(
                    "op: toggle floating on {:?} -> {}",
                    w.proxy.id(),
                    w.floating
                );
            }
        }

        for (id, win_proxy) in start_move {
            let geo = self
                .windows
                .iter()
                .find(|w| w.proxy == win_proxy && w.floating)
                .map(|w| (w.x, w.y));
            if let (Some((x, y)), Some(seat)) = (geo, self.seats.get_mut(&id)) {
                seat.proxy.op_start_pointer();
                seat.op = SeatOp::Move {
                    proxy: win_proxy.clone(),
                    start_x: x,
                    start_y: y,
                };
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        for (id, win_proxy) in start_resize {
            let geo = self
                .windows
                .iter()
                .find(|w| w.proxy == win_proxy && w.floating)
                .map(|w| (w.x, w.y, w.width, w.height));
            if let (Some((x, y, w, h)), Some(seat)) = (geo, self.seats.get_mut(&id)) {
                seat.proxy.op_start_pointer();
                win_proxy.inform_resize_start();
                seat.op = SeatOp::Resize {
                    proxy: win_proxy.clone(),
                    start_x: x,
                    start_y: y,
                    start_width: w,
                    start_height: h,
                    edges: Edges::Bottom.union(Edges::Right),
                };
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        // 3. Arrange windows for each output
        let tag_state = self.tag_state;
        let mut tiled_indices: Vec<usize> = Vec::new();
        for (i, w) in self.windows.iter().enumerate() {
            if !w.floating && !w.fullscreen && tag_state.is_view_visible(w.tags) {
                tiled_indices.push(i);
            }
        }

        let rects = layout_engine.arrange(usable_area, tiled_indices.len(), &self.layout_config);
        let mut any_geo_changed = false;

        for (slot, &idx) in tiled_indices.iter().enumerate() {
            if let Some(rect) = rects.get(slot) {
                let w = &mut self.windows[idx];
                let target = *rect;
                if w.anim_target_geo != Some(target) {
                    w.anim_start_geo = w.visual_geo.or(w.anim_target_geo).or(Some(target));
                    w.anim_target_geo = Some(target);
                    any_geo_changed = true;
                }
                w.x = rect.x;
                w.y = rect.y;
                w.width = rect.width;
                w.height = rect.height;

                w.proxy
                    .propose_dimensions(rect.width as i32, rect.height as i32);
                w.last_proposed_w = rect.width;
                w.last_proposed_h = rect.height;
                w.proxy.set_tiled(Edges::all());
            }
        }

        // Floating windows
        for w in self
            .windows
            .iter_mut()
            .filter(|w| w.floating && !w.fullscreen)
        {
            let target = Rect::new(w.x, w.y, w.width, w.height);
            if w.anim_target_geo != Some(target) {
                w.anim_start_geo = w.visual_geo.or(w.anim_target_geo).or(Some(target));
                w.anim_target_geo = Some(target);
                any_geo_changed = true;
            }
            w.proxy.propose_dimensions(w.width as i32, w.height as i32);
            w.last_proposed_w = w.width;
            w.last_proposed_h = w.height;
            w.proxy.set_tiled(Edges::empty());
        }

        // Apply borders (SSD)
        let (fr, fg, fb, fa) = hex_to_river_rgba(&self.border_color_focused);
        let (ur, ug, ub, ua) = hex_to_river_rgba(&self.border_color_unfocused);
        let focused_proxy = self.seats.values().find_map(|s| s.focused.clone());

        for w in &self.windows {
            let is_focused = focused_proxy.as_ref() == Some(&w.proxy);
            let (cr, cg, cb, ca) = if is_focused {
                (fr, fg, fb, fa)
            } else {
                (ur, ug, ub, ua)
            };
            if w.fullscreen {
                w.proxy.set_borders(Edges::empty(), 0, 0, 0, 0, 0);
            } else if w.ssd {
                w.proxy.use_ssd();
                w.proxy
                    .set_borders(Edges::all(), self.border_width as i32, cr, cg, cb, ca);
            } else {
                w.proxy.use_csd();
                w.proxy.set_borders(Edges::empty(), 0, 0, 0, 0, 0);
            }
        }

        // 4. Interactive pointer operations (Move / Resize)
        for seat in self.seats.values_mut() {
            if let Some(target) = &seat.focused {
                seat.proxy.focus_window(target);
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
                    ..
                } => {
                    if let Some(w) = self.windows.iter_mut().find(|w| &w.proxy == proxy) {
                        let new_w = (*start_width as i32 + seat.op_dx).max(100) as u32;
                        let new_h = (*start_height as i32 + seat.op_dy).max(100) as u32;
                        w.x = *start_x;
                        w.y = *start_y;
                        w.width = new_w;
                        w.height = new_h;
                        w.float_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
                        w.visual_geo = Some(Rect::new(w.x, w.y, w.width, w.height));
                        proxy.propose_dimensions(new_w as i32, new_h as i32);
                    }
                }
                SeatOp::None => {}
            }
        }

        // Trigger animation if geometries changed
        if any_geo_changed && self.anim.enabled {
            self.anim.start();
        }
        if self.anim.is_animating() {
            self.manage_dirty();
        }

        self.sync_occupied_tags();
        self.broadcast_status();
        let _ = out_id;
        _proxy.manage_finish();
    }

    pub fn handle_render_start(&mut self, _proxy: &RiverWindowManagerV1) {
        let border_width = self.border_width as i32;
        let usable_area = self
            .outputs
            .values()
            .next()
            .map(|o| o.usable_area)
            .unwrap_or_else(|| Rect::new(0, 30, 1280, 770));

        let is_animating = self.anim.is_animating();
        let progress = self.anim.progress();

        let active_move_proxy = self.seats.values().find_map(|s| match &s.op {
            SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => Some(proxy.clone()),
            _ => None,
        });

        for w in &mut self.windows {
            if self.tag_state.is_view_visible(w.tags) {
                let is_interactive = active_move_proxy.as_ref() == Some(&w.proxy);
                let target = Rect::new(w.x, w.y, w.width, w.height);

                let render_geo = if is_animating && !is_interactive {
                    let start = w.anim_start_geo.unwrap_or(target);
                    let geo = interpolate_rect(start, target, progress);
                    let (cx, cy, cw, ch) = calculate_clip_box(geo, usable_area, border_width);
                    w.proxy.set_clip_box(cx, cy, cw, ch);
                    geo
                } else {
                    w.proxy.set_clip_box(0, 0, 0, 0);
                    target
                };

                w.visual_geo = Some(render_geo);
                w.node.set_position(render_geo.x, render_geo.y);
                w.new = false;
            } else {
                w.node.set_position(-10000, -10000);
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

        for seat in self.seats.values_mut() {
            if seat.op_release {
                if let SeatOp::Resize { proxy, .. } = &seat.op {
                    proxy.inform_resize_end();
                }
                seat.proxy.op_end();
                seat.op = SeatOp::None;
                seat.op_release = false;
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }
        _proxy.render_finish();
    }

    pub fn broadcast_status(&mut self) {
        if self.status_listeners.is_empty() {
            return;
        }
        let json_status = self.format_json_status();
        let waybar_status = self.format_waybar_status();

        self.status_listeners.retain_mut(|(client, fmt)| {
            let text = if fmt.as_deref() == Some("waybar") {
                &waybar_status
            } else {
                &json_status
            };
            let mut msg = text.clone();
            msg.push('\n');
            client.write_all(msg.as_bytes()).is_ok()
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
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    let init_script = config_dir.join("xrwm").join("init");

    if init_script.is_file() {
        tracing::info!("Spawning xrwm init script: {:?}", init_script);
        let home = std::env::var("HOME").unwrap_or_default();
        let current_path = std::env::var("PATH").unwrap_or_default();
        let path = format!("{home}/.local/bin:{current_path}");
        let _ = std::process::Command::new("bash")
            .arg("-c")
            .arg(&init_script)
            .env("PATH", path)
            .spawn();
    }
}
