//! Window manager actions and IPC command dispatch.

use std::time::Duration;

use crate::ipc::IpcCommand;
use crate::tag::TAG_NONE;
use crate::tag::TagMask;
use crate::tag::TagState;
use crate::wm::state::AppState;
use crate::wm::state::WindowRule;
use crate::wm::state::spawn_init_script;

impl AppState {
    /// Closes the currently focused window.
    pub fn close_focused(&mut self) -> Result<String, String> {
        let focused_id = self.focused_window_id();
        let Some(id) = focused_id else {
            return Ok("no view focused to close".to_string());
        };

        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.proxy.close();
            w.closed = true;
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
            w.node.place_top();
            self.manage_dirty();
            Ok(format!("window {id} floating={is_floating}"))
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

    /// Shifts focus to the next or previous visible window.
    pub fn focus_view(&mut self, next: bool) -> Result<String, String> {
        let tag_state = self.tag_state;
        let visible: Vec<u32> = self
            .windows
            .iter()
            .filter(|w| !w.closed && tag_state.is_view_visible(w.tags))
            .map(|w| w.id)
            .collect();

        if visible.is_empty() {
            return Ok("no visible windows".to_string());
        }

        let current = self.focused_window_id();
        let idx = current
            .and_then(|id| visible.iter().position(|&v| v == id))
            .unwrap_or(0);

        let new_idx = if next {
            (idx + 1) % visible.len()
        } else {
            (idx + visible.len() - 1) % visible.len()
        };

        let new_id = visible[new_idx];
        if let Some(win) = self.windows.iter().find(|w| w.id == new_id) {
            for seat in self.seats.values_mut() {
                seat.focused = Some(win.proxy.clone());
            }
            self.manage_dirty();
            Ok(format!("focused window {new_id}"))
        } else {
            Err("failed to focus window".to_string())
        }
    }

    /// Sets the focused tags mask on the WM.
    pub fn set_focused_tags(&mut self, mask: TagMask) -> Result<String, String> {
        if mask == TAG_NONE {
            return Err("at least one tag must be focused".to_string());
        }
        self.tag_state.set_focused_tags(mask);
        self.anim.start();
        self.manage_dirty();
        Ok(format!("focused tags set to {mask}"))
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
            IpcCommand::Zoom => self.zoom_focused(),
            IpcCommand::FocusView(dir) => {
                let next = !matches!(dir.as_str(), "previous" | "prev" | "up" | "left");
                self.focus_view(next)
            }
            IpcCommand::SetFocusedTags(mask) => self.set_focused_tags(*mask),
            IpcCommand::ToggleFocusedTags(mask) => self.toggle_focused_tags(*mask),
            IpcCommand::SetViewTags(mask) => self.set_view_tags(*mask),
            IpcCommand::ToggleViewTags(mask) => self.toggle_view_tags(*mask),
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
            IpcCommand::SetSmartBorders(b) => {
                self.layout_config.smart_borders = *b;
                self.manage_dirty();
                Ok(format!("smart borders set to {b}"))
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
                let float = action.contains("float");
                self.rules.push(WindowRule {
                    app_id: app_id.clone(),
                    title: title.clone(),
                    float,
                    tags: None,
                });
                Ok(format!("rule added for app_id={app_id:?} title={title:?}"))
            }
            IpcCommand::Map {
                mode,
                modifiers,
                key,
                action,
            } => Ok(format!("mapped [{mode}] {modifiers}+{key} -> {action:?}")),
            IpcCommand::Bind { combo, action } => Ok(format!("bound {combo} -> {action:?}")),
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
        assert!(state.rules[0].float);
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
    fn test_app_state_status_formatting() {
        let state = AppState::new();
        let json = state.format_json_status();
        assert!(json.contains("\"focused_tags\":1"));
        assert!(json.contains("\"layout\":\"master-stack\""));

        let waybar = state.format_waybar_status();
        assert!(waybar.contains("\"text\":\"1\""));
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
