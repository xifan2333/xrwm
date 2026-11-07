//! Frame rendering plan computation and Wayland River protocol execution adapter.

use crate::animation::{calculate_clip_box, interpolate_rect};
use crate::layout::Rect;
use crate::protocol::{
    river_node_v1::RiverNodeV1,
    river_window_v1::{Edges, RiverWindowV1},
};
use crate::seat::{LayerShellFocus, SeatOp};
use crate::state::AppState;
use crate::status::hex_to_river_rgba;
use crate::window::WindowItem;

/// Visual border configuration planned for a window in the current frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderPlan {
    pub edges: Edges,
    pub width: i32,
    pub color: (u32, u32, u32, u32),
}

/// Positioning and clipping instructions planned for a window surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSurfacePlan {
    /// Target coordinates for `node.set_position`.
    pub position: (i32, i32),
    /// River clip box `(clip_x, clip_y, clip_width, clip_height)` if clipping is active.
    pub clip_box: Option<(i32, i32, i32, i32)>,
    /// Calculated logical render geometry for tracking and animations.
    pub render_geo: Rect,
    /// Whether the window is visible in this frame (`proxy.show()` vs `proxy.hide()`).
    pub is_visible: bool,
    /// Static target geometry to lock when the window is resting.
    pub lock_static_target: Option<Rect>,
}

/// Comprehensive render plan for a single window.
#[derive(Debug, Clone)]
pub struct WindowFramePlan {
    pub window_id: u32,
    pub proxy: RiverWindowV1,
    pub node: RiverNodeV1,
    pub border: BorderPlan,
    pub surface: WindowSurfacePlan,
}

/// Complete plan for an entire render frame tick.
#[derive(Debug, Clone, Default)]
pub struct FramePlan {
    pub window_plans: Vec<WindowFramePlan>,
    pub z_order_nodes: Vec<RiverNodeV1>,
}

