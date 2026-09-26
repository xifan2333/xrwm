//! Spatial and directional navigation logic across windows and outputs.

use crate::wm::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Next,
    Previous,
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "next" => Some(Self::Next),
            "previous" | "prev" => Some(Self::Previous),
            "left" | "h" => Some(Self::Left),
            "right" | "l" => Some(Self::Right),
            "up" | "k" => Some(Self::Up),
            "down" | "j" => Some(Self::Down),
            _ => None,
        }
    }
}

/// Helper function to pick an adjacent output based on logical (next/prev) or spatial (left/right/up/down) direction.
pub fn pick_adjacent_output<T: Clone>(
    outputs: &[(&T, i32, i32, u32, u32)],
    current_idx: usize,
    dir_str: &str,
) -> Option<T> {
    if outputs.len() <= 1 || current_idx >= outputs.len() {
        return None;
    }

    let (_, cur_x, cur_y, cur_w, cur_h) = outputs[current_idx];
    let cur_cx = cur_x + cur_w as i32 / 2;
    let cur_cy = cur_y + cur_h as i32 / 2;

    match dir_str.to_ascii_lowercase().as_str() {
        "next" => {
            let next_idx = (current_idx + 1) % outputs.len();
            Some(outputs[next_idx].0.clone())
        }
        "previous" | "prev" => {
            let prev_idx = if current_idx == 0 {
                outputs.len() - 1
            } else {
                current_idx - 1
            };
            Some(outputs[prev_idx].0.clone())
        }
        "left" | "h" => outputs
            .iter()
            .filter(|(_, x, _, w, _)| {
                let cx = *x + *w as i32 / 2;
                cx < cur_cx
            })
            .min_by_key(|(_, x, y, w, h)| {
                let cx = *x + *w as i32 / 2;
                let cy = *y + *h as i32 / 2;
                (cur_cx - cx).abs() * 2 + (cur_cy - cy).abs()
            })
            .map(|(t, ..)| (*t).clone()),
        "right" | "l" => outputs
            .iter()
            .filter(|(_, x, _, w, _)| {
                let cx = *x + *w as i32 / 2;
                cx > cur_cx
            })
            .min_by_key(|(_, x, y, w, h)| {
                let cx = *x + *w as i32 / 2;
                let cy = *y + *h as i32 / 2;
                (cx - cur_cx).abs() * 2 + (cur_cy - cy).abs()
            })
            .map(|(t, ..)| (*t).clone()),
        "up" | "k" => outputs
            .iter()
            .filter(|(_, _, y, _, h)| {
                let cy = *y + *h as i32 / 2;
                cy < cur_cy
            })
            .min_by_key(|(_, x, y, w, h)| {
                let cx = *x + *w as i32 / 2;
                let cy = *y + *h as i32 / 2;
                (cur_cy - cy).abs() * 2 + (cur_cx - cx).abs()
            })
            .map(|(t, ..)| (*t).clone()),
        "down" | "j" => outputs
            .iter()
            .filter(|(_, _, y, _, h)| {
                let cy = *y + *h as i32 / 2;
                cy > cur_cy
            })
            .min_by_key(|(_, x, y, w, h)| {
                let cx = *x + *w as i32 / 2;
                let cy = *y + *h as i32 / 2;
                (cy - cur_cy).abs() * 2 + (cur_cx - cx).abs()
            })
            .map(|(t, ..)| (*t).clone()),
        _ => None,
    }
}

