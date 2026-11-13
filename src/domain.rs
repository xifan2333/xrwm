//! Pure in-memory Single Source of Truth (SSOT) domain model and algebraic effect commands.
//!
//! This module defines the system-agnostic core of xrwm: free of Wayland proxy handles,
//! raw Unix domain sockets, or event loop dependencies. All mutations to the domain core
//! produce pure algebraic `DomainEffect` values executed by the compositor adapter.

use std::collections::HashMap;

use wayland_backend::client::ObjectId;

use crate::animation::{AnimationController, SlideDirection};
use crate::binds::{PendingKeyBinding, PendingPointerBinding};
use crate::layout::{LayoutConfig, Rect};
use crate::protocol::river_window_v1::Edges;
use crate::rule::{WindowRule, glob_match};
use crate::seat::{
    CursorWarp, FocusFollowsCursor, LayerShellFocus, PointerAction, SeatItem, SeatOp,
};
use crate::state::{AppState, AttachMode, OutputItem};
use crate::tag::{TAG_ALL, TAG_NONE, TagMask, TagState};
use crate::window::{LAYER_POLICIES, LayerPolicy, WindowItem, WindowLayer, WindowVisibility};

/// Algebraic side-effect command emitted by the pure domain model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainEffect {
    /// Window dimension proposal sent to client surface.
    ProposeDimensions {
        window_id: u32,
        width: u32,
        height: u32,
    },
    /// Inform client window surface it has entered fullscreen on the specified output.
    InformFullscreen {
        window_id: u32,
        output_id: Option<ObjectId>,
    },
    /// Inform client window surface it has exited fullscreen.
    InformNotFullscreen { window_id: u32 },
    /// Inform client window surface interactive resize has started.
    InformResizeStart { window_id: u32 },
    /// Inform client window surface interactive resize has ended.
    InformResizeEnd { window_id: u32 },
    /// Send close request to client surface.
    CloseWindow { window_id: u32 },
    /// Destroy window protocol proxy after close.
    DestroyWindow { window_id: u32 },
    /// Configure whether window edges are considered tiled.
    SetTiled { window_id: u32, tiled: bool },
    /// Configure window decoration mode (true = SSD, false = CSD).
    SetDecorationMode { window_id: u32, ssd: bool },
    /// Synchronize seat focus to window or clear focus.
    SetSeatFocus {
        seat_id: ObjectId,
        window_id: Option<u32>,
    },
    /// Start interactive pointer grab on seat.
    SeatOpStart { seat_id: ObjectId },
    /// End interactive pointer grab on seat.
    SeatOpEnd { seat_id: ObjectId },
    /// Signal manage sequence completion to river-wm.
    ManageFinish,
    /// Differential status broadcast to IPC subscribers.
    BroadcastStatus,
    /// Execute an external action command (e.g. spawn process, reload).
    ExecuteAction { tokens: Vec<String> },
}

/// Pure in-memory representation of a managed application surface without Wayland handles.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowModel {
    pub id: u32,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub tags: TagMask,
    pub has_parent: bool,
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
    pub content_width: Option<u32>,
    pub content_height: Option<u32>,
    pub rule_dimensions: Option<(u32, u32)>,
    pub visual_geo: Option<Rect>,
    pub anim_start_geo: Option<Rect>,
    pub anim_target_geo: Option<Rect>,
    pub last_proposed_w: Option<u32>,
    pub last_proposed_h: Option<u32>,
    pub capture_sessions: u32,
    pub closed: bool,
    pub initial_managed: bool,
    pub initial_rendered: bool,
}

impl WindowModel {
    /// Creates a new pure window model with default initial state.
    pub fn new(id: u32, tags: TagMask, output: Option<ObjectId>) -> Self {
        Self {
            id,
            app_id: None,
            title: None,
            tags,
            has_parent: false,
            floating: false,
            fullscreen: false,
            pending_close: false,
            pending_fullscreen_change: false,
            float_geo: None,
            ssd: true,
            last_applied_ssd: None,
            output,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            content_width: None,
            content_height: None,
            rule_dimensions: None,
            visual_geo: None,
            anim_start_geo: None,
            anim_target_geo: None,
            last_proposed_w: None,
            last_proposed_h: None,
            capture_sessions: 0,
            closed: false,
            initial_managed: false,
            initial_rendered: false,
        }
    }

