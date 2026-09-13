//! Window manager actions and IPC command dispatch.

use std::time::Duration;

use crate::ipc::IpcCommand;
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::wm::state::AppState;
use crate::wm::state::AttachMode;
use crate::wm::state::WindowRule;
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
                seat.focused = Some(proxy.clone());
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

        let default_usable_area = crate::layout::Rect::new(0, 30, 1280, 770);

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            let usable = w
                .output
                .as_ref()
                .and_then(|out_id| self.outputs.get(out_id))
                .map(|o| o.usable_area)
                .unwrap_or(default_usable_area);

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
            w.width = new_w.max(100);
            w.height = new_h.max(100);
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

    /// Bumps the focused window to the master position in the layout stack.
    /// If the view on the top of the stack is already focused, bumps the second view to top (matching river-classic).
    pub fn zoom_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let tag_state = self.tag_state;
        let visible_tiled: Vec<usize> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                !w.closed && !w.floating && !w.fullscreen && tag_state.is_view_visible(w.tags)
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

        let target_idx = if pos == 0 {
            // Already at top: bump second view to top
            visible_tiled[1]
        } else {
            // Bump focused view to top
            visible_tiled[pos]
        };

        let win = self.windows.remove(target_idx);
        let win_id = win.id;
        self.windows.insert(0, win);
        self.manage_dirty();
        Ok(format!("zoomed window {win_id}"))
    }

    /// Finds the target window in the given direction.
    pub fn find_target_window(&self, dir: Direction, skip_floating: bool) -> Option<u32> {
        let tag_state = self.tag_state;
        let visible: Vec<&crate::wm::state::WindowItem> = self
            .windows
            .iter()
            .filter(|w| {
                !w.closed && (!skip_floating || !w.floating) && tag_state.is_view_visible(w.tags)
            })
            .collect();

        if visible.len() <= 1 {
            return None;
        }

        let focused_id = self.focused_window_id()?;
        let current_win = visible.iter().find(|w| w.id == focused_id)?;

        match dir {
            Direction::Next => {
                let idx = visible.iter().position(|w| w.id == focused_id)?;
                let next_idx = (idx + 1) % visible.len();
                Some(visible[next_idx].id)
            }
            Direction::Previous => {
                let idx = visible.iter().position(|w| w.id == focused_id)?;
                let prev_idx = (idx + visible.len() - 1) % visible.len();
                Some(visible[prev_idx].id)
            }
            Direction::Left | Direction::Right | Direction::Up | Direction::Down => {
                let fx = current_win.x + current_win.width as i32 / 2;
                let fy = current_win.y + current_win.height as i32 / 2;

                let mut best_id = None;
                let mut best_dist = i64::MAX;

                for w in &visible {
                    if w.id == focused_id {
                        continue;
                    }
                    let cx = w.x + w.width as i32 / 2;
                    let cy = w.y + w.height as i32 / 2;
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
                    for w in &visible {
                        if w.id == focused_id {
                            continue;
                        }
                        let cx = w.x + w.width as i32 / 2;
                        let cy = w.y + w.height as i32 / 2;
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

        let tag_state = self.tag_state;
        let dest_win = self.windows.iter().find(|w| {
            !w.closed && w.output == Some(out_id.clone()) && tag_state.is_view_visible(w.tags)
        });
        if let Some(w) = dest_win {
            let proxy = w.proxy.clone();
            for seat in self.seats.values_mut() {
                seat.focused = Some(proxy.clone());
            }
        }

        // Warp pointer if cursor_warp is enabled
        if self.cursor_warp != crate::wm::CursorWarp::Disabled
            && let Some(out) = self.outputs.get(&out_id)
        {
            let (cx, cy) = match (self.cursor_warp, dest_win) {
                (crate::wm::CursorWarp::OnFocusChange, Some(w)) => {
                    (w.x + w.width as i32 / 2, w.y + w.height as i32 / 2)
                }
                _ => (out.x + out.width as i32 / 2, out.y + out.height as i32 / 2),
            };
            for seat in self.seats.values() {
                seat.proxy.pointer_warp(cx, cy);
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

        let dest_tags = self.tag_state.focused;

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.output = Some(out_id.clone());
            if current_tags {
                w.tags = dest_tags;
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
        let tag_state = self.tag_state;
        let visible_count = self
            .windows
            .iter()
            .filter(|w| {
                !w.closed && (!skip_floating || !w.floating) && tag_state.is_view_visible(w.tags)
            })
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
            let (cx, cy) = (win.x + win.width as i32 / 2, win.y + win.height as i32 / 2);
            let out_id = win.output.clone();
            for seat in self.seats.values_mut() {
                seat.focused = Some(proxy.clone());
                if self.cursor_warp == crate::wm::CursorWarp::OnFocusChange {
                    seat.proxy.pointer_warp(cx, cy);
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
        if self.tag_state.focused != mask {
            let old_mask = self.tag_state.focused;
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
        self.tag_state.set_focused_tags(mask);
        self.manage_dirty();
        Ok(format!("focused tags set to {mask}"))
    }

    /// Toggles tags back to the previous tag setup.
    pub fn focus_previous_tags(&mut self) -> Result<String, String> {
        let prev = self.previous_focused_tags;
        self.set_focused_tags(prev)
    }

    /// Sends the focused window to the previous tag setup.
    pub fn send_to_previous_tags(&mut self) -> Result<String, String> {
        let prev = self.previous_focused_tags;
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
        self.layout_config.gaps = padding;
        self.manage_dirty();
        Ok(format!("view padding set to {padding}px"))
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

    /// Toggles the focused tags mask on the WM.
    pub fn toggle_focused_tags(&mut self, mask: TagMask) -> Result<String, String> {
        let old_mask = self.tag_state.focused;
        let new_mask = self.tag_state.focused ^ mask;
        if new_mask != TAG_NONE && new_mask != old_mask {
            let dir = if new_mask > old_mask {
                crate::animation::SlideDirection::Right
            } else {
                crate::animation::SlideDirection::Left
            };
            self.tag_slide_dir = Some(dir);
            self.tag_anim_old_mask = old_mask;
            self.anim.start();
        }
        self.tag_state.toggle_focused_tags(mask);
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
        if !self.modes.iter().any(|m| m.eq_ignore_ascii_case(name)) {
            self.modes.push(name.to_string());
        }
        Ok(format!("declared mode {name}"))
    }

    /// Enters a declared modal keybinding mode.
    pub fn enter_mode(&mut self, mode: &str) -> Result<String, String> {
        let name = mode.trim();
        if !self.modes.iter().any(|m| m.eq_ignore_ascii_case(name)) {
            return Err(format!("unknown mode '{name}', declare it first"));
        }
        if self.active_mode != name {
            self.active_mode = name.to_string();
            self.mode_dirty = true;
            self.manage_dirty();
        }
        Ok(format!("entered mode {name}"))
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
                    w.width = (w.width as i32 + delta).max(100) as u32;
                } else {
                    w.height = (w.height as i32 + delta).max(100) as u32;
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
            (self.layout_config.split_ratio + delta).clamp(0.1, 0.9)
        } else {
            trimmed
                .parse::<f32>()
                .map_err(|_| "Invalid ratio float")?
                .clamp(0.1, 0.9)
        };
        self.layout_config.split_ratio = new_ratio;
        self.manage_dirty();
        Ok(format!("main ratio set to {:.2}", new_ratio))
    }

    /// Sets or adjusts the layout stack split ratio (supports absolute 0.50 or relative +/-0.05).
    pub fn set_stack_ratio_arg(&mut self, arg: &str) -> Result<String, String> {
        let trimmed = arg.trim();
        let new_ratio = if trimmed.starts_with('+') || trimmed.starts_with('-') {
            let delta = trimmed.parse::<f32>().map_err(|_| "Invalid ratio delta")?;
            (self.layout_config.stack_split_ratio + delta).clamp(0.1, 0.9)
        } else {
            trimmed
                .parse::<f32>()
                .map_err(|_| "Invalid ratio float")?
                .clamp(0.1, 0.9)
        };
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
        let clamped = ratio.clamp(0.1, 0.9);
        self.layout_config.split_ratio = clamped;
        self.manage_dirty();
        Ok(format!("main ratio set to {clamped:.2}"))
    }

    /// Sets the layout stack split ratio (clamped to 0.1 .. 0.9).
    pub fn set_stack_ratio(&mut self, ratio: f32) -> Result<String, String> {
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
        let mods = crate::wm::binds::parse_modifiers(modifiers);
        let Some(keysym) = crate::wm::binds::resolve_keysym(key, mods) else {
            return Err(format!("Unknown keysym: {key}"));
        };

        self.pending_key_bindings
            .retain(|b| !(b.mode == mode && b.modifiers == mods && b.keysym == keysym));

        let to_remove: Vec<wayland_backend::client::ObjectId> = self
            .key_bindings
            .iter()
            .filter(|(_, b)| b.mode == mode && b.modifiers == mods && b.keysym == keysym)
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_remove {
            if let Some(b) = self.key_bindings.remove(&id) {
                b.proxy.destroy();
            }
        }
        self.manage_dirty();
        Ok(format!("unmapped [{mode}] {modifiers}+{key}"))
    }

    /// Unmaps a pointer binding in the specified mode.
    pub fn unmap_pointer(
        &mut self,
        mode: &str,
        modifiers: &str,
        button: &str,
    ) -> Result<String, String> {
        let mods = crate::wm::binds::parse_modifiers(modifiers);
        let Some(btn_code) = crate::wm::binds::parse_button(button) else {
            return Err(format!("Unknown pointer button: {button}"));
        };

        self.pending_pointer_bindings
            .retain(|b| !(b.mode == mode && b.modifiers == mods && b.button == btn_code));

        for seat in self.seats.values_mut() {
            let to_remove: Vec<wayland_backend::client::ObjectId> = seat
                .pointer_bindings
                .iter()
                .filter(|(_, b)| b.mode == mode && b.modifiers == mods && b.button == btn_code)
                .map(|(id, _)| id.clone())
                .collect();

            for id in to_remove {
                if let Some(b) = seat.pointer_bindings.remove(&id) {
                    b.proxy.destroy();
                }
            }
        }
        self.manage_dirty();
        Ok(format!("unmapped pointer [{mode}] {modifiers}+{button}"))
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
            IpcCommand::ViewPadding(g) => self.set_view_padding(*g),
            IpcCommand::BorderWidth(w) => {
                self.border_width = *w;
                self.manage_dirty();
                Ok(format!("border width set to {w}px"))
            }
            IpcCommand::BorderColorFocused(c) => {
                self.border_color_focused = c.clone();
                self.manage_dirty();
                Ok(format!("focused border color set to {c}"))
            }
            IpcCommand::BorderColorUnfocused(c) => {
                self.border_color_unfocused = c.clone();
                self.manage_dirty();
                Ok(format!("unfocused border color set to {c}"))
            }
            IpcCommand::BorderColorUrgent(c) => {
                self.border_color_urgent = c.clone();
                self.manage_dirty();
                Ok(format!("urgent border color set to {c}"))
            }
            IpcCommand::MainRatio(r) => self.set_main_ratio_arg(r),
            IpcCommand::StackRatio(r) => self.set_stack_ratio_arg(r),
            IpcCommand::MainCount(c) => self.set_main_count_arg(c),
            IpcCommand::DeclareMode(mode) => self.declare_mode(mode),
            IpcCommand::EnterMode(mode) => self.enter_mode(mode),
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
                            let tag_num =
                                action[1].parse::<u32>().map_err(|_| "Invalid tag number")?;
                            let mask = if (1..=32).contains(&tag_num) {
                                1 << (tag_num - 1)
                            } else {
                                tag_num
                            };
                            tags = Some(mask);
                        } else {
                            return Err("Usage: rule-add ... tags <tag>".to_string());
                        }
                    }
                    "dimensions" => {
                        if action.len() > 2 {
                            let w = action[1].parse::<u32>().map_err(|_| "Invalid width")?;
                            let h = action[2].parse::<u32>().map_err(|_| "Invalid height")?;
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
                let mods = crate::wm::binds::parse_modifiers(modifiers);
                let Some(keysym) = crate::wm::binds::resolve_keysym(key, mods) else {
                    return Err(format!("Unknown keysym: {key}"));
                };
                self.pending_key_bindings
                    .push(crate::wm::binds::PendingKeyBinding {
                        mode: mode.clone(),
                        modifiers: mods,
                        keysym,
                        action: action.clone(),
                    });
                self.manage_dirty();
                Ok(format!("mapped [{mode}] {modifiers}+{key} -> {action:?}"))
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
                let mods = crate::wm::binds::parse_modifiers(modifiers);
                let Some(btn_code) = crate::wm::binds::parse_button(button) else {
                    return Err(format!("Unknown pointer button: {button}"));
                };
                let ptr_action = crate::wm::seat::PointerAction::from_tokens(action);
                self.pending_pointer_bindings
                    .push(crate::wm::binds::PendingPointerBinding {
                        mode: mode.clone(),
                        modifiers: mods,
                        button: btn_code,
                        action: ptr_action,
                    });
                self.manage_dirty();
                Ok(format!(
                    "mapped-pointer [{mode}] {modifiers}+{button} -> {action:?}"
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
                spawn_init_script();
                Ok("reloaded init script".to_string())
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

        match action[0].as_str() {
            "spawn" => {
                if action.len() > 1 {
                    let cmd = &action[1];
                    let args = &action[2..];
                    tracing::info!("Binding spawn: {cmd} {args:?}");
                    let _ = std::process::Command::new(cmd).args(args).spawn();
                }
            }
            "close" => {
                let _ = self.close_focused();
            }
            "toggle-float" | "toggle-floating" => {
                tracing::info!("Binding toggle-float pressed");
                let _ = self.toggle_float_focused();
            }
            "toggle-fullscreen" | "fullscreen" => {
                tracing::info!("Binding toggle-fullscreen pressed");
                let _ = self.toggle_fullscreen_focused();
            }
            "focus-previous-tags" => {
                let _ = self.focus_previous_tags();
            }
            "send-to-previous-tags" => {
                let _ = self.send_to_previous_tags();
            }
            "declare-mode" => {
                if action.len() > 1 {
                    let _ = self.declare_mode(&action[1]);
                }
            }
            "enter-mode" => {
                if action.len() > 1 {
                    let _ = self.enter_mode(&action[1]);
                }
            }
            "resize" => {
                let horizontal = if action.len() > 1 {
                    !matches!(action[1].as_str(), "vertical" | "v" | "height")
                } else {
                    true
                };
                let delta = if action.len() > 2 {
                    action[2].parse::<i32>().unwrap_or(20)
                } else {
                    20
                };
                let _ = self.resize_window(horizontal, delta);
            }
            "main-ratio" => {
                if action.len() > 1 {
                    let _ = self.set_main_ratio_arg(&action[1]);
                }
            }
            "stack-ratio" => {
                if action.len() > 1 {
                    let _ = self.set_stack_ratio_arg(&action[1]);
                }
            }
            "main-count" => {
                if action.len() > 1 {
                    let _ = self.set_main_count_arg(&action[1]);
                }
            }
            "main-location" => {
                if action.len() > 1 {
                    let loc_str = &action[1];
                    let loc = match loc_str.to_ascii_lowercase().as_str() {
                        "top" => crate::layout::MainLocation::Top,
                        "bottom" => crate::layout::MainLocation::Bottom,
                        "left" => crate::layout::MainLocation::Left,
                        "right" => crate::layout::MainLocation::Right,
                        _ => crate::layout::MainLocation::Left,
                    };
                    let _ = self.set_main_location(loc);
                }
            }
            "view-padding" => {
                if action.len() > 1
                    && let Ok(p) = action[1].parse::<u32>()
                {
                    let _ = self.set_view_padding(p);
                }
            }
            "default-attach-mode" => {
                if action.len() > 1 {
                    let raw = action[1..].join(" ");
                    if let Ok(mode) = AttachMode::parse(&raw) {
                        let _ = self.set_attach_mode(mode);
                    }
                }
            }
            "unmap" => {
                if action.len() >= 4 {
                    let _ = self.unmap_key(&action[1], &action[2], &action[3]);
                }
            }
            "unmap-pointer" => {
                if action.len() >= 4 {
                    let _ = self.unmap_pointer(&action[1], &action[2], &action[3]);
                }
            }
            "set-cursor-warp" => {
                if action.len() > 1
                    && let Ok(mode) = crate::wm::CursorWarp::parse(&action[1])
                {
                    let _ = self.set_cursor_warp(mode);
                }
            }
            "focus-follows-cursor" => {
                if action.len() > 1
                    && let Ok(mode) = crate::wm::FocusFollowsCursor::parse(&action[1])
                {
                    let _ = self.set_focus_follows_cursor(mode);
                }
            }
            "border-width" => {
                if action.len() > 1
                    && let Ok(w) = action[1].parse::<u32>()
                {
                    self.border_width = w;
                    self.manage_dirty();
                }
            }
            "border-color-focused" => {
                if action.len() > 1 {
                    self.border_color_focused = action[1].clone();
                    self.manage_dirty();
                }
            }
            "border-color-unfocused" => {
                if action.len() > 1 {
                    self.border_color_unfocused = action[1].clone();
                    self.manage_dirty();
                }
            }
            "border-color-urgent" => {
                if action.len() > 1 {
                    self.border_color_urgent = action[1].clone();
                    self.manage_dirty();
                }
            }
            "animation" => {
                if action.len() > 1 {
                    self.anim.enabled = matches!(action[1].as_str(), "true" | "1" | "on");
                }
            }
            "animation-duration" => {
                if action.len() > 1
                    && let Ok(ms) = action[1].parse::<u64>()
                {
                    self.anim.duration = Duration::from_millis(ms);
                }
            }
            "zoom" => {
                let _ = self.zoom_focused();
            }
            "focus-view" => {
                let mut skip_floating = false;
                let mut dir = "next";
                for tok in &action[1..] {
                    if tok == "-skip-floating" {
                        skip_floating = true;
                    } else {
                        dir = tok.as_str();
                    }
                }
                let _ = self.focus_view_direction(dir, skip_floating);
            }
            "focus-output" => {
                let dir = action.get(1).map(|s| s.as_str()).unwrap_or("next");
                let _ = self.focus_output(dir);
            }
            "send-to-output" => {
                let mut current_tags = false;
                let mut dir = "next";
                for tok in &action[1..] {
                    if tok == "-current-tags" {
                        current_tags = true;
                    } else {
                        dir = tok.as_str();
                    }
                }
                let _ = self.send_to_output(dir, current_tags);
            }
            "swap" => {
                let dir = action.get(1).map(|s| s.as_str()).unwrap_or("next");
                let _ = self.swap_direction(dir);
            }
            "snap" => {
                let edge = action.get(1).map(|s| s.as_str()).unwrap_or("left");
                let _ = self.snap_focused(edge);
            }
            "set-focused-tags" => {
                if action.len() > 1
                    && let Ok(mask) = action[1].parse::<u32>()
                {
                    let _ = self.set_focused_tags(mask);
                }
            }
            "set-view-tags" => {
                if action.len() > 1
                    && let Ok(mask) = action[1].parse::<u32>()
                {
                    let _ = self.set_view_tags(mask);
                }
            }
            "toggle-focused-tags" => {
                if action.len() > 1
                    && let Ok(mask) = action[1].parse::<u32>()
                {
                    let _ = self.toggle_focused_tags(mask);
                }
            }
            "toggle-view-tags" => {
                if action.len() > 1
                    && let Ok(mask) = action[1].parse::<u32>()
                {
                    let _ = self.toggle_view_tags(mask);
                }
            }
            "spawn-tagmask" => {
                if action.len() > 1
                    && let Ok(mask) = action[1].parse::<u32>()
                {
                    let _ = self.set_spawn_tagmask(mask);
                }
            }
            "exit" => {
                self.should_exit = true;
            }
            "reload" => {
                spawn_init_script();
            }
            other => {
                tracing::warn!("Unknown action: {other}");
            }
        }
    }

    /// Handles a keybinding press event triggered by river-xkb-bindings.
    pub fn handle_key_binding_pressed(&mut self, binding_id: &wayland_backend::client::ObjectId) {
        let action_opt = self.key_bindings.get(binding_id).map(|b| b.action.clone());
        let Some(action) = action_opt else {
            return;
        };
        self.execute_action_tokens(&action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_ipc_gaps() {
        let mut state = AppState::new();
        let cmd = IpcCommand::ViewPadding(8);
        let res = state.handle_ipc_command(&cmd);
        assert!(res.is_ok());
        assert_eq!(state.layout_config.gaps, 8);
    }

    #[test]
    fn test_app_state_ipc_rules() {
        let mut state = AppState::new();
        let cmd = IpcCommand::RuleAdd {
            app_id: Some("mpv".into()),
            title: None,
            action: vec!["float".into()],
        };
        let res = state.handle_ipc_command(&cmd);
        assert!(res.is_ok());
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].float, Some(true));
        assert_eq!(state.rules[0].ssd, None);

        let csd_cmd = IpcCommand::RuleAdd {
            app_id: Some("foot".into()),
            title: None,
            action: vec!["csd".into()],
        };
        let res_csd = state.handle_ipc_command(&csd_cmd);
        assert!(res_csd.is_ok());
        assert_eq!(state.rules[1].ssd, Some(false));

        let dim_cmd = IpcCommand::RuleAdd {
            app_id: Some("imv".into()),
            title: None,
            action: vec!["dimensions".into(), "960".into(), "540".into()],
        };
        assert!(state.handle_ipc_command(&dim_cmd).is_ok());
        assert_eq!(state.rules[2].dimensions, Some((960, 540)));

        let tag_cmd = IpcCommand::RuleAdd {
            app_id: Some("firefox".into()),
            title: None,
            action: vec!["tags".into(), "2".into()],
        };
        assert!(state.handle_ipc_command(&tag_cmd).is_ok());
        assert_eq!(state.rules[3].tags, Some(2));

        let pos_cmd = IpcCommand::RuleAdd {
            app_id: Some("calc".into()),
            title: None,
            action: vec!["position".into(), "100".into(), "200".into()],
        };
        assert!(state.handle_ipc_command(&pos_cmd).is_ok());
        assert_eq!(state.rules[4].position, Some((100, 200)));

        let fs_cmd = IpcCommand::RuleAdd {
            app_id: Some("gamescope".into()),
            title: None,
            action: vec!["fullscreen".into()],
        };
        assert!(state.handle_ipc_command(&fs_cmd).is_ok());
        assert_eq!(state.rules[5].fullscreen, Some(true));

        let out_cmd = IpcCommand::RuleAdd {
            app_id: Some("wechat".into()),
            title: None,
            action: vec!["output".into(), "DP-1".into()],
        };
        assert!(state.handle_ipc_command(&out_cmd).is_ok());
        assert_eq!(state.rules[6].output, Some("DP-1".into()));

        // Test list-rules
        let all_rules = state.list_rules(None).unwrap();
        assert!(all_rules.contains("-app-id mpv float"));
        assert!(all_rules.contains("-app-id calc position 100 200"));
        assert!(all_rules.contains("-app-id gamescope fullscreen"));

        let float_rules = state.list_rules(Some("float")).unwrap();
        assert!(float_rules.contains("-app-id mpv float"));
        assert!(!float_rules.contains("position"));

        // Test rule-del
        let del_cmd = IpcCommand::RuleDel {
            app_id: Some("mpv".into()),
            title: None,
            action: vec!["float".into()],
        };
        assert!(state.handle_ipc_command(&del_cmd).is_ok());
        assert_eq!(state.rules.len(), 6);

        let del_not_found = IpcCommand::RuleDel {
            app_id: Some("nonexistent".into()),
            title: None,
            action: vec!["float".into()],
        };
        assert_eq!(
            state.handle_ipc_command(&del_not_found).unwrap(),
            "no matching rule found"
        );
    }

    #[test]
    fn test_app_state_ipc_tags() {
        let mut state = AppState::new();
        assert_eq!(state.tag_state.focused, 1);

        state
            .handle_ipc_command(&IpcCommand::SetFocusedTags(5))
            .unwrap();
        assert_eq!(state.tag_state.focused, 5);

        state
            .handle_ipc_command(&IpcCommand::ToggleFocusedTags(2))
            .unwrap();
        assert_eq!(state.tag_state.focused, 7);

        state
            .handle_ipc_command(&IpcCommand::SpawnTagmask(511))
            .unwrap();
        assert_eq!(state.spawn_tagmask, 511);
    }

    #[test]
    fn test_app_state_ipc_layout_and_animation() {
        let mut state = AppState::new();
        state
            .handle_ipc_command(&IpcCommand::MainRatio("0.65".into()))
            .unwrap();
        assert!((state.layout_config.split_ratio - 0.65).abs() < f32::EPSILON);

        state
            .handle_ipc_command(&IpcCommand::StackRatio("0.70".into()))
            .unwrap();
        assert!((state.layout_config.stack_split_ratio - 0.70).abs() < f32::EPSILON);

        state
            .handle_ipc_command(&IpcCommand::MainCount("2".into()))
            .unwrap();
        assert_eq!(state.layout_config.main_count, 2);

        state
            .handle_ipc_command(&IpcCommand::Animation(false))
            .unwrap();
        assert!(!state.anim.enabled);

        state
            .handle_ipc_command(&IpcCommand::AnimationDuration(200))
            .unwrap();
        assert_eq!(state.anim.duration, Duration::from_millis(200));
    }

    #[test]
    fn test_app_state_ipc_map() {
        let mut state = AppState::new();
        let map_cmd = IpcCommand::Map {
            mode: "normal".into(),
            modifiers: "Super".into(),
            key: "Return".into(),
            action: vec!["spawn".into(), "foot".into()],
        };
        let res = state.handle_ipc_command(&map_cmd);
        assert!(res.is_ok());
        assert_eq!(state.pending_key_bindings.len(), 1);

        let unmap_cmd = IpcCommand::Unmap {
            mode: "normal".into(),
            modifiers: "Super".into(),
            key: "Return".into(),
        };
        let res_unmap = state.handle_ipc_command(&unmap_cmd);
        assert!(res_unmap.is_ok());
        assert_eq!(state.pending_key_bindings.len(), 0);
    }

    #[test]
    fn test_app_state_ipc_map_pointer() {
        let mut state = AppState::new();
        let cmd = IpcCommand::MapPointer {
            mode: "normal".into(),
            modifiers: "Super".into(),
            button: "BTN_LEFT".into(),
            action: vec!["move-view".into()],
        };
        assert!(state.handle_ipc_command(&cmd).is_ok());
        assert_eq!(state.pending_pointer_bindings.len(), 1);
        assert_eq!(state.pending_pointer_bindings[0].button, 0x110);

        let unmap_ptr_cmd = IpcCommand::UnmapPointer {
            mode: "normal".into(),
            modifiers: "Super".into(),
            button: "BTN_LEFT".into(),
        };
        let res_unmap_ptr = state.handle_ipc_command(&unmap_ptr_cmd);
        assert!(res_unmap_ptr.is_ok());
        assert_eq!(state.pending_pointer_bindings.len(), 0);
    }

    #[test]
    fn test_app_state_status_formatting() {
        let state = AppState::new();
        let json = state.format_json_status();
        assert!(json.contains("\"focused_tags\":1"));
        assert!(json.contains("\"layout\":\"master-stack\""));

        let waybar = state.format_waybar_status();
        assert!(waybar.contains("\"text\":\"1\""));
    }

    #[test]
    fn test_app_state_modal_modes_and_relative_ratio() {
        let mut state = AppState::new();
        assert_eq!(state.active_mode, "normal");

        state.declare_mode("resize").unwrap();
        assert!(state.modes.contains(&"resize".to_string()));

        state.enter_mode("resize").unwrap();
        assert_eq!(state.active_mode, "resize");

        state.enter_mode("normal").unwrap();
        assert_eq!(state.active_mode, "normal");

        // Relative ratio adjustment
        let initial_ratio = state.layout_config.split_ratio;
        state.set_main_ratio_arg("+0.05").unwrap();
        assert!((state.layout_config.split_ratio - (initial_ratio + 0.05)).abs() < 1e-4);

        state.set_main_ratio_arg("-0.10").unwrap();
        assert!((state.layout_config.split_ratio - (initial_ratio - 0.05)).abs() < 1e-4);

        // Stack ratio adjustment
        let initial_stack_ratio = state.layout_config.stack_split_ratio;
        state.set_stack_ratio_arg("+0.05").unwrap();
        assert!(
            (state.layout_config.stack_split_ratio - (initial_stack_ratio + 0.05)).abs() < 1e-4
        );

        state.set_stack_ratio_arg("-0.10").unwrap();
        assert!(
            (state.layout_config.stack_split_ratio - (initial_stack_ratio - 0.05)).abs() < 1e-4
        );

        state.set_stack_ratio(0.85).unwrap();
        assert!((state.layout_config.stack_split_ratio - 0.85).abs() < f32::EPSILON);

        // Clamping test
        state.set_stack_ratio(1.5).unwrap();
        assert!((state.layout_config.stack_split_ratio - 0.9).abs() < f32::EPSILON);
        state.set_stack_ratio(-0.5).unwrap();
        assert!((state.layout_config.stack_split_ratio - 0.1).abs() < f32::EPSILON);

        // Action tokens test
        state.execute_action_tokens(&["main-ratio".into(), "0.60".into()]);
        assert!((state.layout_config.split_ratio - 0.60).abs() < 1e-4);
        state.execute_action_tokens(&["stack-ratio".into(), "0.60".into()]);
        assert!((state.layout_config.stack_split_ratio - 0.60).abs() < 1e-4);
        state.execute_action_tokens(&["stack-ratio".into(), "+0.10".into()]);
        assert!((state.layout_config.stack_split_ratio - 0.70).abs() < 1e-4);
        state.execute_action_tokens(&["view-padding".into(), "12".into()]);
        assert_eq!(state.layout_config.gaps, 12);

        // Attach mode test
        state.execute_action_tokens(&["default-attach-mode".into(), "bottom".into()]);
        assert_eq!(state.attach_mode, AttachMode::Bottom);
        state.execute_action_tokens(&["default-attach-mode".into(), "after".into(), "3".into()]);
        assert_eq!(state.attach_mode, AttachMode::After(3));

        // Cursor warp and focus-follows-cursor tests
        state.execute_action_tokens(&["set-cursor-warp".into(), "on-output-change".into()]);
        assert_eq!(state.cursor_warp, crate::wm::CursorWarp::OnOutputChange);
        state.execute_action_tokens(&["set-cursor-warp".into(), "disabled".into()]);
        assert_eq!(state.cursor_warp, crate::wm::CursorWarp::Disabled);

        state.execute_action_tokens(&["focus-follows-cursor".into(), "disabled".into()]);
        assert_eq!(
            state.focus_follows_cursor,
            crate::wm::FocusFollowsCursor::Disabled
        );
        state.execute_action_tokens(&["focus-follows-cursor".into(), "always".into()]);
        assert_eq!(
            state.focus_follows_cursor,
            crate::wm::FocusFollowsCursor::Always
        );

        // Relative count adjustment
        assert_eq!(state.layout_config.main_count, 1);
        state.set_main_count_arg("+1").unwrap();
        assert_eq!(state.layout_config.main_count, 2);
        state.set_main_count_arg("-1").unwrap();
        assert_eq!(state.layout_config.main_count, 1);
    }

    #[test]
    fn test_workspace_slide_direction() {
        let mut state = AppState::new();
        assert_eq!(state.tag_state.focused, 1);
        assert!(state.tag_slide_dir.is_none());

        // Tag 1 -> Tag 2 (higher index) => SlideDirection::Right
        state.set_focused_tags(2).unwrap();
        assert_eq!(
            state.tag_slide_dir,
            Some(crate::animation::SlideDirection::Right)
        );
        assert_eq!(state.tag_anim_old_mask, 1);

        // Tag 2 -> Tag 1 (lower index) => SlideDirection::Left
        state.set_focused_tags(1).unwrap();
        assert_eq!(
            state.tag_slide_dir,
            Some(crate::animation::SlideDirection::Left)
        );
        assert_eq!(state.tag_anim_old_mask, 2);
    }

    #[test]
    fn test_app_state_empty_actions() {
        let mut state = AppState::new();
        assert_eq!(state.close_focused().unwrap(), "no view focused to close");
        assert!(state.toggle_float_focused().is_err());
        assert!(state.zoom_focused().is_err());
        assert_eq!(state.focus_view(true).unwrap(), "no visible windows");
        assert!(state.snap_focused("left").is_err());
        assert_eq!(
            state.focus_view_direction("next", true).unwrap(),
            "no visible windows"
        );

        state.execute_action_tokens(&["snap".into(), "left".into()]);
        state.execute_action_tokens(&["focus-view".into(), "-skip-floating".into(), "next".into()]);
        state.execute_action_tokens(&[
            "send-to-output".into(),
            "-current-tags".into(),
            "right".into(),
        ]);

        // Multi-output on empty state
        assert_eq!(
            state
                .handle_ipc_command(&IpcCommand::FocusOutput("next".into()))
                .unwrap(),
            "no destination output found"
        );
        assert_eq!(
            state
                .handle_ipc_command(&IpcCommand::SendToOutput {
                    direction: "next".into(),
                    current_tags: false,
                })
                .unwrap(),
            "no destination output found"
        );
    }

    #[test]
    fn test_pick_adjacent_output() {
        // Two side-by-side monitors:
        // Output A: 0, 0, 1920, 1080
        // Output B: 1920, 0, 1920, 1080
        let a = "A";
        let b = "B";
        let outputs = vec![(&a, 0, 0, 1920, 1080), (&b, 1920, 0, 1920, 1080)];

        // From Output A (idx 0):
        assert_eq!(pick_adjacent_output(&outputs, 0, "next"), Some("B"));
        assert_eq!(pick_adjacent_output(&outputs, 0, "right"), Some("B"));
        assert_eq!(pick_adjacent_output(&outputs, 0, "left"), None);
        assert_eq!(pick_adjacent_output(&outputs, 0, "up"), None);
        assert_eq!(pick_adjacent_output(&outputs, 0, "down"), None);

        // From Output B (idx 1):
        assert_eq!(pick_adjacent_output(&outputs, 1, "next"), Some("A"));
        assert_eq!(pick_adjacent_output(&outputs, 1, "previous"), Some("A"));
        assert_eq!(pick_adjacent_output(&outputs, 1, "left"), Some("A"));
        assert_eq!(pick_adjacent_output(&outputs, 1, "right"), None);

        // Two stacked monitors:
        // Top: 0, 0, 1920, 1080
        // Bottom: 0, 1080, 1920, 1080
        let top = "Top";
        let bottom = "Bottom";
        let stacked = vec![(&top, 0, 0, 1920, 1080), (&bottom, 0, 1080, 1920, 1080)];
        assert_eq!(pick_adjacent_output(&stacked, 0, "down"), Some("Bottom"));
        assert_eq!(pick_adjacent_output(&stacked, 0, "up"), None);
        assert_eq!(pick_adjacent_output(&stacked, 1, "up"), Some("Top"));
        assert_eq!(pick_adjacent_output(&stacked, 1, "down"), None);
    }
}
