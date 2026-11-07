//! Window state tracking, invariants, and geometry lifecycle.

use crate::layout::Rect;
use crate::nav::Direction;
use crate::protocol::{
    river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1,
    river_window_v1::{Edges, RiverWindowV1},
};
use crate::tag::TagMask;
use wayland_backend::client::ObjectId;

pub const MIN_WINDOW_DIMENSION: u32 = 100;

/// Mutually exclusive classification of a managed window according to its Z-priority and visual role.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WindowLayer {
    /// L1: Normal workspace tiled window participating in master-stack or monocle layout.
    Tiled = 0,
    /// L2: Normal workspace floating window, subject to per-tag floating collapse.
    WorkspaceFloating = 1,
    /// L3: Scratchpad drawer overlay window (Tag 32, but not TAG_ALL).
    Scratchpad = 2,
    /// L4: Pinned global overlay window (`TAG_ALL`, e.g. pip-camera, wayhud).
    Pinned = 3,
    /// L5: Fullscreen exclusive window covering the output usable area.
    Fullscreen = 4,
}

/// Static behavioral policies associated with each window layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerPolicy {
    /// Whether windows in this layer are arranged by the dynamic tiling engine.
    pub tiled_layout: bool,
    /// Whether windows in this layer are collapsible by `toggle-floating-views`.
    pub collapsible: bool,
    /// Whether windows in this layer mark non-all workspace tags as occupied.
    pub occupies_tag: bool,
    /// Whether windows in this layer render standard server-side decoration (SSD) borders.
    pub has_border: bool,
}

/// Static policy table indexed by `WindowLayer as usize`.
pub const LAYER_POLICIES: [LayerPolicy; 5] = [
    // Tiled
    LayerPolicy {
        tiled_layout: true,
        collapsible: false,
        occupies_tag: true,
        has_border: true,
    },
    // WorkspaceFloating
    LayerPolicy {
        tiled_layout: false,
        collapsible: true,
        occupies_tag: true,
        has_border: true,
    },
    // Scratchpad
    LayerPolicy {
        tiled_layout: false,
        collapsible: false,
        occupies_tag: true,
        has_border: true,
    },
    // Pinned
    LayerPolicy {
        tiled_layout: false,
        collapsible: false,
        occupies_tag: false,
        has_border: true,
    },
    // Fullscreen
    LayerPolicy {
        tiled_layout: false,
        collapsible: false,
        occupies_tag: true,
        has_border: false,
    },
];

impl WindowLayer {
    /// Returns the static policy configuration for this layer.
    #[inline]
    pub const fn policy(self) -> LayerPolicy {
        LAYER_POLICIES[self as usize]
    }
}

/// Visual border role determining the border color and plan for a window.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BorderRole {
    /// Window does not render borders (fullscreen or CSD).
    None = 0,
    /// Window is actively being captured or recorded (urgent border color highlight).
    Captured = 1,
    /// Window is focused by a seat.
    Focused = 2,
    /// Window is unfocused.
    Unfocused = 3,
}

/// Core managed application window tracking Wayland proxy, layout state, and geometry invariants.
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
    /// True when the compositor reported a parent window (dialog, file picker, popup menu).
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
    // Animation & visual geometry tracking
    pub visual_geo: Option<Rect>,
    pub anim_start_geo: Option<Rect>,
    pub anim_target_geo: Option<Rect>,
    // None means no proposal has been sent; zero lets the client choose its size.
    pub last_proposed_w: Option<u32>,
    pub last_proposed_h: Option<u32>,
    /// Active screen capture sessions (River v5) recording/sharing this window.
    pub capture_sessions: u32,
}

/// Resolved window rule configuration properties.
#[derive(Debug, Default, Clone)]
pub struct RuleConfig {
    pub float: Option<bool>,
    pub ssd: Option<bool>,
    pub tags: Option<TagMask>,
    pub fullscreen: Option<bool>,
    pub output: Option<ObjectId>,
    pub dimensions: Option<(u32, u32)>,
    pub position: Option<(i32, i32)>,
}