    /// Converts an existing WindowItem into a pure WindowModel.
    pub fn from_window_item(w: &WindowItem) -> Self {
        Self {
            id: w.id,
            app_id: w.app_id.clone(),
            title: w.title.clone(),
            tags: w.tags,
            has_parent: w.has_parent,
            floating: w.floating,
            fullscreen: w.fullscreen,
            pending_close: w.pending_close,
            pending_fullscreen_change: w.pending_fullscreen_change,
            float_geo: w.float_geo,
            ssd: w.ssd,
            last_applied_ssd: w.last_applied_ssd,
            output: w.output.clone(),
            x: w.x,
            y: w.y,
            width: w.width,
            height: w.height,
            content_width: w.content_width,
            content_height: w.content_height,
            rule_dimensions: w.rule_dimensions,
            visual_geo: w.visual_geo,
            anim_start_geo: w.anim_start_geo,
            anim_target_geo: w.anim_target_geo,
            last_proposed_w: w.last_proposed_w,
            last_proposed_h: w.last_proposed_h,
            capture_sessions: w.capture_sessions(),
            closed: w.closed,
            initial_managed: w.initial_managed,
            initial_rendered: w.initial_rendered,
        }
    }

    /// Synchronizes pure state updates back into a WindowItem.
    pub fn sync_to_window_item(&self, w: &mut WindowItem) {
        w.app_id = self.app_id.clone();
        w.title = self.title.clone();
        w.tags = self.tags;
        w.has_parent = self.has_parent;
        w.floating = self.floating;
        w.fullscreen = self.fullscreen;
        w.pending_close = self.pending_close;
        w.pending_fullscreen_change = self.pending_fullscreen_change;
        w.float_geo = self.float_geo;
        w.ssd = self.ssd;
        w.last_applied_ssd = self.last_applied_ssd;
        w.output = self.output.clone();
        w.x = self.x;
        w.y = self.y;
        w.width = self.width;
        w.height = self.height;
        w.content_width = self.content_width;
        w.content_height = self.content_height;
        w.rule_dimensions = self.rule_dimensions;
        w.visual_geo = self.visual_geo;
        w.anim_start_geo = self.anim_start_geo;
        w.anim_target_geo = self.anim_target_geo;
        w.last_proposed_w = self.last_proposed_w;
        w.last_proposed_h = self.last_proposed_h;
        w.closed = self.closed;
        w.initial_managed = self.initial_managed;
        w.initial_rendered = self.initial_rendered;
    }

    #[inline]
    pub fn layer(&self) -> WindowLayer {
        if self.fullscreen {
            WindowLayer::Fullscreen
        } else if self.tags == TAG_ALL {
            WindowLayer::Pinned
        } else if (self.tags & (1 << 31)) != 0 {
            WindowLayer::Scratchpad
        } else if self.floating {
            WindowLayer::WorkspaceFloating
        } else {
            WindowLayer::Tiled
        }
    }

    #[inline]
    pub fn layer_policy(&self) -> LayerPolicy {
        LAYER_POLICIES[self.layer() as usize]
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    #[inline]
    pub fn effective_width(&self) -> u32 {
        self.content_width.unwrap_or(self.width)
    }

    #[inline]
    pub fn effective_height(&self) -> u32 {
        self.content_height.unwrap_or(self.height)
    }

    /// Proposes target dimensions, emitting a `DomainEffect` only if dimensions changed.
    pub fn propose_dimensions(&mut self, w: u32, h: u32) -> Option<DomainEffect> {
        let same = self.last_proposed_w == Some(w) && self.last_proposed_h == Some(h);
        if same && (w != 0 || h != 0) {
            return None;
        }
        self.last_proposed_w = Some(w);
        self.last_proposed_h = Some(h);
        Some(DomainEffect::ProposeDimensions {
            window_id: self.id,
            width: w,
            height: h,
        })
    }
}

/// Pure in-memory representation of an output display area.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputModel {
    pub id: ObjectId,
    pub name: Option<String>,
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
    pub capture_sessions: u32,
}

