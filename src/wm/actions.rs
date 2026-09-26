//! Window manager actions and IPC command dispatch.

use std::time::Duration;

use crate::ipc::IpcCommand;
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::wm::state::AppState;
use crate::wm::state::AttachMode;
use crate::wm::state::WindowRule;
use crate::wm::state::reap_zombies;
use crate::wm::state::spawn_init_script;

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

impl AppState {
    /// Closes the currently focused window.
    pub fn close_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Ok("no view focused to close".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.pending_close = true;
        }

        self.manage_dirty();
        Ok(format!("closed window {id}"))
    }

    /// Toggles floating / tiled class on the focused window.
    pub fn toggle_float_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.floating = !w.floating;
            let is_floating = w.floating;
            tracing::info!("toggle_float_focused: window {id} -> floating={is_floating}");

            if is_floating {
                if let Some(saved) = w.float_geo {
                    w.x = saved.x;
                    w.y = saved.y;
                    w.width = saved.width;
                    w.height = saved.height;
                } else {
                    w.float_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));
                }
            } else {
                w.float_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));
            }

            let proxy = w.proxy.clone();
            for seat in self.seats.values_mut() {
                seat.set_focused_window(Some(proxy.clone()));
            }

            self.manage_dirty();
            Ok(format!("window {id} floating={is_floating}"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Snaps the focused window to a screen half and sets it to floating.
    pub fn snap_focused(&mut self, edge_str: &str) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let dir = match edge_str.to_ascii_lowercase().as_str() {
            "left" | "h" => Direction::Left,
            "right" | "l" => Direction::Right,
            "up" | "k" => Direction::Up,
            "down" | "j" => Direction::Down,
            _ => {
                return Err(format!(
                    "Invalid snap edge: '{edge_str}', expected left|right|up|down"
                ));
            }
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            let usable = w
                .output
                .as_ref()
                .and_then(|out_id| self.outputs.get(out_id))
                .map(|o| o.usable_area)
                .or_else(|| self.outputs.values().next().map(|o| o.usable_area));
            let Some(usable) = usable else {
                return Err("window has no active output".to_string());
            };

            w.floating = true;

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
                _ => unreachable!(),
            };

            w.x = new_x;
            w.y = new_y;
            w.width = new_w.max(crate::wm::MIN_WINDOW_DIMENSION);
            w.height = new_h.max(crate::wm::MIN_WINDOW_DIMENSION);
            w.float_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));
            w.visual_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));

            self.manage_dirty();
            Ok(format!("snapped window {id} {edge_str}"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Toggles fullscreen state on the focused window.
    pub fn toggle_fullscreen_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.fullscreen = !w.fullscreen;
            w.pending_fullscreen_change = true;
            let is_fs = w.fullscreen;
            tracing::info!("toggle_fullscreen_focused: window {id} -> fullscreen={is_fs}");
            self.manage_dirty();
            Ok(format!("window {id} fullscreen={is_fs}"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Toggles monocle layout mode on the focused output (maximizing all tiled views).
    pub fn toggle_monocle(&mut self) -> Result<String, String> {
        let focused_out = self.get_focused_output_id();
        let is_monocle = if let Some(ref out_id) = focused_out {
            if let Some(out) = self.outputs.get_mut(out_id) {
                out.monocle = !out.monocle;
                out.monocle
            } else {
                self.layout_config.monocle = !self.layout_config.monocle;
                self.layout_config.monocle
            }
        } else {
            self.layout_config.monocle = !self.layout_config.monocle;
            self.layout_config.monocle
        };
        self.layout_config.monocle = is_monocle;
        self.manage_dirty();
        let state = if is_monocle { "enabled" } else { "disabled" };
        Ok(format!("monocle mode {state}"))
    }

    /// Bumps the focused window to the master position in the layout stack.
    /// If the view on the top of the stack is already focused, bumps the second view to top (matching river-classic).
    pub fn zoom_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let focused_win_output = self
            .windows
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| w.output.clone());

        let visible_tiled: Vec<usize> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                !w.closed
                    && !w.floating
                    && !w.fullscreen
                    && self.is_window_visible(w)
                    && w.output == focused_win_output
            })
            .map(|(i, _)| i)
            .collect();

        if visible_tiled.len() <= 1 {
            return Ok(format!("zoomed window {id}"));
        }

        let focused_pos = visible_tiled
            .iter()
            .position(|&idx| self.windows[idx].id == id);
        let Some(pos) = focused_pos else {
            return Ok(format!("window {id} not in tiled layout"));
        };

        let (target_idx, dest_idx) = if pos == 0 {
            // Already at top: bump second view to top
            (visible_tiled[1], visible_tiled[0])
        } else {
            // Bump focused view to top
            (visible_tiled[pos], visible_tiled[0])
        };

        let win = self.windows.remove(target_idx);
        let win_id = win.id;
        self.windows.insert(dest_idx, win);
        self.manage_dirty();
        Ok(format!("zoomed window {win_id}"))
    }

    /// Finds the target window in the given direction.
    ///
    /// When `skip_floating` is true, floating windows are excluded from destination candidates,
    /// but the currently focused window (even if floating) is preserved as the spatial and
    /// order reference for navigation into tiled windows.
    pub fn find_target_window(&self, dir: Direction, skip_floating: bool) -> Option<u32> {
        let candidates: Vec<&crate::wm::state::WindowItem> = self
            .windows
            .iter()
            .filter(|w| !w.closed && (!skip_floating || !w.floating) && self.is_window_visible(w))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let focused_id = self.focused_window_id()?;
        let current_win = self
            .windows
            .iter()
            .find(|w| w.id == focused_id && !w.closed && self.is_window_visible(w));

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
                    let current_pos = self.windows.iter().position(|w| w.id == focused_id);
                    if let Some(pos) = current_pos {
                        let next_cand = candidates.iter().find(|c| {
                            self.windows.iter().position(|w| w.id == c.id).unwrap_or(0) > pos
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
                    let current_pos = self.windows.iter().position(|w| w.id == focused_id);
                    if let Some(pos) = current_pos {
                        let prev_cand = candidates.iter().rev().find(|c| {
                            self.windows.iter().position(|w| w.id == c.id).unwrap_or(0) < pos
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
    pub fn find_target_output(&self, dir_str: &str) -> Option<wayland_backend::client::ObjectId> {
        if self.outputs.len() <= 1 {
            return None;
        }

        let mut output_list: Vec<(
            &wayland_backend::client::ObjectId,
            &crate::wm::state::OutputItem,
        )> = self.outputs.iter().collect();
        output_list.sort_by_key(|(_, o)| (o.x, o.y));

        let current_id = self.get_focused_output_id()?;
        let current_idx = output_list.iter().position(|(id, _)| **id == current_id)?;

        let tuples: Vec<(&wayland_backend::client::ObjectId, i32, i32, u32, u32)> = output_list
            .iter()
            .map(|(id, o)| (*id, o.x, o.y, o.width, o.height))
            .collect();

        pick_adjacent_output(&tuples, current_idx, dir_str)
    }

    /// Focuses output in the specified direction.
    pub fn focus_output(&mut self, dir_str: &str) -> Result<String, String> {
        let target_id = self.find_target_output(dir_str);
        let Some(out_id) = target_id else {
            return Ok("no destination output found".to_string());
        };
        self.focused_output = Some(out_id.clone());

        if let Some(out) = self.outputs.get(&out_id) {
            self.tag_state = out.tag_state;
            self.previous_focused_tags = out.previous_focused_tags;
        }

        let dest_win = self
            .windows
            .iter()
            .find(|w| !w.closed && w.output == Some(out_id.clone()) && self.is_window_visible(w));
        if let Some(w) = dest_win {
            let proxy = w.proxy.clone();
            for seat in self.seats.values_mut() {
                seat.set_focused_window(Some(proxy.clone()));
            }
        }

        // Warp pointer if cursor_warp is enabled
        if self.cursor_warp != crate::wm::CursorWarp::Disabled
            && let Some(out) = self.outputs.get(&out_id)
        {
            let (cx, cy) = match (self.cursor_warp, dest_win) {
                (crate::wm::CursorWarp::OnFocusChange, Some(w)) => (
                    w.x + w.effective_width() as i32 / 2,
                    w.y + w.effective_height() as i32 / 2,
                ),
                _ => (out.x + out.width as i32 / 2, out.y + out.height as i32 / 2),
            };
            for seat in self.seats.values_mut() {
                seat.pending_warp = Some((cx, cy));
            }
        }

        self.manage_dirty();
        Ok(format!("focused output {:?}", out_id))
    }

    /// Sends the focused window to output in the specified direction.
    pub fn send_to_output(&mut self, dir_str: &str, current_tags: bool) -> Result<String, String> {
        let target_id = self.find_target_output(dir_str);
        let Some(out_id) = target_id else {
            return Ok("no destination output found".to_string());
        };

        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let dest_tags = self
            .outputs
            .get(&out_id)
            .map(|o| o.tag_state.focused)
            .unwrap_or(self.tag_state.focused);
        let source_out = self
            .windows
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| w.output.clone());
        let src_area = source_out
            .and_then(|so| self.outputs.get(&so))
            .map(|o| o.usable_area);
        let dest_area = self.outputs.get(&out_id).map(|o| o.usable_area);

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            AppState::migrate_window_to_output(w, Some(out_id.clone()), src_area, dest_area);
            if current_tags {
                w.tags = dest_tags;
            }
            if w.fullscreen {
                w.pending_fullscreen_change = true;
            }
            self.manage_dirty();
            Ok(format!("sent window {id} to output {:?}", out_id))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Shifts focus in the specified direction (next, prev, left, right, up, down).
    pub fn focus_view_direction(
        &mut self,
        dir_str: &str,
        skip_floating: bool,
    ) -> Result<String, String> {
        let visible_count = self
            .windows
            .iter()
            .filter(|w| !w.closed && (!skip_floating || !w.floating) && self.is_window_visible(w))
            .count();
        if visible_count == 0 {
            return Ok("no visible windows".to_string());
        }

        let dir = Direction::parse(dir_str).unwrap_or(Direction::Next);
        let target_id = self.find_target_window(dir, skip_floating);
        let Some(new_id) = target_id else {
            return Ok("no target window in direction".to_string());
        };

        if let Some(win) = self.windows.iter().find(|w| w.id == new_id) {
            let proxy = win.proxy.clone();
            let (cx, cy) = (
                win.x + win.effective_width() as i32 / 2,
                win.y + win.effective_height() as i32 / 2,
            );
            let out_id = win.output.clone();
            for seat in self.seats.values_mut() {
                seat.set_focused_window(Some(proxy.clone()));
                if self.cursor_warp == crate::wm::CursorWarp::OnFocusChange {
                    seat.pending_warp = Some((cx, cy));
                }
            }
            if let Some(out) = out_id {
                self.focused_output = Some(out);
            }
            self.manage_dirty();
            Ok(format!("focused window {new_id}"))
        } else {
            Err("failed to focus window".to_string())
        }
    }

    /// Swaps the focused window with the window in the specified direction.
    pub fn swap_direction(&mut self, dir_str: &str) -> Result<String, String> {
        let dir = Direction::parse(dir_str).unwrap_or(Direction::Next);
        let focused_id = self.focused_window_id();
        let Some(f_id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let target_id = self.find_target_window(dir, false);
        let Some(t_id) = target_id else {
            return Ok("no target window to swap with".to_string());
        };

        let f_idx = self.windows.iter().position(|w| w.id == f_id);
        let t_idx = self.windows.iter().position(|w| w.id == t_id);

        if let (Some(i1), Some(i2)) = (f_idx, t_idx) {
            self.windows.swap(i1, i2);
            self.manage_dirty();
            Ok(format!("swapped window {f_id} with {t_id}"))
        } else {
            Err("window not found to swap".to_string())
        }
    }

    /// Shifts focus to the next or previous visible window.
    pub fn focus_view(&mut self, next: bool) -> Result<String, String> {
        if next {
            self.focus_view_direction("next", false)
        } else {
            self.focus_view_direction("previous", false)
        }
    }

    /// Sets the focused tags mask on the WM.
    pub fn set_focused_tags(&mut self, mask: TagMask) -> Result<String, String> {
        if mask == TAG_NONE {
            return Err("at least one tag must be focused".to_string());
        }
        let focused_out_id = self.get_focused_output_id();
        let old_mask = if let Some(ref out_id) = focused_out_id
            && let Some(out) = self.outputs.get_mut(out_id)
        {
            let old = out.tag_state.focused;
            if old != mask {
                out.previous_focused_tags = old;
                out.tag_state.set_focused_tags(mask);
            }
            old
        } else {
            let old = self.tag_state.focused;
            if old != mask {
                self.previous_focused_tags = old;
                self.tag_state.set_focused_tags(mask);
            }
            old
        };

        if old_mask != mask {
            self.tag_state.set_focused_tags(mask);
            self.previous_focused_tags = old_mask;
            let dir = if mask > old_mask {
                crate::animation::SlideDirection::Right
            } else {
                crate::animation::SlideDirection::Left
            };
            self.tag_slide_dir = Some(dir);
            self.tag_anim_old_mask = old_mask;
            self.anim.start();
        }
        self.reconcile_focus();
        self.manage_dirty();
        Ok(format!("focused tags set to {mask}"))
    }

    /// Toggles tags back to the previous tag setup.
    pub fn focus_previous_tags(&mut self) -> Result<String, String> {
        let prev = if let Some(ref out_id) = self.get_focused_output_id()
            && let Some(out) = self.outputs.get(out_id)
        {
            out.previous_focused_tags
        } else {
            self.previous_focused_tags
        };
        self.set_focused_tags(prev)
    }

    /// Sends the focused window to the previous tag setup.
    pub fn send_to_previous_tags(&mut self) -> Result<String, String> {
        let prev = if let Some(ref out_id) = self.get_focused_output_id()
            && let Some(out) = self.outputs.get(out_id)
        {
            out.previous_focused_tags
        } else {
            self.previous_focused_tags
        };
        self.set_view_tags(prev)
    }

    /// Sets the master area location in the layout engine.
    pub fn set_main_location(
        &mut self,
        loc: crate::layout::MainLocation,
    ) -> Result<String, String> {
        self.layout_config.main_location = loc;
        self.manage_dirty();
        Ok(format!("main location set to {:?}", loc).to_lowercase())
    }

    /// Sets the padding around views in pixels (gaps).
    pub fn set_view_padding(&mut self, padding: u32) -> Result<String, String> {
        if padding > i32::MAX as u32 {
            return Err("view padding exceeds maximum allowed value".to_string());
        }
        self.layout_config.view_padding = padding;
        self.manage_dirty();
        Ok(format!("view padding set to {padding}px"))
    }

    /// Sets the padding around the outer perimeter of the layout area.
    pub fn set_outer_padding(&mut self, padding: u32) -> Result<String, String> {
        if padding > i32::MAX as u32 {
            return Err("outer padding exceeds maximum allowed value".to_string());
        }
        self.layout_config.outer_padding = padding;
        self.manage_dirty();
        Ok(format!("outer padding set to {padding}px"))
    }

    /// Sets the attach mode for newly spawned windows.
    pub fn set_attach_mode(&mut self, mode: AttachMode) -> Result<String, String> {
        self.attach_mode = mode;
        Ok(format!("attach mode set to {:?}", mode).to_lowercase())
    }

    /// Sets cursor warp mode.
    pub fn set_cursor_warp(&mut self, warp: crate::wm::CursorWarp) -> Result<String, String> {
        self.cursor_warp = warp;
        Ok(format!("cursor warp set to {:?}", warp).to_lowercase())
    }

    /// Sets focus-follows-cursor mode.
    pub fn set_focus_follows_cursor(
        &mut self,
        mode: crate::wm::FocusFollowsCursor,
    ) -> Result<String, String> {
        self.focus_follows_cursor = mode;
        Ok(format!("focus-follows-cursor set to {:?}", mode).to_lowercase())
    }

    /// Sets cursor hide timeout in milliseconds.
    pub fn set_hide_cursor_timeout(&mut self, timeout: u64) -> Result<String, String> {
        if timeout > 0 {
            return Err(
                "hide-cursor timeout is not supported: River protocol does not report continuous pointer activity or permit hiding client cursors".to_string(),
            );
        }
        self.cursor_hide_timeout = 0;
        Ok("hide cursor timeout disabled".to_string())
    }

    /// Sets whether cursor is hidden when typing.
    pub fn set_hide_cursor_when_typing(&mut self, enabled: bool) -> Result<String, String> {
        if enabled {
            return Err(
                "hide-cursor when-typing is not supported: River protocol does not expose non-binding keystrokes or permit hiding client cursors".to_string(),
            );
        }
        self.cursor_hide_when_typing = false;
        Ok("hide cursor when typing disabled".to_string())
    }

    /// Toggles the focused tags mask on the WM.
    pub fn toggle_focused_tags(&mut self, mask: TagMask) -> Result<String, String> {
        let focused_out_id = self.get_focused_output_id();
        let (old_mask, new_mask) = if let Some(ref out_id) = focused_out_id
            && let Some(out) = self.outputs.get_mut(out_id)
        {
            let old = out.tag_state.focused;
            let new = old ^ mask;
            if new != TAG_NONE && new != old {
                out.previous_focused_tags = old;
                out.tag_state.toggle_focused_tags(mask);
            }
            (old, new)
        } else {
            let old = self.tag_state.focused;
            let new = old ^ mask;
            if new != TAG_NONE && new != old {
                self.previous_focused_tags = old;
                self.tag_state.toggle_focused_tags(mask);
            }
            (old, new)
        };

        if new_mask != TAG_NONE && new_mask != old_mask {
            self.previous_focused_tags = old_mask;
            self.tag_state.focused = new_mask;
            let dir = if new_mask > old_mask {
                crate::animation::SlideDirection::Right
            } else {
                crate::animation::SlideDirection::Left
            };
            self.tag_slide_dir = Some(dir);
            self.tag_anim_old_mask = old_mask;
            self.anim.start();
        }
        self.reconcile_focus();
        self.manage_dirty();
        Ok(format!("focused tags toggled with {mask}"))
    }

    /// Assigns the focused window to the specified tag mask.
    pub fn set_view_tags(&mut self, mask: TagMask) -> Result<String, String> {
        if mask == TAG_NONE {
            return Err("a window must belong to at least one tag".to_string());
        }
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.tags = mask;
            self.sync_occupied_tags();
            self.reconcile_focus();
            self.manage_dirty();
            Ok(format!("window {id} tags set to {mask}"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Toggles specific tag bits on the focused window.
    pub fn toggle_view_tags(&mut self, mask: TagMask) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            let new_tags = w.tags ^ mask;
            if new_tags == TAG_NONE {
                return Err("cannot clear all tags on window".to_string());
            }
            w.tags = new_tags;
            self.sync_occupied_tags();
            self.reconcile_focus();
            self.manage_dirty();
            Ok(format!("window {id} tags toggled to {new_tags}"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Sets the spawn tagmask restricting tags of new views.
    pub fn set_spawn_tagmask(&mut self, mask: TagMask) -> Result<String, String> {
        self.spawn_tagmask = mask;
        Ok(format!("spawn tagmask set to {mask}"))
    }

    /// Declares a new modal keybinding mode.
    pub fn declare_mode(&mut self, mode: &str) -> Result<String, String> {
        let name = mode.trim();
        if name.is_empty() {
            return Err("mode name cannot be empty".to_string());
        }
        let canonical = name.to_ascii_lowercase();
        if !self.modes.iter().any(|m| m == &canonical) {
            self.modes.push(canonical.clone());
        }
        Ok(format!("declared mode {canonical}"))
    }

    /// Enters a declared modal keybinding mode.
    pub fn enter_mode(&mut self, mode: &str) -> Result<String, String> {
        let name = mode.trim();
        let canonical = self
            .modes
            .iter()
            .find(|m| m.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| format!("unknown mode '{name}', declare it first"))?;
        if self.session_locked && canonical != "locked" {
            return Err("cannot switch mode while session is locked".to_string());
        }
        if self.active_mode != canonical {
            self.active_mode = canonical.clone();
            self.mode_dirty = true;
            self.manage_dirty();
        }
        Ok(format!("entered mode {canonical}"))
    }

    /// Moves a floating window by delta pixels in the given direction.
    pub fn move_window(&mut self, dir_str: &str, delta: i32) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let dir = match dir_str.to_ascii_lowercase().as_str() {
            "left" | "h" => Direction::Left,
            "right" | "l" => Direction::Right,
            "up" | "k" => Direction::Up,
            "down" | "j" => Direction::Down,
            _ => {
                return Err(format!(
                    "Invalid direction: '{dir_str}', expected left|right|up|down"
                ));
            }
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            if !w.floating {
                return Ok(format!("window {id} is not floating, move ignored"));
            }

            match dir {
                Direction::Left => w.x -= delta,
                Direction::Right => w.x += delta,
                Direction::Up => w.y -= delta,
                Direction::Down => w.y += delta,
                _ => unreachable!(),
            }

            w.float_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));
            w.visual_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));
            let (new_x, new_y) = (w.x, w.y);
            self.manage_dirty();
            Ok(format!("moved window {id} to ({new_x}, {new_y})"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Resizes floating window dimensions or adjusts tiled split ratio.
    pub fn resize_window(&mut self, horizontal: bool, delta: i32) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            if w.floating {
                if horizontal {
                    w.width =
                        (w.width as i32 + delta).max(crate::wm::MIN_WINDOW_DIMENSION as i32) as u32;
                } else {
                    w.height = (w.height as i32 + delta).max(crate::wm::MIN_WINDOW_DIMENSION as i32)
                        as u32;
                }
                w.float_geo = Some(crate::layout::Rect::new(w.x, w.y, w.width, w.height));
                let new_w = w.width;
                let new_h = w.height;
                self.manage_dirty();
                Ok(format!("resized window {id} to {new_w}x{new_h}"))
            } else if horizontal {
                let ratio_delta = (delta as f32) / 1000.0;
                let new_ratio = (self.layout_config.split_ratio + ratio_delta).clamp(0.1, 0.9);
                self.layout_config.split_ratio = new_ratio;
                self.manage_dirty();
                Ok(format!("main ratio adjusted to {:.2}", new_ratio))
            } else {
                let ratio_delta = (delta as f32) / 1000.0;
                let new_ratio =
                    (self.layout_config.stack_split_ratio + ratio_delta).clamp(0.1, 0.9);
                self.layout_config.stack_split_ratio = new_ratio;
                self.manage_dirty();
                Ok(format!("stack ratio adjusted to {:.2}", new_ratio))
            }
        } else {
            Err("window not found".to_string())
        }
    }

    /// Sets or adjusts the layout main split ratio (supports absolute 0.55 or relative +/-0.05).
    pub fn set_main_ratio_arg(&mut self, arg: &str) -> Result<String, String> {
        let trimmed = arg.trim();
        let new_ratio = if trimmed.starts_with('+') || trimmed.starts_with('-') {
            let delta = trimmed.parse::<f32>().map_err(|_| "Invalid ratio delta")?;
            if !delta.is_finite() {
                return Err("Ratio delta must be a finite number".to_string());
            }
            let current = if self.layout_config.split_ratio.is_finite() {
                self.layout_config.split_ratio
            } else {
                0.55
            };
            (current + delta).clamp(0.1, 0.9)
        } else {
            let val = trimmed.parse::<f32>().map_err(|_| "Invalid ratio float")?;
            if !val.is_finite() {
                return Err("Ratio value must be a finite number".to_string());
            }
            val.clamp(0.1, 0.9)
        };
        if !new_ratio.is_finite() {
            return Err("Resulting ratio must be a finite number".to_string());
        }
        self.layout_config.split_ratio = new_ratio;
        self.manage_dirty();
        Ok(format!("main ratio set to {:.2}", new_ratio))
    }

    /// Sets or adjusts the layout stack split ratio (supports absolute 0.50 or relative +/-0.05).
    pub fn set_stack_ratio_arg(&mut self, arg: &str) -> Result<String, String> {
        let trimmed = arg.trim();
        let new_ratio = if trimmed.starts_with('+') || trimmed.starts_with('-') {
            let delta = trimmed.parse::<f32>().map_err(|_| "Invalid ratio delta")?;
            if !delta.is_finite() {
                return Err("Ratio delta must be a finite number".to_string());
            }
            let current = if self.layout_config.stack_split_ratio.is_finite() {
                self.layout_config.stack_split_ratio
            } else {
                0.50
            };
            (current + delta).clamp(0.1, 0.9)
        } else {
            let val = trimmed.parse::<f32>().map_err(|_| "Invalid ratio float")?;
            if !val.is_finite() {
                return Err("Ratio value must be a finite number".to_string());
            }
            val.clamp(0.1, 0.9)
        };
        if !new_ratio.is_finite() {
            return Err("Resulting ratio must be a finite number".to_string());
        }
        self.layout_config.stack_split_ratio = new_ratio;
        self.manage_dirty();
        Ok(format!("stack ratio set to {:.2}", new_ratio))
    }

    /// Sets or adjusts the number of windows in the master layout area (supports +/-1).
    pub fn set_main_count_arg(&mut self, arg: &str) -> Result<String, String> {
        let trimmed = arg.trim();
        let new_count = if trimmed.starts_with('+') || trimmed.starts_with('-') {
            let delta = trimmed.parse::<i32>().map_err(|_| "Invalid count delta")?;
            (self.layout_config.main_count as i32 + delta).max(1) as u32
        } else {
            trimmed.parse::<u32>().map_err(|_| "Invalid count")?.max(1)
        };
        self.layout_config.main_count = new_count;
        self.manage_dirty();
        Ok(format!("main count set to {new_count}"))
    }

    /// Sets the layout main split ratio (clamped to 0.1 .. 0.9).
    pub fn set_main_ratio(&mut self, ratio: f32) -> Result<String, String> {
        if !ratio.is_finite() {
            return Err("Ratio must be a finite number".to_string());
        }
        let clamped = ratio.clamp(0.1, 0.9);
        self.layout_config.split_ratio = clamped;
        self.manage_dirty();
        Ok(format!("main ratio set to {clamped:.2}"))
    }

    /// Sets the layout stack split ratio (clamped to 0.1 .. 0.9).
    pub fn set_stack_ratio(&mut self, ratio: f32) -> Result<String, String> {
        if !ratio.is_finite() {
            return Err("Ratio must be a finite number".to_string());
        }
        let clamped = ratio.clamp(0.1, 0.9);
        self.layout_config.stack_split_ratio = clamped;
        self.manage_dirty();
        Ok(format!("stack ratio set to {clamped:.2}"))
    }

    /// Sets the number of windows in the master layout area.
    pub fn set_main_count(&mut self, count: u32) -> Result<String, String> {
        let c = count.max(1);
        self.layout_config.main_count = c;
        self.manage_dirty();
        Ok(format!("main count set to {c}"))
    }

    /// Unmaps a key binding in the specified mode.
    pub fn unmap_key(&mut self, mode: &str, modifiers: &str, key: &str) -> Result<String, String> {
        let mods = crate::wm::binds::parse_modifiers(modifiers)?;
        let Some(keysym) = crate::wm::binds::resolve_keysym(key, mods) else {
            return Err(format!("Unknown keysym: {key}"));
        };
        let mode_norm = mode.trim();

        self.configured_key_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode_norm) && b.modifiers == mods && b.keysym == keysym)
        });
        self.pending_key_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode_norm) && b.modifiers == mods && b.keysym == keysym)
        });

        let to_remove: Vec<wayland_backend::client::ObjectId> = self
            .key_bindings
            .iter()
            .filter(|(_, b)| {
                b.mode.eq_ignore_ascii_case(mode_norm) && b.modifiers == mods && b.keysym == keysym
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_remove {
            if let Some(b) = self.key_bindings.remove(&id) {
                b.proxy.destroy();
            }
        }
        self.manage_dirty();
        Ok(format!("unmapped [{mode_norm}] {modifiers}+{key}"))
    }

    /// Unmaps a pointer binding in the specified mode.
    pub fn unmap_pointer(
        &mut self,
        mode: &str,
        modifiers: &str,
        button: &str,
    ) -> Result<String, String> {
        let mods = crate::wm::binds::parse_modifiers(modifiers)?;
        let Some(btn_code) = crate::wm::binds::parse_button(button) else {
            return Err(format!("Unknown pointer button: {button}"));
        };
        let mode_norm = mode.trim();

        self.configured_pointer_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode_norm) && b.modifiers == mods && b.button == btn_code)
        });
        self.pending_pointer_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode_norm) && b.modifiers == mods && b.button == btn_code)
        });

        for seat in self.seats.values_mut() {
            let to_remove: Vec<wayland_backend::client::ObjectId> = seat
                .pointer_bindings
                .iter()
                .filter(|(_, b)| {
                    b.mode.eq_ignore_ascii_case(mode_norm)
                        && b.modifiers == mods
                        && b.button == btn_code
                })
                .map(|(id, _)| id.clone())
                .collect();

            for id in to_remove {
                if let Some(b) = seat.pointer_bindings.remove(&id) {
                    b.proxy.destroy();
                }
            }
        }
        self.manage_dirty();
        Ok(format!(
            "unmapped pointer [{mode_norm}] {modifiers}+{button}"
        ))
    }

    /// Lists active window rules, optionally filtered by action.
    pub fn list_rules(&self, filter_action: Option<&str>) -> Result<String, String> {
        let mut lines = Vec::new();

        for r in &self.rules {
            let mut match_desc = Vec::new();
            if let Some(ref id) = r.app_id {
                match_desc.push(format!("-app-id {id}"));
            }
            if let Some(ref t) = r.title {
                match_desc.push(format!("-title {t}"));
            }
            let match_str = if match_desc.is_empty() {
                "*".to_string()
            } else {
                match_desc.join(" ")
            };

            let mut actions = Vec::new();
            if let Some(fl) = r.float {
                if fl {
                    actions.push("float".to_string());
                } else {
                    actions.push("no-float".to_string());
                }
            }
            if let Some(ssd) = r.ssd {
                if ssd {
                    actions.push("ssd".to_string());
                } else {
                    actions.push("csd".to_string());
                }
            }
            if let Some(tags) = r.tags {
                actions.push(format!("tags {tags}"));
            }
            if let Some((w, h)) = r.dimensions {
                actions.push(format!("dimensions {w} {h}"));
            }
            if let Some((x, y)) = r.position {
                actions.push(format!("position {x} {y}"));
            }
            if let Some(fs) = r.fullscreen
                && fs
            {
                actions.push("fullscreen".to_string());
            }
            if let Some(ref out) = r.output {
                actions.push(format!("output {out}"));
            }

            for act in actions {
                if let Some(filter) = filter_action {
                    let act_prefix = act.split_whitespace().next().unwrap_or(&act);
                    if !act_prefix.eq_ignore_ascii_case(filter) {
                        continue;
                    }
                }
                lines.push(format!("{match_str} {act}"));
            }
        }

        if lines.is_empty() {
            Ok("no matching rules".to_string())
        } else {
            Ok(lines.join("\n"))
        }
    }

    /// Deletes a rule matching app_id, title, and action.
    pub fn rule_del(
        &mut self,
        app_id: Option<&str>,
        title: Option<&str>,
        action: &[String],
    ) -> Result<String, String> {
        if action.is_empty() {
            return Err("Rule action cannot be empty".to_string());
        }

        let act = action[0].to_ascii_lowercase();
        let initial_len = self.rules.len();

        self.rules.retain(|r| {
            let app_match = r.app_id.as_deref() == app_id;
            let title_match = r.title.as_deref() == title;
            if !app_match || !title_match {
                return true;
            }
            let action_match = match act.as_str() {
                "float" | "no-float" => r.float.is_some(),
                "ssd" | "csd" | "no-ssd" | "no-border" => r.ssd.is_some(),
                "tags" => r.tags.is_some(),
                "dimensions" => r.dimensions.is_some(),
                "position" => r.position.is_some(),
                "fullscreen" | "no-fullscreen" => r.fullscreen.is_some(),
                "output" => r.output.is_some(),
                _ => false,
            };
            !action_match
        });

        if self.rules.len() < initial_len {
            Ok(format!(
                "rule deleted for app_id={app_id:?} title={title:?} action={action:?}"
            ))
        } else {
            Ok("no matching rule found".to_string())
        }
    }

    /// Handles an incoming IPC command and synchronously applies it to the WM.
    pub fn handle_ipc_command(&mut self, cmd: &IpcCommand) -> Result<String, String> {
        match cmd {
            IpcCommand::Ping => Ok("pong".to_string()),
            IpcCommand::Close => self.close_focused(),
            IpcCommand::ToggleFloat => self.toggle_float_focused(),
            IpcCommand::ToggleFullscreen => self.toggle_fullscreen_focused(),
            IpcCommand::ToggleMonocle => self.toggle_monocle(),
            IpcCommand::Zoom => self.zoom_focused(),
            IpcCommand::FocusView {
                direction,
                skip_floating,
            } => self.focus_view_direction(direction, *skip_floating),
            IpcCommand::FocusOutput(dir) => self.focus_output(dir),
            IpcCommand::SendToOutput {
                direction,
                current_tags,
            } => self.send_to_output(direction, *current_tags),
            IpcCommand::Swap(dir) => self.swap_direction(dir),
            IpcCommand::Snap(edge) => self.snap_focused(edge),
            IpcCommand::SetFocusedTags(mask) => self.set_focused_tags(*mask),
            IpcCommand::ToggleFocusedTags(mask) => self.toggle_focused_tags(*mask),
            IpcCommand::SetViewTags(mask) => self.set_view_tags(*mask),
            IpcCommand::ToggleViewTags(mask) => self.toggle_view_tags(*mask),
            IpcCommand::FocusPreviousTags => self.focus_previous_tags(),
            IpcCommand::SendToPreviousTags => self.send_to_previous_tags(),
            IpcCommand::SpawnTagmask(mask) => self.set_spawn_tagmask(*mask),
            IpcCommand::MainLocation(loc) => self.set_main_location(*loc),
            IpcCommand::DefaultAttachMode(mode) => self.set_attach_mode(*mode),
            IpcCommand::SetCursorWarp(mode) => self.set_cursor_warp(*mode),
            IpcCommand::FocusFollowsCursor(mode) => self.set_focus_follows_cursor(*mode),
            IpcCommand::HideCursorTimeout(ms) => self.set_hide_cursor_timeout(*ms),
            IpcCommand::HideCursorWhenTyping(en) => self.set_hide_cursor_when_typing(*en),
            IpcCommand::ViewPadding(g) => self.set_view_padding(*g),
            IpcCommand::OuterPadding(p) => self.set_outer_padding(*p),
            IpcCommand::BorderWidth(w) => {
                if *w > i32::MAX as u32 {
                    return Err("border width exceeds maximum allowed value".to_string());
                }
                self.border_width = *w;
                self.manage_dirty();
                Ok(format!("border width set to {w}px"))
            }
            IpcCommand::BorderColorFocused(c) => {
                crate::wm::state::parse_hex_color(c)?;
                self.border_color_focused = c.clone();
                self.manage_dirty();
                Ok(format!("focused border color set to {c}"))
            }
            IpcCommand::BorderColorUnfocused(c) => {
                crate::wm::state::parse_hex_color(c)?;
                self.border_color_unfocused = c.clone();
                self.manage_dirty();
                Ok(format!("unfocused border color set to {c}"))
            }
            IpcCommand::BorderColorUrgent(c) => {
                crate::wm::state::parse_hex_color(c)?;
                self.border_color_urgent = c.clone();
                self.manage_dirty();
                Ok(format!("urgent border color set to {c}"))
            }
            IpcCommand::MainRatio(r) => self.set_main_ratio_arg(r),
            IpcCommand::StackRatio(r) => self.set_stack_ratio_arg(r),
            IpcCommand::MainCount(c) => self.set_main_count_arg(c),
            IpcCommand::DeclareMode(mode) => self.declare_mode(mode),
            IpcCommand::EnterMode(mode) => self.enter_mode(mode),
            IpcCommand::MoveWindow { direction, delta } => self.move_window(direction, *delta),
            IpcCommand::ResizeWindow { horizontal, delta } => {
                self.resize_window(*horizontal, *delta)
            }
            IpcCommand::Animation(enabled) => {
                self.anim.enabled = *enabled;
                Ok(format!("animations set to {enabled}"))
            }
            IpcCommand::AnimationDuration(ms) => {
                self.anim.duration = Duration::from_millis(*ms);
                Ok(format!("animation duration set to {ms}ms"))
            }
            IpcCommand::RuleAdd {
                app_id,
                title,
                action,
            } => {
                if action.is_empty() {
                    return Err("Rule action cannot be empty".to_string());
                }
                let mut float = None;
                let mut ssd = None;
                let mut tags = None;
                let mut dimensions = None;
                let mut position = None;
                let mut fullscreen = None;
                let mut output = None;

                let act = action[0].to_ascii_lowercase();
                match act.as_str() {
                    "float" => float = Some(true),
                    "no-float" => float = Some(false),
                    "ssd" => ssd = Some(true),
                    "csd" | "no-ssd" | "no-border" => ssd = Some(false),
                    "fullscreen" => fullscreen = Some(true),
                    "no-fullscreen" => fullscreen = Some(false),
                    "tags" => {
                        if action.len() > 1 {
                            let raw = action[1].trim();
                            let mask = if let Some(hex) =
                                raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X"))
                            {
                                u32::from_str_radix(hex, 16)
                                    .map_err(|_| "Invalid tag mask".to_string())?
                            } else {
                                raw.parse::<u32>()
                                    .map_err(|_| "Invalid tag mask".to_string())?
                            };
                            if mask == 0 {
                                return Err(
                                    "a window rule must specify at least one tag".to_string()
                                );
                            }
                            tags = Some(mask);
                        } else {
                            return Err("Usage: rule-add ... tags <mask>".to_string());
                        }
                    }
                    "dimensions" => {
                        if action.len() > 2 {
                            let w = action[1].parse::<u32>().map_err(|_| "Invalid width")?;
                            let h = action[2].parse::<u32>().map_err(|_| "Invalid height")?;
                            if w > i32::MAX as u32 || h > i32::MAX as u32 {
                                return Err(
                                    "window dimensions exceed maximum allowed value".to_string()
                                );
                            }
                            dimensions = Some((w, h));
                        } else {
                            return Err(
                                "Usage: rule-add ... dimensions <width> <height>".to_string()
                            );
                        }
                    }
                    "position" => {
                        if action.len() > 2 {
                            let x = action[1]
                                .parse::<i32>()
                                .map_err(|_| "Invalid x coordinate")?;
                            let y = action[2]
                                .parse::<i32>()
                                .map_err(|_| "Invalid y coordinate")?;
                            position = Some((x, y));
                        } else {
                            return Err("Usage: rule-add ... position <x> <y>".to_string());
                        }
                    }
                    "output" => {
                        if action.len() > 1 {
                            output = Some(action[1].clone());
                        } else {
                            return Err("Usage: rule-add ... output <name|id>".to_string());
                        }
                    }
                    other => {
                        return Err(format!("Unknown rule action: {other}"));
                    }
                }

                self.rules.push(WindowRule {
                    app_id: app_id.clone(),
                    title: title.clone(),
                    float,
                    ssd,
                    tags,
                    dimensions,
                    position,
                    fullscreen,
                    output,
                });
                Ok(format!(
                    "rule added for app_id={app_id:?} title={title:?} action={action:?}"
                ))
            }
            IpcCommand::RuleDel {
                app_id,
                title,
                action,
            } => self.rule_del(app_id.as_deref(), title.as_deref(), action),
            IpcCommand::ListRules { action } => self.list_rules(action.as_deref()),
            IpcCommand::Map {
                mode,
                modifiers,
                key,
                action,
            } => {
                let mods = crate::wm::binds::parse_modifiers(modifiers)?;
                let Some(keysym) = crate::wm::binds::resolve_keysym(key, mods) else {
                    return Err(format!("Unknown keysym: {key}"));
                };
                let mode_norm = self
                    .modes
                    .iter()
                    .find(|m| m.eq_ignore_ascii_case(mode.trim()))
                    .cloned()
                    .unwrap_or_else(|| mode.trim().to_ascii_lowercase());
                let pending = crate::wm::binds::PendingKeyBinding {
                    mode: mode_norm.clone(),
                    modifiers: mods,
                    keysym,
                    action: action.clone(),
                };
                self.configured_key_bindings.retain(|b| {
                    !(b.mode.eq_ignore_ascii_case(&mode_norm)
                        && b.modifiers == mods
                        && b.keysym == keysym)
                });
                self.configured_key_bindings.push(pending.clone());
                self.pending_key_bindings.retain(|b| {
                    !(b.mode.eq_ignore_ascii_case(&mode_norm)
                        && b.modifiers == mods
                        && b.keysym == keysym)
                });
                self.pending_key_bindings.push(pending);

                let to_remove: Vec<wayland_backend::client::ObjectId> = self
                    .key_bindings
                    .iter()
                    .filter(|(_, b)| {
                        b.mode.eq_ignore_ascii_case(&mode_norm)
                            && b.modifiers == mods
                            && b.keysym == keysym
                    })
                    .map(|(id, _)| id.clone())
                    .collect();

                for id in to_remove {
                    if let Some(b) = self.key_bindings.remove(&id) {
                        b.proxy.destroy();
                    }
                }
                self.manage_dirty();
                Ok(format!(
                    "mapped [{mode_norm}] {modifiers}+{key} -> {action:?}"
                ))
            }
            IpcCommand::Unmap {
                mode,
                modifiers,
                key,
            } => self.unmap_key(mode, modifiers, key),
            IpcCommand::MapPointer {
                mode,
                modifiers,
                button,
                action,
            } => {
                let mods = crate::wm::binds::parse_modifiers(modifiers)?;
                let Some(btn_code) = crate::wm::binds::parse_button(button) else {
                    return Err(format!("Unknown pointer button: {button}"));
                };
                let mode_norm = self
                    .modes
                    .iter()
                    .find(|m| m.eq_ignore_ascii_case(mode.trim()))
                    .cloned()
                    .unwrap_or_else(|| mode.trim().to_ascii_lowercase());
                let ptr_action = crate::wm::seat::PointerAction::from_tokens(action);
                let pending = crate::wm::binds::PendingPointerBinding {
                    mode: mode_norm.clone(),
                    modifiers: mods,
                    button: btn_code,
                    action: ptr_action,
                };
                self.configured_pointer_bindings.retain(|b| {
                    !(b.mode.eq_ignore_ascii_case(&mode_norm)
                        && b.modifiers == mods
                        && b.button == btn_code)
                });
                self.configured_pointer_bindings.push(pending.clone());
                self.pending_pointer_bindings.retain(|b| {
                    !(b.mode.eq_ignore_ascii_case(&mode_norm)
                        && b.modifiers == mods
                        && b.button == btn_code)
                });
                self.pending_pointer_bindings.push(pending);

                for seat in self.seats.values_mut() {
                    let to_remove: Vec<wayland_backend::client::ObjectId> = seat
                        .pointer_bindings
                        .iter()
                        .filter(|(_, b)| {
                            b.mode.eq_ignore_ascii_case(&mode_norm)
                                && b.modifiers == mods
                                && b.button == btn_code
                        })
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in to_remove {
                        if let Some(b) = seat.pointer_bindings.remove(&id) {
                            b.proxy.destroy();
                        }
                    }
                }
                self.manage_dirty();
                Ok(format!(
                    "mapped-pointer [{mode_norm}] {modifiers}+{button} -> {action:?}"
                ))
            }
            IpcCommand::UnmapPointer {
                mode,
                modifiers,
                button,
            } => self.unmap_pointer(mode, modifiers, button),
            IpcCommand::Status { stream: _, format } => {
                if format.as_deref() == Some("waybar") {
                    Ok(self.format_waybar_status())
                } else {
                    Ok(self.format_json_status())
                }
            }
            IpcCommand::Reload => {
                reap_zombies();
                spawn_init_script();
                Ok("reloaded init script".to_string())
            }
            IpcCommand::Spawn(args) => {
                if args.is_empty() {
                    return Err("spawn command requires a program to run".to_string());
                }
                reap_zombies();
                let prog = &args[0];
                let prog_args = &args[1..];
                std::process::Command::new(prog)
                    .args(prog_args)
                    .spawn()
                    .map_err(|e| format!("failed to spawn {prog}: {e}"))?;
                Ok(format!("spawned {prog}"))
            }
            IpcCommand::Exit => {
                self.should_exit = true;
                Ok("exiting".to_string())
            }
        }
    }

    /// Executes a list of action tokens triggered by keyboard or pointer binding.
    pub fn execute_action_tokens(&mut self, action: &[String]) {
        if action.is_empty() {
            return;
        }

        if action[0] == "spawn" {
            if action.len() > 1 {
                reap_zombies();
                let cmd = &action[1];
                let args = &action[2..];
                tracing::info!("Binding spawn: {cmd} {args:?}");
                let _ = std::process::Command::new(cmd).args(args).spawn();
            }
            return;
        }

        match crate::ipc::parse_cli_args(action) {
            Ok(cmd) => {
                if let Err(e) = self.handle_ipc_command(&cmd) {
                    tracing::warn!("Action execution error for {action:?}: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Unknown action tokens {action:?}: {e}");
            }
        }
    }

    /// Handles a keybinding press event triggered by river-xkb-bindings.
    pub fn handle_key_binding_pressed(&mut self, binding_id: &wayland_backend::client::ObjectId) {
        if self.cursor_hide_when_typing {
            self.hide_cursor();
        }
        let action_opt = self.key_bindings.get(binding_id).map(|b| b.action.clone());
        let Some(action) = action_opt else {
            return;
        };
        self.execute_action_tokens(&action);
    }
}

#[cfg(test)]
#[path = "actions_test.rs"]
mod tests;