impl WindowItem {
    /// Creates a newly tracked window.
    pub fn new(
        id: u32,
        proxy: RiverWindowV1,
        node: RiverNodeV1,
        tags: TagMask,
        output: Option<ObjectId>,
    ) -> Self {
        Self {
            id,
            proxy,
            node,
            initial_managed: false,
            initial_rendered: false,
            closed: false,
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
        }
    }

    // --- Read-only inspection accessors ---

    #[inline]
    pub fn id(&self) -> u32 {
        self.id
    }

    #[inline]
    pub fn proxy(&self) -> &RiverWindowV1 {
        &self.proxy
    }

    #[inline]
    pub fn node(&self) -> &RiverNodeV1 {
        &self.node
    }

    #[inline]
    pub fn is_initial_managed(&self) -> bool {
        self.initial_managed
    }

    #[inline]
    pub fn is_initial_rendered(&self) -> bool {
        self.initial_rendered
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    #[inline]
    pub fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }

    #[inline]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[inline]
    pub fn tags(&self) -> TagMask {
        self.tags
    }

    #[inline]
    pub fn has_parent(&self) -> bool {
        self.has_parent
    }

    #[inline]
    pub fn is_floating(&self) -> bool {
        self.floating
    }

    #[inline]
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Resolves the current mutually exclusive WindowLayer according to invariants.
    #[inline]
    pub fn layer(&self) -> WindowLayer {
        if self.fullscreen {
            WindowLayer::Fullscreen
        } else if self.tags == crate::tag::TAG_ALL {
            WindowLayer::Pinned
        } else if (self.tags & crate::tag::TAG_SCRATCHPAD) != 0 {
            WindowLayer::Scratchpad
        } else if self.floating {
            WindowLayer::WorkspaceFloating
        } else {
            WindowLayer::Tiled
        }
    }

    /// Resolves the border role for this window given its seat focus status.
    #[inline]
    pub fn border_role(&self, is_focused: bool) -> BorderRole {
        if !self.layer().policy().has_border || !self.ssd() {
            BorderRole::None
        } else if self.capture_sessions > 0 {
            BorderRole::Captured
        } else if is_focused {
            BorderRole::Focused
        } else {
            BorderRole::Unfocused
        }
    }

    #[inline]
    pub fn pending_close(&self) -> bool {
        self.pending_close
    }

    #[inline]
    pub fn pending_fullscreen_change(&self) -> bool {
        self.pending_fullscreen_change
    }

    #[inline]
    pub fn float_geo(&self) -> Option<Rect> {
        self.float_geo
    }

    #[inline]
    pub fn ssd(&self) -> bool {
        self.ssd
    }

    #[inline]
    pub fn last_applied_ssd(&self) -> Option<bool> {
        self.last_applied_ssd
    }

    #[inline]
    pub fn output(&self) -> Option<&ObjectId> {
        self.output.as_ref()
    }

    #[inline]
    pub fn x(&self) -> i32 {
        self.x
    }