impl OutputModel {
    /// Creates an OutputModel from an OutputItem.
    pub fn from_output_item(id: &ObjectId, o: &OutputItem) -> Self {
        Self {
            id: id.clone(),
            name: o.name.clone(),
            removed: o.removed,
            usable_area: o.usable_area,
            has_custom_usable_area: o.has_custom_usable_area,
            x: o.x,
            y: o.y,
            width: o.width,
            height: o.height,
            tag_state: o.tag_state,
            previous_focused_tags: o.previous_focused_tags,
            monocle: o.monocle,
            floating_hidden_tags: o.floating_hidden_tags,
            capture_sessions: o.capture_sessions,
        }
    }

    /// Synchronizes pure state updates back into an OutputItem.
    pub fn sync_to_output_item(&self, o: &mut OutputItem) {
        o.name = self.name.clone();
        o.removed = self.removed;
        o.usable_area = self.usable_area;
        o.has_custom_usable_area = self.has_custom_usable_area;
        o.x = self.x;
        o.y = self.y;
        o.width = self.width;
        o.height = self.height;
        o.tag_state = self.tag_state;
        o.previous_focused_tags = self.previous_focused_tags;
        o.monocle = self.monocle;
        o.floating_hidden_tags = self.floating_hidden_tags;
        o.capture_sessions = self.capture_sessions;
    }
}

/// Pure representation of an interactive pointer gesture in domain space.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SeatOpModel {
    #[default]
    None,
    Move {
        window_id: u32,
        start_x: i32,
        start_y: i32,
    },
    Resize {
        window_id: u32,
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
        window_id: u32,
        start_win_id: u32,
    },
}

impl SeatOpModel {
    /// Converts a SeatOp into a pure SeatOpModel using the provided window list.
    pub fn from_seat_op(op: &SeatOp, windows: &[WindowItem]) -> Self {
        match op {
            SeatOp::None => Self::None,
            SeatOp::Move {
                proxy,
                start_x,
                start_y,
            } => {
                let win_id = windows
                    .iter()
                    .find(|w| &w.proxy == proxy)
                    .map(|w| w.id)
                    .unwrap_or(0);
                Self::Move {
                    window_id: win_id,
                    start_x: *start_x,
                    start_y: *start_y,
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
                let win_id = windows
                    .iter()
                    .find(|w| &w.proxy == proxy)
                    .map(|w| w.id)
                    .unwrap_or(0);
                Self::Resize {
                    window_id: win_id,
                    start_x: *start_x,
                    start_y: *start_y,
                    start_width: *start_width,
                    start_height: *start_height,
                    edges: *edges,
                }
            }
            SeatOp::TiledResize { start_ratio } => Self::TiledResize {
                start_ratio: *start_ratio,
            },
            SeatOp::TiledStackResize { start_ratio } => Self::TiledStackResize {
                start_ratio: *start_ratio,
            },
            SeatOp::TiledMove {
                proxy,
                start_win_id,
            } => {
                let win_id = windows
                    .iter()
                    .find(|w| &w.proxy == proxy)
                    .map(|w| w.id)
                    .unwrap_or(0);
                Self::TiledMove {
                    window_id: win_id,
                    start_win_id: *start_win_id,
                }
            }
        }
    }
}

/// Pure in-memory representation of an input seat without Wayland handles.
#[derive(Debug, Clone, PartialEq)]
pub struct SeatModel {
    pub id: ObjectId,
    pub focused_window: Option<u32>,
    pub hovered_window: Option<u32>,
    pub op: SeatOpModel,
    pub pending_action: PointerAction,
    pub layer_focus: LayerShellFocus,
}

impl SeatModel {
    /// Creates a SeatModel from an existing SeatItem and window list.
    pub fn from_seat_item(id: &ObjectId, s: &SeatItem, windows: &[WindowItem]) -> Self {
        let focused_id = s
            .focused
            .as_ref()
            .and_then(|proxy| windows.iter().find(|w| &w.proxy == proxy).map(|w| w.id));
        let hovered_id = s
            .hovered
            .as_ref()
            .and_then(|proxy| windows.iter().find(|w| &w.proxy == proxy).map(|w| w.id));
        Self {
            id: id.clone(),
            focused_window: focused_id,
            hovered_window: hovered_id,
            op: SeatOpModel::from_seat_op(&s.op, windows),
            pending_action: s.pending_action.clone(),
            layer_focus: s.layer_focus,
        }
    }
}

/// Pure representation of modal and pending bindings configuration in domain space.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingModel {
    pub configured_key_bindings: Vec<PendingKeyBinding>,
    pub pending_key_bindings: Vec<PendingKeyBinding>,
    pub configured_pointer_bindings: Vec<PendingPointerBinding>,
    pub pending_pointer_bindings: Vec<PendingPointerBinding>,
    pub active_mode: String,
    pub modes: Vec<String>,
    pub mode_dirty: bool,
    pub session_locked: bool,
    pub pre_lock_mode: Option<String>,
}

