//! Dynamic tiling layout engine for xrwm (rivertile master-stack style).

use wayland_backend::client::ObjectId;

use crate::state::AppState;
use crate::tag::TagMask;
use crate::window::WindowLayer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MainLocation {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutConfig {
    pub split_ratio: f32,
    pub stack_split_ratio: f32,
    pub main_count: u32,
    pub main_location: MainLocation,
    pub view_padding: u32,
    pub outer_padding: u32,
    pub monocle: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            split_ratio: 0.55,
            stack_split_ratio: 0.5,
            main_count: 1,
            main_location: MainLocation::Left,
            view_padding: 4,
            outer_padding: 4,
            monocle: false,
        }
    }
}

pub trait Layout: Send + Sync {
    fn name(&self) -> &'static str;
    fn arrange(&self, usable_area: Rect, count: usize, config: &LayoutConfig) -> Vec<Rect>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MasterStackLayout;

impl Layout for MasterStackLayout {
    fn name(&self) -> &'static str {
        "master-stack"
    }

    fn arrange(&self, usable_area: Rect, count: usize, config: &LayoutConfig) -> Vec<Rect> {
        if count == 0 {
            return Vec::new();
        }

        // Monocle mode fills the entire usable area for all windows
        if config.monocle {
            return vec![usable_area; count];
        }

        let max_op = ((usable_area.width.min(usable_area.height) / 2).saturating_sub(1)) as i32;
        let op = (config.outer_padding as i32).clamp(0, max_op.max(0));

        // Single window with outer padding
        if count == 1 {
            let w = (usable_area.width as i32 - 2 * op).max(1) as u32;
            let h = (usable_area.height as i32 - 2 * op).max(1) as u32;
            return vec![Rect::new(usable_area.x + op, usable_area.y + op, w, h)];
        }

        let mut rects = Vec::with_capacity(count);
        let main_count = (config.main_count as usize).clamp(1, count);
        let has_stack = count > main_count;

        let inner_x = usable_area.x + op;
        let inner_y = usable_area.y + op;
        let inner_w = (usable_area.width as i32 - 2 * op).max(1);
        let inner_h = (usable_area.height as i32 - 2 * op).max(1);

        // Clamp view padding based on usable inner dimensions and window count gaps
        let max_gaps = (count as i32 - 1).max(1);
        let max_vp_w = (inner_w - count as i32).max(0) / max_gaps;
        let max_vp_h = (inner_h - count as i32).max(0) / max_gaps;
        let max_vp = max_vp_w.min(max_vp_h);
        let vp = (config.view_padding as i32).clamp(0, max_vp);

        let split_ratio = if config.split_ratio.is_finite() {
            config.split_ratio.clamp(0.1, 0.9)
        } else {
            0.55
        };
        let stack_split_ratio = if config.stack_split_ratio.is_finite() {
            config.stack_split_ratio.clamp(0.1, 0.9)
        } else {
            0.50
        };

        match config.main_location {
            MainLocation::Left | MainLocation::Right => {
                let (main_w, stack_w) = if has_stack {
                    let total_available = inner_w - vp;
                    let mw = ((total_available as f32) * split_ratio).round() as i32;
                    let sw = total_available - mw;
                    (mw.max(1), sw.max(1))
                } else {
                    (inner_w, 0)
                };

                let (master_start_x, stack_start_x) = if !has_stack {
                    (inner_x, inner_x)
                } else if config.main_location == MainLocation::Left {
                    (inner_x, inner_x + main_w + vp)
                } else {
                    (inner_x + stack_w + vp, inner_x)
                };

                // Arrange Master column
                let main_total_h = inner_h - vp * (main_count as i32 - 1);
                let main_h_step = main_total_h / (main_count as i32);
                let mut curr_y = inner_y;
                for i in 0..main_count {
                    let rem_h = (inner_y + inner_h - curr_y).max(1);
                    let h = if i == main_count - 1 {
                        rem_h as u32
                    } else {
                        main_h_step.clamp(1, rem_h) as u32
                    };
                    rects.push(Rect::new(master_start_x, curr_y, main_w as u32, h));
                    curr_y = (curr_y + h as i32 + vp).min(inner_y + inner_h);
                }

                // Arrange Stack column
                if has_stack {
                    let stack_count = count - main_count;
                    let total_stack_h = inner_h - vp * (stack_count as i32 - 1);

                    if stack_count == 1 {
                        let h = total_stack_h.max(1) as u32;
                        rects.push(Rect::new(stack_start_x, inner_y, stack_w as u32, h));
                    } else {
                        let rem_count = (stack_count - 1) as i32;
                        let first_h = ((total_stack_h as f32) * stack_split_ratio).round() as i32;
                        let max_first_h = (total_stack_h - rem_count).max(1);
                        let first_h = first_h.clamp(1, max_first_h);

                        rects.push(Rect::new(
                            stack_start_x,
                            inner_y,
                            stack_w as u32,
                            first_h as u32,
                        ));

                        let rem_total_h = total_stack_h - first_h;
                        let rem_step = rem_total_h / rem_count;
                        let mut curr_y = (inner_y + first_h + vp).min(inner_y + inner_h);

                        for i in 1..stack_count {
                            let rem_h = (inner_y + inner_h - curr_y).max(1);
                            let h = if i == stack_count - 1 {
                                rem_h as u32
                            } else {
                                rem_step.clamp(1, rem_h) as u32
                            };
                            rects.push(Rect::new(stack_start_x, curr_y, stack_w as u32, h));
                            curr_y = (curr_y + h as i32 + vp).min(inner_y + inner_h);
                        }
                    }
                }
            }
            MainLocation::Top | MainLocation::Bottom => {
                let (main_h, stack_h) = if has_stack {
                    let total_available = inner_h - vp;
                    let mh = ((total_available as f32) * split_ratio).round() as i32;
                    let sh = total_available - mh;
                    (mh.max(1), sh.max(1))
                } else {
                    (inner_h, 0)
                };

                let (master_start_y, stack_start_y) = if !has_stack {
                    (inner_y, inner_y)
                } else if config.main_location == MainLocation::Top {
                    (inner_y, inner_y + main_h + vp)
                } else {
                    (inner_y + stack_h + vp, inner_y)
                };

                // Arrange Master row
                let main_total_w = inner_w - vp * (main_count as i32 - 1);
                let main_w_step = main_total_w / (main_count as i32);
                let mut curr_x = inner_x;
                for i in 0..main_count {
                    let rem_w = (inner_x + inner_w - curr_x).max(1);
                    let w = if i == main_count - 1 {
                        rem_w as u32
                    } else {
                        main_w_step.clamp(1, rem_w) as u32
                    };
                    rects.push(Rect::new(curr_x, master_start_y, w, main_h as u32));
                    curr_x = (curr_x + w as i32 + vp).min(inner_x + inner_w);
                }

                // Arrange Stack row
                if has_stack {
                    let stack_count = count - main_count;
                    let total_stack_w = inner_w - vp * (stack_count as i32 - 1);

                    if stack_count == 1 {
                        let w = total_stack_w.max(1) as u32;
                        rects.push(Rect::new(inner_x, stack_start_y, w, stack_h as u32));
                    } else {
                        let rem_count = (stack_count - 1) as i32;
                        let first_w = ((total_stack_w as f32) * stack_split_ratio).round() as i32;
                        let max_first_w = (total_stack_w - rem_count).max(1);
                        let first_w = first_w.clamp(1, max_first_w);

                        rects.push(Rect::new(
                            inner_x,
                            stack_start_y,
                            first_w as u32,
                            stack_h as u32,
                        ));

                        let rem_total_w = total_stack_w - first_w;
                        let rem_step = rem_total_w / rem_count;
                        let mut curr_x = (inner_x + first_w + vp).min(inner_x + inner_w);

                        for i in 1..stack_count {
                            let rem_w = (inner_x + inner_w - curr_x).max(1);
                            let w = if i == stack_count - 1 {
                                rem_w as u32
                            } else {
                                rem_step.clamp(1, rem_w) as u32
                            };
                            rects.push(Rect::new(curr_x, stack_start_y, w, stack_h as u32));
                            curr_x = (curr_x + w as i32 + vp).min(inner_x + inner_w);
                        }
                    }
                }
            }
        }

        rects
    }
}