    #[inline]
    pub fn y(&self) -> i32 {
        self.y
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    pub fn content_width(&self) -> Option<u32> {
        self.content_width
    }

    #[inline]
    pub fn content_height(&self) -> Option<u32> {
        self.content_height
    }

    #[inline]
    pub fn rule_dimensions(&self) -> Option<(u32, u32)> {
        self.rule_dimensions
    }

    #[inline]
    pub fn visual_geo(&self) -> Option<Rect> {
        self.visual_geo
    }

    #[inline]
    pub fn anim_start_geo(&self) -> Option<Rect> {
        self.anim_start_geo
    }

    #[inline]
    pub fn anim_target_geo(&self) -> Option<Rect> {
        self.anim_target_geo
    }

    #[inline]
    pub fn last_proposed_w(&self) -> Option<u32> {
        self.last_proposed_w
    }

    #[inline]
    pub fn last_proposed_h(&self) -> Option<u32> {
        self.last_proposed_h
    }

    /// Returns the number of active screen capture sessions recording/sharing this window.
    #[inline]
    pub fn capture_sessions(&self) -> u32 {
        self.capture_sessions
    }

    /// Sets the number of active screen capture sessions recording/sharing this window.
    pub fn set_capture_sessions(&mut self, count: u32) {
        self.capture_sessions = count;
    }

    #[inline]
    pub fn effective_width(&self) -> u32 {
        self.content_width.unwrap_or(self.width)
    }

    #[inline]
    pub fn effective_height(&self) -> u32 {
        self.content_height.unwrap_or(self.height)
    }

    // --- State mutation & domain transitions ---

    /// Sets window app_id.
    pub fn set_app_id(&mut self, app_id: Option<String>) {
        self.app_id = app_id;
    }

    /// Sets window title.
    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    /// Sets parent relationship reported by River.
    pub fn set_has_parent(&mut self, has_parent: bool) {
        self.has_parent = has_parent;
    }

    /// Marks the window as closed.
    pub fn mark_closed(&mut self) {
        self.closed = true;
    }

    /// Marks window as initially managed.
    pub fn mark_initial_managed(&mut self) {
        self.initial_managed = true;
    }

    /// Marks window as initially rendered.
    pub fn mark_initial_rendered(&mut self) {
        self.initial_rendered = true;
    }

    /// Sets tags bitmask directly.
    pub fn set_tags(&mut self, mask: TagMask) {
        self.tags = mask;
    }

    /// Toggles specific tag bits on the window.
    pub fn toggle_tags(&mut self, mask: TagMask) {
        self.tags ^= mask;
    }

    /// Requests window closure (processed during the next manage sequence).
    pub fn request_close(&mut self) {
        self.pending_close = true;
    }

    /// Processes pending close request inside the manage sequence.
    pub fn process_pending_close(&mut self) {
        if self.pending_close {
            self.proxy.close();
            self.pending_close = false;
        }
    }

    /// Toggles floating state, restoring previous floating geometry when returning to floating
    /// or centering on output usable area.
    pub fn toggle_floating(&mut self, usable_area: Option<Rect>) -> bool {
        self.floating = !self.floating;
        if self.floating {
            if let Some(saved) = self.float_geo {
                self.x = saved.x;
                self.y = saved.y;
                self.width = saved.width;
                self.height = saved.height;
            } else {
                if let Some(usable) = usable_area
                    && self.width > 0
                    && self.height > 0
                {
                    let cx = (usable.x as i64
                        + ((usable.width as i64 - self.width as i64) / 2).max(0))
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    let cy = (usable.y as i64
                        + ((usable.height as i64 - self.height as i64) / 2).max(0))
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    self.x = cx;
                    self.y = cy;
                }
                self.float_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
            }
        } else {
            self.float_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
        }
        self.floating
    }

    /// Snaps window to screen half and marks it as floating.
    pub fn snap(&mut self, dir: Direction, usable: Rect) {
        self.floating = true;

        let (new_x, new_y, new_w, new_h) = match dir {
            Direction::Left => (usable.x, usable.y, usable.width / 2, usable.height),
            Direction::Right => (
                usable.x + (usable.width as i32 / 2),
                usable.y,
                usable.width - (usable.width / 2),
                usable.height,
            ),
            Direction::Up => (usable.x, usable.y, usable.width, usable.height / 2),
            Direction::Down => (
                usable.x,
                usable.y + (usable.height as i32 / 2),
                usable.width,
                usable.height - (usable.height / 2),
            ),
            _ => (usable.x, usable.y, usable.width, usable.height),
        };

        self.x = new_x;
        self.y = new_y;
        self.width = new_w.max(MIN_WINDOW_DIMENSION);
        self.height = new_h.max(MIN_WINDOW_DIMENSION);
        self.float_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
        self.visual_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
    }

    /// Toggles fullscreen state.
    pub fn toggle_fullscreen(&mut self) -> bool {
        self.fullscreen = !self.fullscreen;
        self.pending_fullscreen_change = true;
        self.fullscreen
    }

    /// Applies pending fullscreen requests during the manage sequence.
    pub fn apply_pending_fullscreen(&mut self, out_proxy: Option<&RiverOutputV1>) {
        if self.pending_fullscreen_change {
            if self.fullscreen {
                if let Some(out) = out_proxy {
                    self.proxy.fullscreen(out);
                }
                self.proxy.inform_fullscreen();
            } else {
                self.proxy.exit_fullscreen();
                self.proxy.inform_not_fullscreen();
                // Invalidate proposal cache so manage sequence submits proposal for restored layout/floating geometry
                self.last_proposed_w = None;
                self.last_proposed_h = None;
            }
            self.pending_fullscreen_change = false;
        }
    }

    /// Exits fullscreen unconditionally when output is removed.
    pub fn exit_fullscreen_on_output_removed(&mut self) {
        self.fullscreen = false;
        self.pending_fullscreen_change = false;
        self.proxy.inform_not_fullscreen();
        self.last_proposed_w = None;
        self.last_proposed_h = None;
    }

    /// Synchronizes proposal cache with River compositor.
    pub fn sync_proposed_dimensions(&mut self) {
        if self.last_proposed_w != Some(self.width) || self.last_proposed_h != Some(self.height) {
            self.proxy.propose_dimensions(
                self.width.min(i32::MAX as u32) as i32,
                self.height.min(i32::MAX as u32) as i32,
            );
            self.last_proposed_w = Some(self.width);
            self.last_proposed_h = Some(self.height);
        }
    }

    /// Proposes initial dimensions if none has been sent yet.
    pub fn propose_initial_dimensions(&mut self) {
        if !self.fullscreen && (self.last_proposed_w.is_none() || self.last_proposed_h.is_none()) {
            self.sync_proposed_dimensions();
        }
    }

    /// Applies client Dimensions event while preserving floating geometry invariants.
    pub fn apply_client_dimensions(
        &mut self,
        width: u32,
        height: u32,
        is_resizing: bool,
        has_dimensions_rule: bool,
        has_position_rule: bool,
        usable_area: Option<Rect>,
    ) {
        self.content_width = Some(width);
        self.content_height = Some(height);

        // Strictly align with river-classic:
        // Floating geometry with an explicit dimensions rule is invariant and must never be
        // overwritten by intermediate or unconfigured client commits.
        if self.floating && !is_resizing && !has_dimensions_rule {
            self.width = width;
            self.height = height;
            self.last_proposed_w = Some(width);
            self.last_proposed_h = Some(height);

            if !has_position_rule
                && (self.float_geo.is_none() || !self.initial_managed)
                && let Some(usable) = usable_area
            {
                let cx = (usable.x as i64 + ((usable.width as i64 - width as i64) / 2).max(0))
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                let cy = (usable.y as i64 + ((usable.height as i64 - height as i64) / 2).max(0))
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                self.x = cx;
                self.y = cy;
            }

            self.float_geo = Some(Rect::new(self.x, self.y, width, height));
        }
    }

    /// Updates active floating geometry during interactive pointer move or resize or keyboard action.
    pub fn update_floating_geometry(&mut self, rect: Rect) {
        self.x = rect.x;
        self.y = rect.y;
        self.width = rect.width.max(MIN_WINDOW_DIMENSION);
        self.height = rect.height.max(MIN_WINDOW_DIMENSION);
        self.float_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
        self.visual_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
        self.sync_proposed_dimensions();
    }

    /// Concludes an interactive pointer move or resize, locking resting geometry.
    pub fn finish_interaction(&mut self) {
        let resting = Rect::new(self.x, self.y, self.width, self.height);
        self.anim_target_geo = Some(resting);
        self.anim_start_geo = Some(resting);
        self.visual_geo = Some(resting);
    }

    /// Sets window coordinates directly (used for spatial positioning and tests).
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        if self.floating {
            self.float_geo = Some(Rect::new(x, y, self.width, self.height));
            self.visual_geo = Some(Rect::new(x, y, self.width, self.height));
        }
    }