impl Default for BindingModel {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingModel {
    pub fn new() -> Self {
        Self {
            configured_key_bindings: Vec::new(),
            pending_key_bindings: Vec::new(),
            configured_pointer_bindings: Vec::new(),
            pending_pointer_bindings: Vec::new(),
            active_mode: "normal".to_string(),
            modes: vec!["normal".to_string(), "locked".to_string()],
            mode_dirty: false,
            session_locked: false,
            pre_lock_mode: None,
        }
    }

    pub fn from_binding_manager(bm: &crate::binds::BindingManager) -> Self {
        Self {
            configured_key_bindings: bm.configured_key_bindings.clone(),
            pending_key_bindings: bm.pending_key_bindings.clone(),
            configured_pointer_bindings: bm.configured_pointer_bindings.clone(),
            pending_pointer_bindings: bm.pending_pointer_bindings.clone(),
            active_mode: bm.active_mode.clone(),
            modes: bm.modes.clone(),
            mode_dirty: bm.mode_dirty,
            session_locked: bm.session_locked,
            pre_lock_mode: bm.pre_lock_mode.clone(),
        }
    }

    pub fn sync_to_binding_manager(&self, bm: &mut crate::binds::BindingManager) {
        bm.configured_key_bindings = self.configured_key_bindings.clone();
        bm.pending_key_bindings = self.pending_key_bindings.clone();
        bm.configured_pointer_bindings = self.configured_pointer_bindings.clone();
        bm.pending_pointer_bindings = self.pending_pointer_bindings.clone();
        bm.active_mode = self.active_mode.clone();
        bm.modes = self.modes.clone();
        bm.mode_dirty = self.mode_dirty;
        bm.session_locked = self.session_locked;
        bm.pre_lock_mode = self.pre_lock_mode.clone();
    }
}

/// Pure in-memory Single Source of Truth (SSOT) domain state machine.
#[derive(Debug, Clone)]
pub struct DomainModel {
    pub windows: Vec<WindowModel>,
    pub outputs: HashMap<ObjectId, OutputModel>,
    pub focused_output: Option<ObjectId>,
    pub seats: HashMap<ObjectId, SeatModel>,
    pub tag_state: TagState,
    pub previous_focused_tags: TagMask,
    pub layout_config: LayoutConfig,
    pub border_width: u32,
    pub border_color_focused: String,
    pub border_color_unfocused: String,
    pub border_color_urgent: String,
    pub rules: Vec<WindowRule>,
    pub bindings: BindingModel,
    pub next_view_id: u32,
    pub attach_mode: AttachMode,
    pub cursor_warp: CursorWarp,
    pub focus_follows_cursor: FocusFollowsCursor,
    pub spawn_tagmask: TagMask,
    pub pointer: (i32, i32),
    pub anim: AnimationController,
    pub tag_slide_dir: Option<SlideDirection>,
    pub tag_anim_old_mask: TagMask,
    pub should_exit: bool,
}

impl Default for DomainModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainModel {
    /// Creates a fresh, pure in-memory domain model.
    pub fn new() -> Self {
        Self {
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
            bindings: BindingModel::new(),
            next_view_id: 1,
            attach_mode: AttachMode::default(),
            cursor_warp: CursorWarp::default(),
            focus_follows_cursor: FocusFollowsCursor::default(),
            spawn_tagmask: u32::MAX,
            pointer: (0, 0),
            anim: AnimationController::default(),
            tag_slide_dir: None,
            tag_anim_old_mask: TAG_NONE,
            should_exit: false,
        }
    }

