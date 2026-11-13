//! Window manager actions and IPC command dispatch.

use std::time::Duration;

use crate::ipc::IpcCommand;
use crate::rule::RuleActionKind;
use crate::state::AppState;
use crate::state::AttachMode;
use crate::state::WindowLayer;
use crate::state::WindowRule;
use crate::state::reap_zombies;
use crate::state::spawn_init_script;
use crate::tag::TAG_NONE;
use crate::tag::TagMask;

pub use crate::nav::{Direction, find_target_output, find_target_window, pick_adjacent_output};

impl AppState {
    /// Closes the currently focused window.
    pub fn close_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Ok("no view focused to close".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.request_close();
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

        let focused_out_id = self.get_focused_output_id();
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            let usable = w
                .output
                .as_ref()
                .or(focused_out_id.as_ref())
                .and_then(|id| self.outputs.get(id))
                .map(|o| o.usable_area);
            let is_floating = w.toggle_floating(usable);
            tracing::debug!(
                window_id = id,
                floating = is_floating,
                "Toggled window floating"
            );

            let proxy = w.proxy.clone();
            for seat in self.seats.values_mut() {
                seat.set_focused_window(Some(proxy.clone()));
            }

            self.uncollapse_floating_for_window_by_proxy(&proxy);
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

            let proxy = w.proxy.clone();
            w.snap(dir, usable);
            self.uncollapse_floating_for_window_by_proxy(&proxy);
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
            let prev_layer = w.layer();
            let is_fs = w.toggle_fullscreen();
            let proxy = w.proxy.clone();
            tracing::debug!(
                window_id = id,
                fullscreen = is_fs,
                "Toggled window fullscreen"
            );
            if prev_layer != WindowLayer::WorkspaceFloating
                && w.layer() == WindowLayer::WorkspaceFloating
            {
                self.uncollapse_floating_for_window_by_proxy(&proxy);
            }
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

    /// Toggles visibility of all floating views on the focused output.
    pub fn toggle_floating_views(&mut self) -> Result<String, String> {
        let focused_out_id = self.get_focused_output_id();
        let Some(out_id) = focused_out_id else {
            return Err("no focused output".to_string());
        };

        let active_tags = self.tag_state_for_output(Some(&out_id)).focused;

        let collapsible_mask: TagMask = self
            .windows
            .iter()
            .filter(|w| {
                !w.closed
                    && w.layer().policy().collapsible
                    && (w.output.as_ref() == Some(&out_id) || self.outputs.len() <= 1)
            })
            .map(|w| w.tags)
            .fold(0, |acc, tags| acc | tags);

        let is_hidden = if let Some(out) = self.outputs.get_mut(&out_id) {
            let is_currently_hidden = (out.floating_hidden_tags & active_tags) != 0;
            if is_currently_hidden {
                out.floating_hidden_tags &= !active_tags;
                false
            } else {
                out.floating_hidden_tags |= active_tags & collapsible_mask;
                (out.floating_hidden_tags & active_tags) != 0
            }
        } else {
            false
        };

        if is_hidden {
            // Hiding floating windows:
            // If the currently focused window on any seat is a collapsible floating window on this output,
            // transfer focus to the first visible tiled window on this output so the user keeps focus.
            let out_tag_state = self.tag_state_for_output(Some(&out_id));

            let first_tiled = self
                .windows
                .iter()
                .find(|w| {
                    !w.closed
                        && w.layer() == WindowLayer::Tiled
                        && out_tag_state.is_view_visible(w.tags)
                        && (w.output.as_ref() == Some(&out_id) || self.outputs.len() <= 1)
                })
                .map(|w| w.proxy.clone());

            for seat in self.seats.values_mut() {
                let focus_is_collapsible_on_this_out = seat.focused.as_ref().is_some_and(|f| {
                    self.windows.iter().any(|w| {
                        &w.proxy == f
                            && w.layer().policy().collapsible
                            && (w.output.as_ref() == Some(&out_id) || self.outputs.len() <= 1)
                    })
                });

                if focus_is_collapsible_on_this_out {
                    seat.set_focused_window(first_tiled.clone());
                }
            }
        } else {
            // Restoring floating windows:
            // Transfer focus back to the top visible workspace floating window on this output.
            let top_floating = self
                .windows
                .iter()
                .rev()
                .find(|w| {
                    !w.closed
                        && self.is_window_visible(w)
                        && w.layer() == WindowLayer::WorkspaceFloating
                        && (w.output.as_ref() == Some(&out_id) || self.outputs.len() <= 1)
                })
                .map(|w| w.proxy.clone());

            if let Some(target) = top_floating {
                for seat in self.seats.values_mut() {
                    seat.set_focused_window(Some(target.clone()));
                }
            }
        }

        self.manage_dirty();
        let state_str = if is_hidden { "hidden" } else { "visible" };
        Ok(format!("floating views {state_str}"))
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

    #[inline]
    pub fn find_target_window(&self, dir: Direction, skip_floating: bool) -> Option<u32> {
        crate::nav::find_target_window(self, dir, skip_floating)
    }

    #[inline]
    pub fn find_target_output(&self, dir_str: &str) -> Option<wayland_backend::client::ObjectId> {
        crate::nav::find_target_output(self, dir_str)
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
        if self.cursor_warp != crate::CursorWarp::Disabled
            && let Some(out) = self.outputs.get(&out_id)
        {
            let (cx, cy) = match (self.cursor_warp, dest_win) {
                (crate::CursorWarp::OnFocusChange, Some(w)) => (
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

        let dest_tags = self.tag_state_for_output(Some(&out_id)).focused;
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
            w.migrate_to_output(Some(out_id.clone()), src_area, dest_area);
            if current_tags {
                w.set_tags(dest_tags);
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
                if self.cursor_warp == crate::CursorWarp::OnFocusChange {
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
    pub fn set_cursor_warp(&mut self, warp: crate::CursorWarp) -> Result<String, String> {
        self.cursor_warp = warp;
        Ok(format!("cursor warp set to {:?}", warp).to_lowercase())
    }

    /// Sets focus-follows-cursor mode.
    pub fn set_focus_follows_cursor(
        &mut self,
        mode: crate::FocusFollowsCursor,
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
            let prev_layer = w.layer();
            w.set_tags(mask);
            let proxy = w.proxy.clone();
            if prev_layer != WindowLayer::WorkspaceFloating
                && w.layer() == WindowLayer::WorkspaceFloating
            {
                self.uncollapse_floating_for_window_by_proxy(&proxy);
            }
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
            let prev_layer = w.layer();
            w.set_tags(new_tags);
            let proxy = w.proxy.clone();
            if prev_layer != WindowLayer::WorkspaceFloating
                && w.layer() == WindowLayer::WorkspaceFloating
            {
                self.uncollapse_floating_for_window_by_proxy(&proxy);
            }
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
        self.bindings.declare_mode(mode)
    }

    /// Enters a declared modal keybinding mode.
    pub fn enter_mode(&mut self, mode: &str) -> Result<String, String> {
        let prev_mode = self.bindings.active_mode.clone();
        let res = self.bindings.enter_mode(mode)?;
        if self.bindings.mode_dirty {
            tracing::debug!(from = %prev_mode, to = %self.bindings.active_mode, "Entered mode");
            self.manage_dirty();
        }
        Ok(res)
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

            w.move_by(dir, delta);
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
                w.resize_by(horizontal, delta);
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
        let mods = crate::binds::parse_modifiers(modifiers)?;
        let Some(keysym) = crate::binds::resolve_keysym(key, mods) else {
            return Err(format!("Unknown keysym: {key}"));
        };
        let mode_norm = mode.trim();
        self.bindings.unmap_key(mode_norm, mods, keysym);
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
        let mods = crate::binds::parse_modifiers(modifiers)?;
        let Some(btn_code) = crate::binds::parse_button(button) else {
            return Err(format!("Unknown pointer button: {button}"));
        };
        let mode_norm = mode.trim();
        self.bindings
            .unmap_pointer(mode_norm, mods, btn_code, &mut self.seats);
        self.manage_dirty();
        Ok(format!(
            "unmapped pointer [{mode_norm}] {modifiers}+{button}"
        ))
    }

    /// Lists active window rules, optionally filtered by action kind.
    pub fn list_rules(&self, filter_action: Option<RuleActionKind>) -> Result<String, String> {
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

            for act in r.actions() {
                if filter_action.is_some_and(|filter| filter != act.kind()) {
                    continue;
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

    /// Deletes every rule matching app_id, title, and the targeted action kind.
    pub fn rule_del(
        &mut self,
        app_id: Option<&str>,
        title: Option<&str>,
        action: RuleActionKind,
    ) -> Result<String, String> {
        let initial_len = self.rules.len();

        self.rules.retain(|r| {
            r.app_id.as_deref() != app_id || r.title.as_deref() != title || !action.is_set_for(r)
        });

        if self.rules.len() < initial_len {
            Ok(format!(
                "rule deleted for app_id={app_id:?} title={title:?} action={}",
                action.keyword()
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
            IpcCommand::ToggleFloatingViews => self.toggle_floating_views(),
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
            IpcCommand::BorderColorFocused(color) => {
                self.border_color_focused = *color;
                self.manage_dirty();
                Ok(format!("focused border color set to {color}"))
            }
            IpcCommand::BorderColorUnfocused(color) => {
                self.border_color_unfocused = *color;
                self.manage_dirty();
                Ok(format!("unfocused border color set to {color}"))
            }
            IpcCommand::BorderColorUrgent(color) => {
                self.border_color_urgent = *color;
                self.manage_dirty();
                Ok(format!("urgent border color set to {color}"))
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
                // Defence in depth: commands can also arrive as raw IPC JSON, so
                // re-assert the domain invariants before mutating the rule store.
                action.validate()?;
                self.rules
                    .push(WindowRule::new(app_id.clone(), title.clone(), action));
                Ok(format!(
                    "rule added for app_id={app_id:?} title={title:?} action={action}"
                ))
            }
            IpcCommand::RuleDel {
                app_id,
                title,
                action,
            } => self.rule_del(app_id.as_deref(), title.as_deref(), *action),
            IpcCommand::ListRules { action } => self.list_rules(*action),
            IpcCommand::Map {
                mode,
                modifiers,
                key,
                action,
            } => {
                let mods = crate::binds::parse_modifiers(modifiers)?;
                let Some(keysym) = crate::binds::resolve_keysym(key, mods) else {
                    return Err(format!("Unknown keysym: {key}"));
                };
                let mode_norm = self
                    .bindings
                    .modes
                    .iter()
                    .find(|m| m.eq_ignore_ascii_case(mode.trim()))
                    .cloned()
                    .unwrap_or_else(|| mode.trim().to_ascii_lowercase());
                self.bindings
                    .map_key(&mode_norm, mods, keysym, action.clone());
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
                let mods = crate::binds::parse_modifiers(modifiers)?;
                let Some(btn_code) = crate::binds::parse_button(button) else {
                    return Err(format!("Unknown pointer button: {button}"));
                };
                let mode_norm = self
                    .bindings
                    .modes
                    .iter()
                    .find(|m| m.eq_ignore_ascii_case(mode.trim()))
                    .cloned()
                    .unwrap_or_else(|| mode.trim().to_ascii_lowercase());
                let ptr_action = crate::seat::PointerAction::from_tokens(action);
                self.bindings
                    .map_pointer(&mode_norm, mods, btn_code, ptr_action);
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
            IpcCommand::Status {
                stream: _,
                format,
                tag,
                window,
                scratchpad,
                pinned,
            } => {
                if *scratchpad {
                    Ok(self.format_scratchpad_status())
                } else if *pinned {
                    Ok(self.format_pinned_status())
                } else if *window {
                    Ok(self.format_window_status())
                } else if let Some(t) = tag {
                    Ok(self.format_tag_status(*t))
                } else if format.as_deref() == Some("waybar") {
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
                tracing::debug!(command = cmd, ?args, "Spawned keybinding action");
                if let Err(e) = std::process::Command::new(cmd).args(args).spawn() {
                    tracing::warn!(
                        command = cmd,
                        ?args,
                        error = %e,
                        "Failed to spawn keybinding action"
                    );
                }
            }
            return;
        }

        match crate::ipc::parse_cli_args(action) {
            Ok(cmd) => {
                if let Err(e) = self.handle_ipc_command(&cmd) {
                    tracing::warn!(?action, error = %e, "Action execution error");
                }
            }
            Err(e) => {
                tracing::warn!(?action, error = %e, "Unknown action tokens");
            }
        }
    }

    /// Handles a keybinding press event triggered by river-xkb-bindings.
    pub fn handle_key_binding_pressed(&mut self, binding_id: &wayland_backend::client::ObjectId) {
        if self.cursor_hide_when_typing {
            self.hide_cursor();
        }
        let action_opt = self
            .bindings
            .key_bindings
            .get(binding_id)
            .map(|b| b.action.clone());
        let Some(action) = action_opt else {
            return;
        };
        self.execute_action_tokens(&action);
    }
}