    /// Moves floating window by delta pixels in given direction.
    pub fn move_by(&mut self, dir: Direction, delta: i32) {
        if !self.floating {
            return;
        }
        match dir {
            Direction::Left => self.x -= delta,
            Direction::Right => self.x += delta,
            Direction::Up => self.y -= delta,
            Direction::Down => self.y += delta,
            _ => {}
        }
        self.float_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
        self.visual_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
    }

    /// Resizes floating window dimensions by delta.
    pub fn resize_by(&mut self, horizontal: bool, delta: i32) {
        if !self.floating {
            return;
        }
        if horizontal {
            self.width = (self.width as i32 + delta).max(MIN_WINDOW_DIMENSION as i32) as u32;
        } else {
            self.height = (self.height as i32 + delta).max(MIN_WINDOW_DIMENSION as i32) as u32;
        }
        self.float_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
        self.visual_geo = Some(Rect::new(self.x, self.y, self.width, self.height));
    }

    /// Arranges window in tiled layout slot, returning whether geometry changed.
    pub fn arrange_tiled(&mut self, rect: Rect) -> bool {
        let mut geo_changed = false;
        if !self.initial_managed {
            let start_w = (rect.width * 7 / 10).max(10);
            let start_h = (rect.height * 7 / 10).max(10);
            let start_x = rect.x + (rect.width as i32 - start_w as i32) / 2;
            let start_y = rect.y + (rect.height as i32 - start_h as i32) / 2;
            self.anim_start_geo = Some(Rect::new(start_x, start_y, start_w, start_h));
            self.anim_target_geo = Some(rect);
            geo_changed = true;
        } else if self.anim_target_geo != Some(rect) {
            self.anim_start_geo = self.visual_geo.or(self.anim_target_geo).or(Some(rect));
            self.anim_target_geo = Some(rect);
            geo_changed = true;
        }

        self.x = rect.x;
        self.y = rect.y;
        self.width = rect.width;
        self.height = rect.height;

        self.sync_proposed_dimensions();
        self.proxy.set_tiled(Edges::all());
        geo_changed
    }