    /// Converts an AppState snapshot into a pure DomainModel.
    pub fn from_app_state(state: &AppState) -> Self {
        let windows: Vec<WindowModel> = state
            .windows
            .iter()
            .map(WindowModel::from_window_item)
            .collect();
        let outputs: HashMap<ObjectId, OutputModel> = state
            .outputs
            .iter()
            .map(|(id, o)| (id.clone(), OutputModel::from_output_item(id, o)))
            .collect();
        let seats: HashMap<ObjectId, SeatModel> = state
            .seats
            .iter()
            .map(|(id, s)| (id.clone(), SeatModel::from_seat_item(id, s, &state.windows)))
            .collect();

        Self {
            windows,
            outputs,
            focused_output: state.focused_output.clone(),
            seats,
            tag_state: state.tag_state,
            previous_focused_tags: state.previous_focused_tags,
            layout_config: state.layout_config.clone(),
            border_width: state.border_width,
            border_color_focused: state.border_color_focused.clone(),
            border_color_unfocused: state.border_color_unfocused.clone(),
            border_color_urgent: state.border_color_urgent.clone(),
            rules: state.rules.clone(),
            bindings: BindingModel::from_binding_manager(&state.bindings),
            next_view_id: state.next_view_id,
            attach_mode: state.attach_mode,
            cursor_warp: state.cursor_warp,
            focus_follows_cursor: state.focus_follows_cursor,
            spawn_tagmask: state.spawn_tagmask,
            pointer: state.pointer,
            anim: state.anim.clone(),
            tag_slide_dir: state.tag_slide_dir,
            tag_anim_old_mask: state.tag_anim_old_mask,
            should_exit: state.should_exit,
        }
    }

    /// Synchronizes pure state updates back into an AppState.
    pub fn sync_to_app_state(&self, state: &mut AppState) {
        state.next_view_id = self.next_view_id;
        state.tag_state = self.tag_state;
        state.previous_focused_tags = self.previous_focused_tags;
        state.layout_config = self.layout_config.clone();
        state.border_width = self.border_width;
        state.border_color_focused = self.border_color_focused.clone();
        state.border_color_unfocused = self.border_color_unfocused.clone();
        state.border_color_urgent = self.border_color_urgent.clone();
        state.rules = self.rules.clone();
        self.bindings.sync_to_binding_manager(&mut state.bindings);
        state.attach_mode = self.attach_mode;
        state.cursor_warp = self.cursor_warp;
        state.focus_follows_cursor = self.focus_follows_cursor;
        state.spawn_tagmask = self.spawn_tagmask;
        state.pointer = self.pointer;
        state.anim = self.anim.clone();
        state.tag_slide_dir = self.tag_slide_dir;
        state.tag_anim_old_mask = self.tag_anim_old_mask;
        state.should_exit = self.should_exit;
        state.focused_output = self.focused_output.clone();

        for (id, out_model) in &self.outputs {
            if let Some(o) = state.outputs.get_mut(id) {
                out_model.sync_to_output_item(o);
            }
        }

        // Rebuild windows in domain model order, retaining existing WindowItems by ID
        let mut new_windows = Vec::with_capacity(self.windows.len());
        for win_model in &self.windows {
            if let Some(pos) = state.windows.iter().position(|w| w.id == win_model.id) {
                let mut w = state.windows.remove(pos);
                win_model.sync_to_window_item(&mut w);
                new_windows.push(w);
            }
        }
        state.windows = new_windows;

        // Sync seats
        for (seat_id, seat_model) in &self.seats {
            if let Some(seat_item) = state.seats.get_mut(seat_id) {
                let target_proxy = seat_model.focused_window.and_then(|id| {
                    state
                        .windows
                        .iter()
                        .find(|w| w.id == id)
                        .map(|w| w.proxy.clone())
                });
                seat_item.set_focused_window(target_proxy);
            }
        }
    }

    /// Allocates a new monotonically increasing window view ID.
    pub fn allocate_view_id(&mut self) -> u32 {
        let id = self.next_view_id;
        self.next_view_id = self.next_view_id.wrapping_add(1);
        id
    }