/// Finds the target window in the given direction.
///
/// When `skip_floating` is true, floating windows are excluded from destination candidates,
/// but the currently focused window (even if floating) is preserved as the spatial and
/// order reference for navigation into tiled windows.
pub fn find_target_window(state: &AppState, dir: Direction, skip_floating: bool) -> Option<u32> {
    let candidates: Vec<&crate::wm::state::WindowItem> = state
        .windows
        .iter()
        .filter(|w| !w.closed && (!skip_floating || !w.floating) && state.is_window_visible(w))
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let focused_id = state.focused_window_id()?;
    let current_win = state
        .windows
        .iter()
        .find(|w| w.id == focused_id && !w.closed && state.is_window_visible(w));

    let current_in_candidates = candidates.iter().position(|w| w.id == focused_id);

    match dir {
        Direction::Next => {
            if let Some(idx) = current_in_candidates {
                if candidates.len() <= 1 {
                    return None;
                }
                let next_idx = (idx + 1) % candidates.len();
                Some(candidates[next_idx].id)
            } else {
                let current_pos = state.windows.iter().position(|w| w.id == focused_id);
                if let Some(pos) = current_pos {
                    let next_cand = candidates.iter().find(|c| {
                        state.windows.iter().position(|w| w.id == c.id).unwrap_or(0) > pos
                    });
                    next_cand
                        .map(|w| w.id)
                        .or_else(|| candidates.first().map(|w| w.id))
                } else {
                    candidates.first().map(|w| w.id)
                }
            }
        }
        Direction::Previous => {
            if let Some(idx) = current_in_candidates {
                if candidates.len() <= 1 {
                    return None;
                }
                let prev_idx = (idx + candidates.len() - 1) % candidates.len();
                Some(candidates[prev_idx].id)
            } else {
                let current_pos = state.windows.iter().position(|w| w.id == focused_id);
                if let Some(pos) = current_pos {
                    let prev_cand = candidates.iter().rev().find(|c| {
                        state.windows.iter().position(|w| w.id == c.id).unwrap_or(0) < pos
                    });
                    prev_cand
                        .map(|w| w.id)
                        .or_else(|| candidates.last().map(|w| w.id))
                } else {
                    candidates.last().map(|w| w.id)
                }
            }
        }
        Direction::Left | Direction::Right | Direction::Up | Direction::Down => {
            if current_in_candidates.is_some() && candidates.len() <= 1 {
                return None;
            }

            let (fx, fy) = if let Some(cw) = current_win {
                (
                    cw.x + cw.effective_width() as i32 / 2,
                    cw.y + cw.effective_height() as i32 / 2,
                )
            } else {
                (0, 0)
            };

            let mut best_id = None;
            let mut best_dist = i64::MAX;

            for w in &candidates {
                if w.id == focused_id {
                    continue;
                }
                let cx = w.x + w.effective_width() as i32 / 2;
                let cy = w.y + w.effective_height() as i32 / 2;
                let dx = cx - fx;
                let dy = cy - fy;

                let in_direction = match dir {
                    Direction::Left => dx < 0 && dx.abs() >= dy.abs(),
                    Direction::Right => dx > 0 && dx.abs() >= dy.abs(),
                    Direction::Up => dy < 0 && dy.abs() >= dx.abs(),
                    Direction::Down => dy > 0 && dy.abs() >= dx.abs(),
                    _ => false,
                };

                if in_direction {
                    let dist = (dx as i64).pow(2) + (dy as i64).pow(2);
                    if dist < best_dist {
                        best_dist = dist;
                        best_id = Some(w.id);
                    }
                }
            }

            if best_id.is_none() {
                for w in &candidates {
                    if w.id == focused_id {
                        continue;
                    }
                    let cx = w.x + w.effective_width() as i32 / 2;
                    let cy = w.y + w.effective_height() as i32 / 2;
                    let dx = cx - fx;
                    let dy = cy - fy;

                    let in_half_plane = match dir {
                        Direction::Left => dx < 0,
                        Direction::Right => dx > 0,
                        Direction::Up => dy < 0,
                        Direction::Down => dy > 0,
                        _ => false,
                    };

                    if in_half_plane {
                        let dist = (dx as i64).pow(2) + (dy as i64).pow(2);
                        if dist < best_dist {
                            best_dist = dist;
                            best_id = Some(w.id);
                        }
                    }
                }
            }

            best_id
        }
    }
}

/// Finds the target output given a direction string (next, prev, left, right, up, down).
pub fn find_target_output(
    state: &AppState,
    dir_str: &str,
) -> Option<wayland_backend::client::ObjectId> {
    if state.outputs.len() <= 1 {
        return None;
    }

    let mut output_list: Vec<(
        &wayland_backend::client::ObjectId,
        &crate::wm::state::OutputItem,
    )> = state.outputs.iter().collect();
    output_list.sort_by_key(|(_, o)| (o.x, o.y));

    let current_id = state.get_focused_output_id()?;
    let current_idx = output_list.iter().position(|(id, _)| **id == current_id)?;

    let tuples: Vec<(&wayland_backend::client::ObjectId, i32, i32, u32, u32)> = output_list
        .iter()
        .map(|(id, o)| (*id, o.x, o.y, o.width, o.height))
        .collect();

    pick_adjacent_output(&tuples, current_idx, dir_str)
}