    /// Arranges floating window during manage sequence, returning whether geometry changed.
    pub fn arrange_floating(&mut self, is_visible: bool, is_any_pointer_op: bool) -> bool {
        let mut geo_changed = false;
        let target = Rect::new(self.x, self.y, self.width, self.height);

        if !is_any_pointer_op {
            if !self.initial_managed {
                if is_visible {
                    let start_w = ((target.width as u64 * 7 / 10) as u32).max(10);
                    let start_h = ((target.height as u64 * 7 / 10) as u32).max(10);
                    let start_x = (target.x as i64 + (target.width as i64 - start_w as i64) / 2)
                        .clamp(i32::MIN as i64, i32::MAX as i64)
                        as i32;
                    let start_y = (target.y as i64 + (target.height as i64 - start_h as i64) / 2)
                        .clamp(i32::MIN as i64, i32::MAX as i64)
                        as i32;
                    self.anim_start_geo = Some(Rect::new(start_x, start_y, start_w, start_h));
                    self.anim_target_geo = Some(target);
                    geo_changed = true;
                } else {
                    self.anim_start_geo = Some(target);
                    self.anim_target_geo = Some(target);
                    self.visual_geo = Some(target);
                }
            } else if is_visible && self.anim_target_geo != Some(target) {
                self.anim_start_geo = self.visual_geo.or(self.anim_target_geo).or(Some(target));
                self.anim_target_geo = Some(target);
                geo_changed = true;
            } else if !is_visible && self.anim_target_geo != Some(target) {
                self.anim_start_geo = Some(target);
                self.anim_target_geo = Some(target);
                self.visual_geo = Some(target);
            }
        }

        self.sync_proposed_dimensions();
        self.proxy.set_tiled(Edges::empty());
        geo_changed
    }

    /// Synchronizes decoration mode (SSD / CSD) with River compositor.
    pub fn apply_ssd(&mut self) {
        if self.last_applied_ssd != Some(self.ssd) {
            if self.ssd {
                self.proxy.use_ssd();
            } else {
                self.proxy.use_csd();
            }
            self.last_applied_ssd = Some(self.ssd);
        }
    }

    /// Records the visual geometry rendered during the current frame.
    pub fn record_render_frame(&mut self, render_geo: Rect, is_visible: bool) {
        self.visual_geo = Some(render_geo);
        if is_visible {
            self.initial_rendered = true;
        }
    }