    /// Applies matching rules to a window model.
    pub fn apply_rules_to_window_model(&self, window: &mut WindowModel) {
        for rule in &self.rules {
            let app_id_matches = match (&rule.app_id, &window.app_id) {
                (Some(rule_pattern), Some(window_app_id)) => {
                    glob_match(rule_pattern, window_app_id)
                }
                (None, _) => true,
                _ => false,
            };
            let title_matches = match (&rule.title, &window.title) {
                (Some(rule_pattern), Some(window_title)) => glob_match(rule_pattern, window_title),
                (None, _) => true,
                _ => false,
            };

            if app_id_matches && title_matches {
                if let Some(float) = rule.float {
                    window.floating = float;
                }
                if let Some(ssd) = rule.ssd {
                    window.ssd = ssd;
                }
                if let Some(tags) = rule.tags {
                    window.tags = tags;
                }
                if let Some(fullscreen) = rule.fullscreen {
                    window.fullscreen = fullscreen;
                }
                if let Some(dims) = rule.dimensions {
                    window.rule_dimensions = Some(dims);
                    window.width = dims.0;
                    window.height = dims.1;
                }
                if let Some((x, y)) = rule.position {
                    window.x = x;
                    window.y = y;
                }
                if let Some(ref out_str) = rule.output {
                    let trimmed = out_str.trim();
                    let matched_out = self.outputs.iter().find(|(id, o)| {
                        o.name
                            .as_deref()
                            .is_some_and(|n| n.eq_ignore_ascii_case(trimmed))
                            || id.to_string() == trimmed
                            || id.protocol_id().to_string() == trimmed
                    });
                    if let Some((id, _)) = matched_out {
                        window.output = Some(id.clone());
                    }
                }
            }
        }
    }

    /// Attaches a window into the model according to the configured attach mode.
    pub fn attach_window(&mut self, mut window: WindowModel) -> Vec<DomainEffect> {
        let mut effects = Vec::new();

        self.apply_rules_to_window_model(&mut window);

        if window.ssd {
            effects.push(DomainEffect::SetDecorationMode {
                window_id: window.id,
                ssd: true,
            });
        }

        // Uncollapse floating for this window's tags if workspace floating
        if window.layer() == WindowLayer::WorkspaceFloating {
            if let Some(out_id) = &window.output {
                if let Some(out) = self.outputs.get_mut(out_id) {
                    out.floating_hidden_tags &= !window.tags;
                }
            } else {
                for out in self.outputs.values_mut() {
                    out.floating_hidden_tags &= !window.tags;
                }
            }
        }

        let focused_id = self.focused_window_id();
        let target_out = window.output.clone();
        let is_floating = window.floating;

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
            self.windows.push(window);
        } else {
            let focused_pos = focused_id.and_then(|id| {
                output_indices
                    .iter()
                    .position(|&idx| self.windows.get(idx).is_some_and(|w| w.id == id))
            });

            let insert_pos = self
                .attach_mode
                .calculate_insert_index(focused_pos, output_indices.len());

            if insert_pos >= output_indices.len() {
                let last_idx = *output_indices.last().unwrap_or(&0);
                self.windows
                    .insert((last_idx + 1).min(self.windows.len()), window);
            } else {
                let target_idx = output_indices[insert_pos];
                self.windows.insert(target_idx, window);
            }
        }

        effects
    }

    /// Removes a window by ID and clears any seat references holding it.
    pub fn remove_window(&mut self, id: u32) -> Vec<DomainEffect> {
        let mut effects = Vec::new();

        for seat in self.seats.values_mut() {
            let hit = match &seat.op {
                SeatOpModel::Move { window_id, .. }
                | SeatOpModel::Resize { window_id, .. }
                | SeatOpModel::TiledMove { window_id, .. } => *window_id == id,
                SeatOpModel::None
                | SeatOpModel::TiledResize { .. }
                | SeatOpModel::TiledStackResize { .. } => false,
            };
            if hit {
                if matches!(&seat.op, SeatOpModel::Resize { .. }) {
                    effects.push(DomainEffect::InformResizeEnd { window_id: id });
                }
                effects.push(DomainEffect::SeatOpEnd {
                    seat_id: seat.id.clone(),
                });
                seat.op = SeatOpModel::None;
            }
            if seat.focused_window == Some(id) {
                seat.focused_window = None;
                effects.push(DomainEffect::SetSeatFocus {
                    seat_id: seat.id.clone(),
                    window_id: None,
                });
            }
            if seat.hovered_window == Some(id) {
                seat.hovered_window = None;
            }
        }

        effects.push(DomainEffect::DestroyWindow { window_id: id });
        self.windows.retain(|w| w.id != id);
        effects
    }