/// Target geometry and visibility plan calculated for a single managed window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowArrangePlan {
    pub window_id: u32,
    /// Target geometry calculated for this window (tiled target or floating resting box).
    pub target_rect: Option<Rect>,
    /// Whether this window is arranged as a floating window.
    pub is_floating: bool,
    /// Whether the window is visible on the current active tags / floating collapse state.
    pub is_visible: bool,
}

/// Comprehensive window arrangement plan computed for a display frame manage tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArrangePlan {
    /// Collapsible tag masks projected per output (output ObjectId -> updated floating_hidden_tags).
    pub output_floating_hidden_tags: Vec<(ObjectId, TagMask)>,
    /// Calculated target arrangements for individual windows.
    pub window_plans: Vec<WindowArrangePlan>,
    /// Whether any pointer gesture (move, resize, drag-swap) is actively held.
    pub is_any_pointer_op: bool,
}

impl ArrangePlan {
    /// Returns true if no windows or outputs require arrangement.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.window_plans.is_empty() && self.output_floating_hidden_tags.is_empty()
    }
}

/// Computes a pure, side-effect-free window arrangement plan for the current application state.
pub fn compute_arrange_plan(state: &AppState) -> ArrangePlan {
    if state.outputs.is_empty() {
        return ArrangePlan::default();
    }

    // 1. Project floating_hidden_tags to maintain per-tag collapse invariant
    let is_single_output = state.outputs.len() <= 1;
    let mut output_floating_hidden_tags = Vec::with_capacity(state.outputs.len());
    for (out_id, out) in &state.outputs {
        let collapsible_mask: TagMask = state
            .windows
            .iter()
            .filter(|w| {
                !w.closed
                    && w.layer().policy().collapsible
                    && (w.output.as_ref() == Some(out_id) || is_single_output)
            })
            .map(|w| w.tags)
            .fold(0, |acc, tags| acc | tags);

        output_floating_hidden_tags
            .push((out_id.clone(), out.floating_hidden_tags & collapsible_mask));
    }

    let layout_engine = MasterStackLayout;
    let mut window_plans = Vec::with_capacity(state.windows.len());

    let active_outputs: Vec<(ObjectId, Rect)> = state
        .outputs
        .iter()
        .map(|(id, o)| (id.clone(), o.usable_area))
        .collect();

    // 2. Compute tiled layout for each active output
    for (out_id, usable_area) in active_outputs {
        let out_tag_state = state.tag_state_for_output(Some(&out_id));
        let mut tiled_indices: Vec<usize> = Vec::new();
        for (i, w) in state.windows.iter().enumerate() {
            let matches_output = match (&w.output, out_id.is_null()) {
                (Some(wo), false) => wo == &out_id,
                _ => true,
            };
            if matches_output
                && w.layer().policy().tiled_layout
                && out_tag_state.is_view_visible(w.tags)
            {
                tiled_indices.push(i);
            }
        }

        let mut out_layout_config = state.layout_config.clone();
        if let Some(out) = state.outputs.get(&out_id) {
            out_layout_config.monocle = out.monocle;
        }

        let rects = layout_engine.arrange(usable_area, tiled_indices.len(), &out_layout_config);

        for (slot, &idx) in tiled_indices.iter().enumerate() {
            if let Some(&rect) = rects.get(slot) {
                window_plans.push(WindowArrangePlan {
                    window_id: state.windows[idx].id(),
                    target_rect: Some(rect),
                    is_floating: false,
                    is_visible: true,
                });
            }
        }
    }

    let is_any_pointer_op = state
        .seats
        .values()
        .any(|s| !matches!(&s.op, crate::seat::SeatOp::None));

    // 3. Compute floating window visibility and placement plans
    for w in state
        .windows
        .iter()
        .filter(|w| !w.layer().policy().tiled_layout && w.layer() != WindowLayer::Fullscreen)
    {
        let is_visible = state.is_window_visible(w);
        window_plans.push(WindowArrangePlan {
            window_id: w.id(),
            target_rect: None,
            is_floating: true,
            is_visible,
        });
    }

    ArrangePlan {
        output_floating_hidden_tags,
        window_plans,
        is_any_pointer_op,
    }
}

