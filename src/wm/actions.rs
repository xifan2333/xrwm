//! Window manager actions and IPC command dispatch.

use std::time::Duration;

use crate::ipc::IpcCommand;
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::tag::TagState;
use crate::wm::state::AppState;
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
    pub fn zoom_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Err("no view focused".to_string());
        };

        let pos = self.windows.iter().position(|w| w.id == id);
        if let Some(idx) = pos {
            if idx > 0 {
                let win = self.windows.remove(idx);
                self.windows.insert(0, win);
                self.manage_dirty();
            }
            Ok(format!("zoomed window {id}"))
        } else {
            Err("window not found".to_string())
        }
    }

    /// Finds the target window in the given direction.
    pub fn find_target_window(&self, dir: Direction) -> Option<u32> {
        let tag_state = self.tag_state;
        let visible: Vec<&crate::wm::state::WindowItem> = self
            .windows
            .iter()
            .filter(|w| !w.closed && tag_state.is_view_visible(w.tags))
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

    /// Shifts focus in the specified direction (next, prev, left, right, up, down).
    pub fn focus_view_direction(&mut self, dir_str: &str) -> Result<String, String> {
        let visible_count = self
            .windows
            .iter()
            .filter(|w| !w.closed && self.tag_state.is_view_visible(w.tags))
            .count();
        if visible_count == 0 {
            return Ok("no visible windows".to_string());
        }

        let dir = Direction::parse(dir_str).unwrap_or(Direction::Next);
        let target_id = self.find_target_window(dir);
        let Some(new_id) = target_id else {
            return Ok("no target window in direction".to_string());
        };

        if let Some(win) = self.windows.iter().find(|w| w.id == new_id) {
            let proxy = win.proxy.clone();
            for seat in self.seats.values_mut() {
                seat.focused = Some(proxy.clone());
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

        let target_id = self.find_target_window(dir);
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
            self.focus_view_direction("next")
        } else {
            self.focus_view_direction("previous")
        }
    }

    /// Sets the focused tags mask on the WM.
    pub fn set_focused_tags(&mut self, mask: TagMask) -> Result<String, String> {
        if mask == TAG_NONE {
            return Err("at least one tag must be focused".to_string());
        }
        if self.tag_state.focused != mask {
            self.previous_focused_tags = self.tag_state.focused;
        }
        self.tag_state.set_focused_tags(mask);
        self.anim.start();
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

    /// Toggles the focused tags mask on the WM.
    pub fn toggle_focused_tags(&mut self, mask: TagMask) -> Result<String, String> {
        self.tag_state.toggle_focused_tags(mask);
        self.anim.start();
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

    /// Sets the layout main split ratio (clamped to 0.1 .. 0.9).
    pub fn set_main_ratio(&mut self, ratio: f32) -> Result<String, String> {
        let clamped = ratio.clamp(0.1, 0.9);
        self.layout_config.split_ratio = clamped;
        self.manage_dirty();
        Ok(format!("main ratio set to {clamped:.2}"))
    }

    /// Sets the number of windows in the master layout area.
    pub fn set_main_count(&mut self, count: u32) -> Result<String, String> {
        let c = count.max(1);
        self.layout_config.main_count = c;
        self.manage_dirty();
        Ok(format!("main count set to {c}"))
    }

    /// Handles an incoming IPC command and synchronously applies it to the WM.
    pub fn handle_ipc_command(&mut self, cmd: &IpcCommand) -> Result<String, String> {
        match cmd {
            IpcCommand::Ping => Ok("pong".to_string()),
            IpcCommand::Close => self.close_focused(),
            IpcCommand::ToggleFloat => self.toggle_float_focused(),
            IpcCommand::ToggleFullscreen => self.toggle_fullscreen_focused(),
            IpcCommand::Zoom => self.zoom_focused(),
            IpcCommand::FocusView(dir) => self.focus_view_direction(dir),
            IpcCommand::Swap(dir) => self.swap_direction(dir),
            IpcCommand::SetFocusedTags(mask) => self.set_focused_tags(*mask),
            IpcCommand::ToggleFocusedTags(mask) => self.toggle_focused_tags(*mask),
            IpcCommand::SetViewTags(mask) => self.set_view_tags(*mask),
            IpcCommand::ToggleViewTags(mask) => self.toggle_view_tags(*mask),
            IpcCommand::FocusPreviousTags => self.focus_previous_tags(),
            IpcCommand::SendToPreviousTags => self.send_to_previous_tags(),
            IpcCommand::SetMainLocation(loc) => self.set_main_location(*loc),
            IpcCommand::FocusTag(idx) => {
                let mask = TagState::tag_index_to_mask(*idx);
                self.set_focused_tags(mask)
            }
            IpcCommand::MoveToTag(idx) => {
                let mask = TagState::tag_index_to_mask(*idx);
                self.set_view_tags(mask)
            }
            IpcCommand::SetWindowGaps(g) => {
                self.layout_config.gaps = *g;
                self.manage_dirty();
                Ok(format!("window gaps set to {g}px"))
            }
            IpcCommand::SetBorderWidth(w) => {
                self.border_width = *w;
                self.manage_dirty();
                Ok(format!("border width set to {w}px"))
            }
            IpcCommand::SetBorderColorFocused(c) => {
                self.border_color_focused = c.clone();
                self.manage_dirty();
                Ok(format!("focused border color set to {c}"))
            }
            IpcCommand::SetBorderColorUnfocused(c) => {
                self.border_color_unfocused = c.clone();
                self.manage_dirty();
                Ok(format!("unfocused border color set to {c}"))
            }
            IpcCommand::SetBorderColorUrgent(c) => {
                self.border_color_urgent = c.clone();
                self.manage_dirty();
                Ok(format!("urgent border color set to {c}"))
            }
            IpcCommand::SetMainRatio(r) => self.set_main_ratio(*r),
            IpcCommand::SetMainCount(c) => self.set_main_count(*c),
            IpcCommand::SetAnimation(enabled) => {
                self.anim.enabled = *enabled;
                Ok(format!("animations set to {enabled}"))
            }
            IpcCommand::SetAnimationDuration(ms) => {
                self.anim.duration = Duration::from_millis(*ms);
                Ok(format!("animation duration set to {ms}ms"))
            }
            IpcCommand::RuleAdd {
                app_id,
                title,
                action,
            } => {
                let float = if action.contains("float") && !action.contains("no-float") {
                    Some(true)
                } else if action.contains("no-float") {
                    Some(false)
                } else {
                    None
                };
                let ssd = if action.contains("csd")
                    || action.contains("no-ssd")
                    || action.contains("no-border")
                {
                    Some(false)
                } else if action.contains("ssd") {
                    Some(true)
                } else {
                    None
                };
                self.rules.push(WindowRule {
                    app_id: app_id.clone(),
                    title: title.clone(),
                    float,
                    ssd,
                    tags: None,
                });
                Ok(format!("rule added for app_id={app_id:?} title={title:?}"))
            }
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
            IpcCommand::Bind { combo, action } => {
                let (mods_str, key_str) = match combo.rfind('+') {
                    Some(idx) => (&combo[..idx], &combo[idx + 1..]),
                    None => ("None", combo.as_str()),
                };
                let mods = crate::wm::binds::parse_modifiers(mods_str);
                let Some(keysym) = crate::wm::binds::resolve_keysym(key_str, mods) else {
                    return Err(format!("Unknown keysym: {key_str}"));
                };
                self.pending_key_bindings
                    .push(crate::wm::binds::PendingKeyBinding {
                        mode: "normal".to_string(),
                        modifiers: mods,
                        keysym,
                        action: action.clone(),
                    });
                self.manage_dirty();
                Ok(format!("bound {combo} -> {action:?}"))
            }
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

    /// Handles a keybinding press event triggered by river-xkb-bindings.
    pub fn handle_key_binding_pressed(&mut self, binding_id: &wayland_backend::client::ObjectId) {
        let action_opt = self.key_bindings.get(binding_id).map(|b| b.action.clone());
        let Some(action) = action_opt else {
            return;
        };

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
            "zoom" => {
                let _ = self.zoom_focused();
            }
            "focus-view" => {
                let dir = action.get(1).map(|s| s.as_str()).unwrap_or("next");
                let _ = self.focus_view_direction(dir);
            }
            "swap" => {
                let dir = action.get(1).map(|s| s.as_str()).unwrap_or("next");
                let _ = self.swap_direction(dir);
            }
            "focus-tag" => {
                if action.len() > 1
                    && let Ok(tag) = action[1].parse::<u8>()
                {
                    let mask = TagState::tag_index_to_mask(tag);
                    let _ = self.set_focused_tags(mask);
                }
            }
            "move-to-tag" => {
                if action.len() > 1
                    && let Ok(tag) = action[1].parse::<u8>()
                {
                    let mask = TagState::tag_index_to_mask(tag);
                    let _ = self.set_view_tags(mask);
                }
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
            "exit" => {
                self.should_exit = true;
            }
            "reload" => {
                spawn_init_script();
            }
            other => {
                tracing::warn!("Unknown keybinding action: {other}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_ipc_gaps() {
        let mut state = AppState::new();
        let cmd = IpcCommand::SetWindowGaps(8);
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
            action: "float".into(),
        };
        let res = state.handle_ipc_command(&cmd);
        assert!(res.is_ok());
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].float, Some(true));
        assert_eq!(state.rules[0].ssd, None);

        let csd_cmd = IpcCommand::RuleAdd {
            app_id: Some("foot".into()),
            title: None,
            action: "csd".into(),
        };
        let res_csd = state.handle_ipc_command(&csd_cmd);
        assert!(res_csd.is_ok());
        assert_eq!(state.rules[1].ssd, Some(false));
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
    }

    #[test]
    fn test_app_state_ipc_layout_and_animation() {
        let mut state = AppState::new();
        state
            .handle_ipc_command(&IpcCommand::SetMainRatio(0.65))
            .unwrap();
        assert!((state.layout_config.split_ratio - 0.65).abs() < f32::EPSILON);

        state
            .handle_ipc_command(&IpcCommand::SetMainCount(2))
            .unwrap();
        assert_eq!(state.layout_config.main_count, 2);

        state
            .handle_ipc_command(&IpcCommand::SetAnimation(false))
            .unwrap();
        assert!(!state.anim.enabled);

        state
            .handle_ipc_command(&IpcCommand::SetAnimationDuration(200))
            .unwrap();
        assert_eq!(state.anim.duration, Duration::from_millis(200));
    }

    #[test]
    fn test_app_state_ipc_map_and_bind() {
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

        let bind_cmd = IpcCommand::Bind {
            combo: "Super+Shift+Q".into(),
            action: vec!["exit".into()],
        };
        let res2 = state.handle_ipc_command(&bind_cmd);
        assert!(res2.is_ok());
        assert_eq!(state.pending_key_bindings.len(), 2);
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
    fn test_app_state_previous_tags_and_location() {
        let mut state = AppState::new();
        state.set_focused_tags(2).unwrap();
        assert_eq!(state.tag_state.focused, 2);
        assert_eq!(state.previous_focused_tags, 1);

        state.focus_previous_tags().unwrap();
        assert_eq!(state.tag_state.focused, 1);

        state
            .set_main_location(crate::layout::MainLocation::Right)
            .unwrap();
        assert_eq!(
            state.layout_config.main_location,
            crate::layout::MainLocation::Right
        );
    }

    #[test]
    fn test_app_state_empty_actions() {
        let mut state = AppState::new();
        assert_eq!(state.close_focused().unwrap(), "no view focused to close");
        assert!(state.toggle_float_focused().is_err());
        assert!(state.zoom_focused().is_err());
        assert_eq!(state.focus_view(true).unwrap(), "no visible windows");
    }
}