    /// Reconciles focus for all active seats.
    pub fn reconcile_focus(&mut self) -> Vec<DomainEffect> {
        let focused_out_id = self.get_focused_output_id();

        // 1. Windows alive and visible on the focused output
        let focused_out_wins: Vec<u32> = self
            .windows
            .iter()
            .filter(|w| {
                !w.closed
                    && (focused_out_id.is_none() || w.output == focused_out_id)
                    && self.is_window_visible(w)
            })
            .map(|w| w.id)
            .collect();

        // 2. Windows alive and visible anywhere
        let any_visible_wins: Vec<u32> = self
            .windows
            .iter()
            .filter(|w| !w.closed && self.is_window_visible(w))
            .map(|w| w.id)
            .collect();

        let fallback_id = focused_out_wins
            .first()
            .copied()
            .or_else(|| any_visible_wins.first().copied());

        let mut effects = Vec::new();
        for seat in self.seats.values_mut() {
            if seat.layer_focus != LayerShellFocus::None {
                continue;
            }

            let current_valid = seat
                .focused_window
                .is_some_and(|target_id| any_visible_wins.contains(&target_id));

            if !current_valid {
                seat.focused_window = fallback_id;
                effects.push(DomainEffect::SetSeatFocus {
                    seat_id: seat.id.clone(),
                    window_id: fallback_id,
                });
            }
        }

        effects
    }

    /// Returns the currently focused window ID across active seats.
    pub fn focused_window_id(&self) -> Option<u32> {
        self.seats.values().find_map(|s| s.focused_window)
    }

    /// Returns the currently hovered window ID across active seats.
    pub fn hovered_window_id(&self) -> Option<u32> {
        self.seats.values().find_map(|s| s.hovered_window)
    }

    /// Returns the currently focused output ID.
    pub fn get_focused_output_id(&self) -> Option<ObjectId> {
        self.focused_output
            .clone()
            .or_else(|| self.outputs.keys().next().cloned())
    }

    /// Resolves the effective tag state for a specific output.
    pub fn tag_state_for_output(&self, output_id: Option<&ObjectId>) -> TagState {
        if self.outputs.len() <= 1 {
            return self.tag_state;
        }
        if let Some(out) = output_id.and_then(|id| self.outputs.get(id)) {
            out.tag_state
        } else {
            self.tag_state
        }
    }

    /// Resolves the visibility status of a window model against output tag state and floating collapse.
    pub fn resolve_window_visibility(&self, w: &WindowModel) -> WindowVisibility {
        let tag_state = self.tag_state_for_output(w.output.as_ref());
        let floating_hidden_tags = w
            .output
            .as_ref()
            .and_then(|id| self.outputs.get(id))
            .map(|o| o.floating_hidden_tags)
            .unwrap_or(TAG_NONE);

        let active_tags = w.tags & tag_state.focused;
        let is_tag_visible = tag_state.is_view_visible(w.tags);
        let is_floating_collapsed = w.layer_policy().collapsible
            && active_tags != 0
            && (active_tags & !floating_hidden_tags) == 0;

        if !is_tag_visible {
            WindowVisibility::HiddenByTags
        } else if is_floating_collapsed {
            WindowVisibility::HiddenByCollapse
        } else {
            WindowVisibility::Visible
        }
    }

    /// Returns true if a window model is currently visible on screen.
    #[inline]
    pub fn is_window_visible(&self, w: &WindowModel) -> bool {
        self.resolve_window_visibility(w) == WindowVisibility::Visible
    }

    /// Synchronizes occupied tags across outputs based on active windows.
    pub fn sync_occupied_tags(&mut self) {
        let mut global_occupied = TAG_NONE;
        for w in &self.windows {
            if !w.closed && w.tags != TAG_ALL {
                global_occupied |= w.tags;
            }
        }
        self.tag_state.occupied = global_occupied;

        for (out_id, out) in &mut self.outputs {
            let mut occupied = TAG_NONE;
            for w in &self.windows {
                if !w.closed && w.tags != TAG_ALL && w.output.as_ref() == Some(out_id) {
                    occupied |= w.tags;
                }
            }
            out.tag_state.occupied = occupied;
        }
    }
}