/// Computes a pure, side-effect-free frame plan for the current state.
pub fn compute_frame_plan(state: &AppState) -> FramePlan {
    if state.outputs.is_empty() {
        return FramePlan::default();
    }

    let border_width = (state.border_width.min(i32::MAX as u32)) as i32;
    let (fr, fg, fb, fa) = hex_to_river_rgba(&state.border_color_focused);
    let (ur, ug, ub, ua) = hex_to_river_rgba(&state.border_color_unfocused);
    let focused_proxy = state.seats.values().find_map(|s| {
        if s.layer_focus == LayerShellFocus::None {
            s.focused.clone()
        } else {
            None
        }
    });
    let (hide_x, hide_y) = state.offscreen_hiding_position();

    let is_animating = state.anim.is_animating();
    let progress = state.anim.progress();
    let is_tag_animating = state.tag_slide_dir.is_some() && is_animating;

    let active_move_proxy = state.seats.values().find_map(|s| match &s.op {
        SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => Some(proxy.clone()),
        _ => None,
    });

    let mut window_plans = Vec::with_capacity(state.windows.len());

    for w in &state.windows {
        let is_focused = focused_proxy.as_ref() == Some(w.proxy());
        let (cr, cg, cb, ca) = if is_focused {
            (fr, fg, fb, fa)
        } else {
            (ur, ug, ub, ua)
        };

        let border = if w.is_fullscreen() || !w.ssd() {
            BorderPlan {
                edges: Edges::empty(),
                width: 0,
                color: (0, 0, 0, 0),
            }
        } else {
            BorderPlan {
                edges: Edges::all(),
                width: border_width,
                color: (cr, cg, cb, ca),
            }
        };

        let usable_area = w
            .output()
            .and_then(|id| state.outputs.get(id))
            .map(|o| o.usable_area)
            .or_else(|| state.outputs.values().next().map(|o| o.usable_area));

        let Some(usable_area) = usable_area else {
            window_plans.push(WindowFramePlan {
                window_id: w.id(),
                proxy: w.proxy().clone(),
                node: w.node().clone(),
                border,
                surface: WindowSurfacePlan {
                    position: (hide_x, hide_y),
                    clip_box: None,
                    render_geo: Rect::new(w.x(), w.y(), w.width(), w.height()),
                    is_visible: false,
                    lock_static_target: None,
                },
            });
            continue;
        };

        if state.window_visibility(w) == crate::state::WindowVisibility::HiddenByCollapse {
            window_plans.push(WindowFramePlan {
                window_id: w.id(),
                proxy: w.proxy().clone(),
                node: w.node().clone(),
                border,
                surface: WindowSurfacePlan {
                    position: (hide_x, hide_y),
                    clip_box: None,
                    render_geo: Rect::new(w.x(), w.y(), w.width(), w.height()),
                    is_visible: false,
                    lock_static_target: None,
                },
            });
            continue;
        }

        let slide_offset = if let Some(dir) = state.tag_slide_dir {
            match dir {
                crate::animation::SlideDirection::Right => usable_area.width as i32,
                crate::animation::SlideDirection::Left => -(usable_area.width as i32),
            }
        } else {
            0
        };

        let out_tag_state = state.tag_state_for_output(w.output());
        let is_in_current = out_tag_state.is_view_visible(w.tags());
        let is_in_old = is_tag_animating && (w.tags() & state.tag_anim_old_mask) != 0;

        if is_in_current || is_in_old {
            let is_interactive = active_move_proxy.as_ref() == Some(w.proxy());
            let target = Rect::new(w.x(), w.y(), w.width(), w.height());

            let (render_geo, is_visible, edge_clip_box, lock_static_target) = if is_interactive {
                (target, true, None, None)
            } else if w.is_fullscreen() {
                // Fullscreen windows are positioned by the compositor and cannot slide.
                // When on an inactive tag, immediately hide them to prevent covering other workspaces,
                // while fully preserving their fullscreen state (w.fullscreen remains true).
                // When returning to this tag, w.proxy.show() will be invoked to unhide the window.
                (target, is_in_current, None, None)
            } else if is_tag_animating {
                let is_shared = is_in_current && (w.tags() & state.tag_anim_old_mask) != 0;
                if is_shared {
                    (target, true, None, None)
                } else if is_in_old && !is_in_current {
                    // Old tag window sliding out
                    let end_x = target.x - slide_offset;
                    let cur_x = crate::animation::interpolate(target.x, end_x, progress);
                    let cur_geo = Rect::new(cur_x, target.y, target.width, target.height);
                    let clip = calculate_clip_box(cur_geo, usable_area, border_width);
                    let visible = clip.is_some();
                    (cur_geo, visible, clip, None)
                } else {
                    // New tag window sliding in
                    let start_x = target.x + slide_offset;
                    let cur_x = crate::animation::interpolate(start_x, target.x, progress);
                    let cur_geo = Rect::new(cur_x, target.y, target.width, target.height);
                    let clip = calculate_clip_box(cur_geo, usable_area, border_width);
                    let visible = clip.is_some();
                    (cur_geo, visible, clip, None)
                }
            } else if is_animating && w.anim_start_geo().is_some_and(|start| start != target) {
                let start = w.anim_start_geo().unwrap_or(target);
                let geo = interpolate_rect(start, target, progress);
                let b = border_width.max(0) as i64;
                let win_left = geo.x as i64 - b;
                let win_right = geo.x as i64 + geo.width as i64 + b;
                let win_top = geo.y as i64 - b;
                let win_bottom = geo.y as i64 + geo.height as i64 + b;

                let scr_left = usable_area.x as i64;
                let scr_right = usable_area.x as i64 + usable_area.width as i64;
                let scr_top = usable_area.y as i64;
                let scr_bottom = usable_area.y as i64 + usable_area.height as i64;

                let is_fully_inside = win_left >= scr_left
                    && win_top >= scr_top
                    && win_right <= scr_right
                    && win_bottom <= scr_bottom;

                if is_fully_inside {
                    (geo, true, None, None)
                } else {
                    let clip = calculate_clip_box(geo, usable_area, border_width);
                    let visible = clip.is_some();
                    (geo, visible, clip, None)
                }
            } else {
                (target, true, None, Some(target))
            };

            // Consolidated clip box and position offset resolution:
            // 1. Edge clipping (sliding offscreen) takes precedence.
            // 2. Oversized floating client buffers (such as imv's preallocated 1280x720) are clipped
            //    accounting for OpenGL's bottom-left viewport anchor (excess_h offset).
            // 3. Otherwise, clipping is cleanly disabled.
            let (clip_box, clip_offset_y) = if let Some(clip) = edge_clip_box {
                (Some(clip), 0)
            } else if w.is_floating()
                && !w.is_fullscreen()
                && (w.content_width().is_some_and(|cw| cw > render_geo.width)
                    || w.content_height().is_some_and(|ch| ch > render_geo.height))
                && render_geo.width > 0
                && render_geo.height > 0
            {
                let ch = w.content_height().unwrap_or(render_geo.height);
                let excess_h = ch.saturating_sub(render_geo.height) as i32;
                (
                    Some((
                        0,
                        excess_h,
                        render_geo.width.min(i32::MAX as u32) as i32,
                        render_geo.height.min(i32::MAX as u32) as i32,
                    )),
                    excess_h,
                )
            } else {
                (None, 0)
            };

            let position = if is_visible {
                (render_geo.x, render_geo.y - clip_offset_y)
            } else {
                (hide_x, hide_y)
            };

            window_plans.push(WindowFramePlan {
                window_id: w.id(),
                proxy: w.proxy().clone(),
                node: w.node().clone(),
                border,
                surface: WindowSurfacePlan {
                    position,
                    clip_box,
                    render_geo,
                    is_visible,
                    lock_static_target,
                },
            });
        } else {
            window_plans.push(WindowFramePlan {
                window_id: w.id(),
                proxy: w.proxy().clone(),
                node: w.node().clone(),
                border,
                surface: WindowSurfacePlan {
                    position: (hide_x, hide_y),
                    clip_box: None,
                    render_geo: Rect::new(w.x(), w.y(), w.width(), w.height()),
                    is_visible: false,
                    lock_static_target: None,
                },
            });
        }
    }

    // Enforce strict layered Z-ordering (matching river-classic where .float > .layout):
    //
    // Layer 1 (Bottom): Tiled windows.
    //   Place unfocused tiled windows first, then focused tiled window (if focused is tiled).
    let mut z_order_nodes = Vec::with_capacity(state.windows.len());

    for w in state
        .windows
        .iter()
        .filter(|w| !w.is_floating() && state.is_window_visible(w))
    {
        if focused_proxy.as_ref() != Some(w.proxy()) {
            z_order_nodes.push(w.node().clone());
        }
    }
    if let Some(ref focused) = focused_proxy
        && let Some(win) = state
            .windows
            .iter()
            .find(|w| w.proxy() == focused && !w.is_floating() && state.is_window_visible(w))
    {
        z_order_nodes.push(win.node().clone());
    }

    // Layer 2 (Top): Floating windows.
    //   Place unfocused floating windows first, then focused floating window (if focused is floating).
    //   Floating windows are GUARANTEED to ALWAYS remain above all tiled windows!
    for w in state
        .windows
        .iter()
        .filter(|w| w.is_floating() && state.is_window_visible(w))
    {
        if focused_proxy.as_ref() != Some(w.proxy()) {
            z_order_nodes.push(w.node().clone());
        }
    }
    if let Some(ref focused) = focused_proxy
        && let Some(win) = state
            .windows
            .iter()
            .find(|w| w.proxy() == focused && w.is_floating() && state.is_window_visible(w))
    {
        z_order_nodes.push(win.node().clone());
    }

    // Layer 3 (Absolute Top): Fullscreen windows
    for w in state
        .windows
        .iter()
        .filter(|w| w.is_fullscreen() && state.is_window_visible(w))
    {
        z_order_nodes.push(w.node().clone());
    }

    FramePlan {
        window_plans,
        z_order_nodes,
    }
}