/// Executes the computed arrange plan against application state and animations.
pub fn execute_arrange_plan(
    state: &mut AppState,
    plan: &ArrangePlan,
    op_was_released: bool,
) -> bool {
    // 1. Apply projected floating collapse masks per output
    for (out_id, mask) in &plan.output_floating_hidden_tags {
        if let Some(out) = state.outputs.get_mut(out_id) {
            out.floating_hidden_tags = *mask;
        }
    }

    let mut any_geo_changed = false;

    // 2. Apply tiled and floating window target geometries
    for win_plan in &plan.window_plans {
        if let Some(w) = state
            .windows
            .iter_mut()
            .find(|w| w.id() == win_plan.window_id)
        {
            if win_plan.is_floating {
                if w.arrange_floating(win_plan.is_visible, plan.is_any_pointer_op) {
                    any_geo_changed = true;
                }
            } else if let Some(rect) = win_plan.target_rect
                && w.arrange_tiled(rect)
            {
                any_geo_changed = true;
            }
        }
    }

    // 3. Apply decoration requests (SSD / CSD) and mark initial managed
    for w in &mut state.windows {
        w.apply_ssd();
        w.mark_initial_managed();
    }

    // 4. Trigger animation lifecycle if geometries changed
    if !plan.is_any_pointer_op && any_geo_changed && state.anim.enabled {
        state.anim.start();
    }
    if state.anim.is_animating() || op_was_released {
        state.manage_dirty();
    }
    if !state.anim.is_animating() {
        if state.anim.start_time.is_some() {
            state.anim.stop();
        }
        if state.tag_slide_dir.is_some() {
            state.tag_slide_dir = None;
            state.tag_anim_old_mask = crate::tag::TAG_NONE;
        }
        if !state.anim.enabled {
            for w in state.windows.iter_mut().filter(|w| !w.closed) {
                w.sync_visual_to_target();
            }
        }
    }

    // 5. Apply any pending pointer warps during manage sequence
    for seat in state.seats.values_mut() {
        if let Some((wx, wy)) = seat.pending_warp.take() {
            seat.proxy.pointer_warp(wx, wy);
        }
    }

    any_geo_changed
}
