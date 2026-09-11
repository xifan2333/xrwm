//! State management for xrwm.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::ipc::IpcCommand;
use crate::layout::{LayoutConfig, Rect};
use crate::tag::{TAG_1, TagMask, TagState};

#[derive(Debug, Clone)]
pub struct WindowRule {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub float: bool,
    pub tags: Option<TagMask>,
}

#[derive(Debug, Clone)]
pub struct View {
    pub id: u32,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub tags: TagMask,
    pub floating: bool,
    pub fullscreen: bool,
    pub geometry: Rect,
}

#[derive(Debug)]
pub struct AppState {
    pub tag_state: TagState,
    pub layout_config: LayoutConfig,
    pub border_width: u32,
    pub border_color_focused: String,
    pub border_color_unfocused: String,
    pub border_color_urgent: String,
    pub views: HashMap<u32, View>,
    pub focused_view_id: Option<u32>,
    pub rules: Vec<WindowRule>,
    pub next_view_id: u32,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            tag_state: TagState::new(),
            layout_config: LayoutConfig::default(),
            border_width: 2,
            border_color_focused: "0x7aa2f7".to_string(),
            border_color_unfocused: "0x414868".to_string(),
            border_color_urgent: "0xf7768e".to_string(),
            views: HashMap::new(),
            focused_view_id: None,
            rules: Vec::new(),
            next_view_id: 1,
        }
    }

    pub fn handle_ipc_command(&mut self, cmd: &IpcCommand) -> Result<String, String> {
        match cmd {
            IpcCommand::Ping => Ok("pong".to_string()),
            IpcCommand::SetWindowGaps(g) => {
                self.layout_config.gaps = *g;
                Ok(format!("window gaps set to {g}px"))
            }
            IpcCommand::SetBorderWidth(w) => {
                self.border_width = *w;
                Ok(format!("border width set to {w}px"))
            }
            IpcCommand::SetBorderColorFocused(c) => {
                self.border_color_focused = c.clone();
                Ok(format!("focused border color set to {c}"))
            }
            IpcCommand::SetBorderColorUnfocused(c) => {
                self.border_color_unfocused = c.clone();
                Ok(format!("unfocused border color set to {c}"))
            }
            IpcCommand::SetBorderColorUrgent(c) => {
                self.border_color_urgent = c.clone();
                Ok(format!("urgent border color set to {c}"))
            }
            IpcCommand::SetSmartBorders(b) => {
                self.layout_config.smart_borders = *b;
                Ok(format!("smart borders set to {b}"))
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
            IpcCommand::Close => {
                if let Some(id) = self.focused_view_id {
                    self.views.remove(&id);
                    self.focused_view_id = self.views.keys().next().copied();
                    Ok(format!("closed view {id}"))
                } else {
                    Ok("no view focused to close".to_string())
                }
            }
            IpcCommand::Reload => {
                spawn_init_script();
                Ok("reloaded init script".to_string())
            }
            IpcCommand::Exit => {
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    std::process::exit(0);
                });
                Ok("exiting".to_string())
            }
        }
    }

    pub fn format_json_status(&self) -> String {
        let active_tags = self.tag_state.focused_tag_indices();
        let occupied_tags = self.tag_state.occupied_tag_indices();

        let focused_win = self.focused_view_id.and_then(|id| self.views.get(&id));
        let title = focused_win.and_then(|v| v.title.as_deref()).unwrap_or("");
        let app_id = focused_win.and_then(|v| v.app_id.as_deref()).unwrap_or("");

        let obj = serde_json::json!({
            "focused_tags": self.tag_state.focused,
            "occupied_tags": self.tag_state.occupied,
            "active_tag_numbers": active_tags,
            "occupied_tag_numbers": occupied_tags,
            "focused_window": {
                "title": title,
                "app_id": app_id,
            },
            "layout": if self.layout_config.monocle { "monocle" } else { "master-stack" }
        });

        serde_json::to_string(&obj).unwrap_or_default()
    }

    pub fn format_waybar_status(&self) -> String {
        let active = self.tag_state.focused_tag_indices();
        let tags_text = active
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        let focused_win = self.focused_view_id.and_then(|id| self.views.get(&id));
        let title = focused_win.and_then(|v| v.title.as_deref()).unwrap_or("");

        let obj = serde_json::json!({
            "text": tags_text,
            "tooltip": format!("Focused: {title}"),
            "class": if self.layout_config.monocle { "monocle" } else { "tiled" },
        });

        serde_json::to_string(&obj).unwrap_or_default()
    }

    pub fn add_view(&mut self, app_id: Option<String>, title: Option<String>) -> u32 {
        let id = self.next_view_id;
        self.next_view_id += 1;

        // Match rules
        let mut float = false;
        let tags = TAG_1; // default to Tag 1

        for r in &self.rules {
            if let Some(ref aid) = r.app_id {
                if app_id.as_deref() == Some(aid.as_str()) {
                    float = r.float;
                }
            }
        }

        let view = View {
            id,
            app_id,
            title,
            tags,
            floating: float,
            fullscreen: false,
            geometry: Rect::default(),
        };

        self.views.insert(id, view);
        self.focused_view_id = Some(id);
        self.sync_occupied_tags();
        id
    }

    pub fn sync_occupied_tags(&mut self) {
        let tags_list: Vec<TagMask> = self.views.values().map(|v| v.tags).collect();
        self.tag_state.update_occupied_tags(&tags_list);
    }

    /// Mirror the window manager's live window list into the shared state so
    /// `xrwm status` reports the truth and tag occupancy stays accurate.
    pub fn sync_windows(
        &mut self,
        windows: &[(u32, Option<String>, Option<String>, TagMask)],
        focused: Option<u32>,
    ) {
        let live: std::collections::HashSet<u32> = windows.iter().map(|w| w.0).collect();
        self.views.retain(|id, _| live.contains(id));

        for (id, app_id, title, tags) in windows {
            let view = self.views.entry(*id).or_insert_with(|| View {
                id: *id,
                app_id: None,
                title: None,
                tags: *tags,
                floating: false,
                fullscreen: false,
                geometry: Rect::default(),
            });
            view.app_id = app_id.clone();
            view.title = title.clone();
            view.tags = *tags;
        }

        self.focused_view_id = focused.filter(|id| self.views.contains_key(id));
        if self.focused_view_id.is_none() {
            self.focused_view_id = self.views.keys().next().copied();
        }
        self.sync_occupied_tags();
    }
}

pub fn spawn_init_script() {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    let init_script = config_dir.join("xrwm").join("init");

    if init_script.is_file() {
        tracing::info!("Spawning xrwm init script: {:?}", init_script);
        // River is launched from a TTY login shell, where ~/.local/bin is not
        // necessarily on PATH yet. Prepend it so the init script can call
        // `xrwm` directly, matching how the CLI is used interactively.
        let home = std::env::var("HOME").unwrap_or_default();
        let current_path = std::env::var("PATH").unwrap_or_default();
        let path = format!("{home}/.local/bin:{current_path}");
        let _ = std::process::Command::new("bash")
            .arg("-c")
            .arg(&init_script)
            .env("PATH", path)
            .spawn();
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
    fn test_app_state_rules_and_views() {
        let mut state = AppState::new();
        state
            .handle_ipc_command(&IpcCommand::RuleAdd {
                app_id: Some("mpv".into()),
                title: None,
                action: "float".into(),
            })
            .unwrap();

        let id = state.add_view(Some("mpv".into()), Some("Video Player".into()));
        assert!(state.views.get(&id).unwrap().floating);
        assert_eq!(state.focused_view_id, Some(id));
    }
}