/// Executes the computed frame plan against the River compositor and updates window states.
pub fn execute_frame_plan(windows: &mut [WindowItem], plan: &FramePlan) {
    for win_plan in &plan.window_plans {
        if let Some(w) = windows.iter_mut().find(|w| w.id() == win_plan.window_id) {
            // 1. Borders
            w.proxy().set_borders(
                win_plan.border.edges,
                win_plan.border.width,
                win_plan.border.color.0,
                win_plan.border.color.1,
                win_plan.border.color.2,
                win_plan.border.color.3,
            );

            // 2. Clip box
            if let Some((cx, cy, cw, ch)) = win_plan.surface.clip_box {
                w.proxy().set_clip_box(cx, cy, cw, ch);
            } else {
                w.proxy().set_clip_box(0, 0, 0, 0);
            }

            // 3. Static target lock if applicable
            if let Some(target) = win_plan.surface.lock_static_target {
                w.lock_static_geometry(target);
            }

            // 4. Record render frame
            w.record_render_frame(win_plan.surface.render_geo, win_plan.surface.is_visible);

            // 5. Visibility and position
            if win_plan.surface.is_visible {
                w.proxy().show();
                w.node()
                    .set_position(win_plan.surface.position.0, win_plan.surface.position.1);
            } else {
                w.proxy().hide();
                w.node()
                    .set_position(win_plan.surface.position.0, win_plan.surface.position.1);
            }
        }
    }

    // 6. Layer Z-ordering
    for node in &plan.z_order_nodes {
        node.place_top();
    }
}