    /// Resets animation target and start geometries to static resting geometry.
    pub fn lock_static_geometry(&mut self, target: Rect) {
        self.anim_start_geo = Some(target);
        self.anim_target_geo = Some(target);
    }

    /// Resets visual geometry to anim_target_geo when animation is disabled or resting.
    pub fn sync_visual_to_target(&mut self) {
        if let Some(target) = self.anim_target_geo {
            self.visual_geo = Some(target);
        }
    }

    /// Translates floating geometry of a window from `src_area` to `dst_area`.
    pub fn translate_float_geometry(
        geo: Rect,
        src_area: Option<Rect>,
        dst_area: Option<Rect>,
    ) -> Rect {
        let (new_x, new_y) = if let (Some(src), Some(dst)) = (src_area, dst_area) {
            let rel_x = geo.x - src.x;
            let rel_y = geo.y - src.y;
            let new_x = dst.x + rel_x;
            let new_y = dst.y + rel_y;
            let max_x = (dst.x + dst.width as i32 - 50).max(dst.x);
            let max_y = (dst.y + dst.height as i32 - 50).max(dst.y);
            (new_x.clamp(dst.x, max_x), new_y.clamp(dst.y, max_y))
        } else if let Some(dst) = dst_area {
            let cx = dst.x + (dst.width.saturating_sub(geo.width) / 2) as i32;
            let cy = dst.y + (dst.height.saturating_sub(geo.height) / 2) as i32;
            (cx, cy)
        } else {
            (geo.x, geo.y)
        };
        Rect::new(new_x, new_y, geo.width, geo.height)
    }

    /// Migrates a window's output and transforms both active and saved floating geometry.
    pub fn migrate_to_output(
        &mut self,
        dest_out_id: Option<ObjectId>,
        src_area: Option<Rect>,
        dest_area: Option<Rect>,
    ) {
        self.output = dest_out_id;

        // Translate saved floating geometry if present
        if let Some(saved) = self.float_geo {
            self.float_geo = Some(Self::translate_float_geometry(saved, src_area, dest_area));
        }

        // If currently floating, also translate active coordinates and visual geometry
        if self.floating {
            let current_geo = Rect::new(self.x, self.y, self.width, self.height);
            let new_geo = Self::translate_float_geometry(current_geo, src_area, dest_area);
            self.x = new_geo.x;
            self.y = new_geo.y;
            self.float_geo = Some(new_geo);
            self.visual_geo = Some(new_geo);
        }

        if self.fullscreen {
            self.pending_fullscreen_change = true;
        }
    }

    /// Applies rule properties to the window.
    pub fn apply_rule_properties(
        &mut self,
        config: RuleConfig,
        effective_usable_area: Option<Rect>,
    ) {
        let was_floating = self.floating;

        if let Some(fl) = config.float {
            self.floating = fl;
        }
        if let Some(s) = config.ssd {
            self.ssd = s;
        }

        if !self.initial_managed {
            if let Some(t) = config.tags {
                self.tags = t;
            }
            if let Some(fs) = config.fullscreen {
                self.fullscreen = fs;
                self.pending_fullscreen_change = true;
            }
            if let Some(out_id) = config.output {
                self.output = Some(out_id);
            }
        }

        if (!self.initial_managed || !was_floating)
            && let Some((w, h)) = config.dimensions
        {
            self.width = w;
            self.height = h;
            self.rule_dimensions = Some((w, h));
        }

        if !self.initial_managed || !was_floating || self.float_geo.is_none() {
            if let Some((px, py)) = config.position {
                self.x = px;
                self.y = py;
                self.float_geo = Some(Rect::new(px, py, self.width, self.height));
            } else if (config.dimensions.is_some() || self.floating)
                && let Some(usable) = effective_usable_area
                && self.width > 0
                && self.height > 0
            {
                let cx = (usable.x as i64 + ((usable.width as i64 - self.width as i64) / 2).max(0))
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                let cy = (usable.y as i64
                    + ((usable.height as i64 - self.height as i64) / 2).max(0))
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                self.x = cx;
                self.y = cy;
                self.float_geo = Some(Rect::new(cx, cy, self.width, self.height));
            }
        }
    }
}
