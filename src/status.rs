//! Window manager status serialization, formatting, and IPC broadcasting.

pub use crate::render::{hex_to_river_rgba, parse_hex_color};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusSubscription {
    FullJson,
    WaybarLegacy,
    Tag(u8),
    Window,
    Scratchpad,
    Pinned,
}

/// Subscription snapshot used to deduplicate broadcasts and prevent redundant I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriberSnapshot {
    /// Workspace tag status snapshot (focused and occupied bits).
    Tag { focused: bool, occupied: bool },
    /// Focused window status snapshot.
    Window {
        focused_id: Option<u32>,
        title: Option<String>,
        app_id: Option<String>,
        floating: bool,
    },
    /// Waybar legacy text snapshot.
    WaybarLegacy {
        active_tags: String,
        focused_title: Option<String>,
        active_mode: String,
        monocle: bool,
        floating_hidden: bool,
        hidden_floating_count: usize,
        capturing: bool,
        capture_sessions: u32,
    },
    /// Full JSON status snapshot tracking model payload changes.
    FullJson { payload_hash: u64 },
    /// Scratchpad drawer status snapshot.
    Scratchpad {
        focused: bool,
        occupied: bool,
        window_count: usize,
    },
    /// Global pinned overlays status snapshot.
    Pinned {
        active: bool,
        window_count: usize,
        tooltip: String,
    },
}

/// Active status event listener tracking subscriber state and last sent snapshot.
#[derive(Debug)]
pub struct StatusListener {
    pub stream: std::os::unix::net::UnixStream,
    pub subscription: StatusSubscription,
    pub last_snapshot: Option<SubscriberSnapshot>,
}

impl StatusListener {
    /// Creates a new listener without any previously recorded snapshot.
    pub fn new(stream: std::os::unix::net::UnixStream, subscription: StatusSubscription) -> Self {
        Self {
            stream,
            subscription,
            last_snapshot: None,
        }
    }
}

/// Deep module managing status subscribers, non-blocking I/O, broken-pipe pruning,
/// and differential snapshot deduplication.
#[derive(Debug, Default)]
pub struct StatusBroadcaster {
    listeners: Vec<StatusListener>,
}

impl StatusBroadcaster {
    /// Creates an empty status broadcaster.
    pub fn new() -> Self {
        Self {
            listeners: Vec::new(),
        }
    }

    /// Number of active status subscribers.
    pub fn len(&self) -> usize {
        self.listeners.len()
    }

    /// Whether there are no active status subscribers.
    pub fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }

    /// Appends an existing status listener.
    pub fn add_listener(&mut self, listener: StatusListener) {
        self.listeners.push(listener);
    }

    /// Subscribes a UnixStream to status updates.
    pub fn subscribe(
        &mut self,
        stream: std::os::unix::net::UnixStream,
        subscription: StatusSubscription,
    ) {
        self.listeners
            .push(StatusListener::new(stream, subscription));
    }

    /// Registers a newly connected subscriber with an initial snapshot baseline.
    pub fn register(
        &mut self,
        stream: std::os::unix::net::UnixStream,
        subscription: StatusSubscription,
        initial_snapshot: Option<SubscriberSnapshot>,
    ) {
        let mut listener = StatusListener::new(stream, subscription);
        listener.last_snapshot = initial_snapshot;
        self.listeners.push(listener);
    }

    /// Active status listeners slice.
    pub fn listeners(&self) -> &[StatusListener] {
        &self.listeners
    }

    /// Mutable active status listeners.
    pub fn listeners_mut(&mut self) -> &mut Vec<StatusListener> {
        &mut self.listeners
    }

    /// Broadcasts differential status updates to all active IPC status listeners.
    pub fn broadcast(&mut self, state: &AppState) {
        if self.listeners.is_empty() {
            return;
        }

        // Lazy buffers for formatted messages (only computed on demand when a listener actually needs to send)
        let mut cached_tag_msgs: [Option<String>; 32] = Default::default();
        let mut cached_window_msg: Option<String> = None;
        let mut cached_waybar_msg: Option<String> = None;
        let mut cached_json_msg: Option<(String, u64)> = None;
        let mut cached_scratchpad_msg: Option<String> = None;
        let mut cached_pinned_msg: Option<String> = None;

        self.listeners.retain_mut(|listener| {
            let snapshot = match listener.subscription {
                StatusSubscription::FullJson => {
                    let (_, hash) = cached_json_msg.get_or_insert_with(|| {
                        let s = state.format_json_status();
                        use std::hash::{DefaultHasher, Hash, Hasher};
                        let mut hasher = DefaultHasher::new();
                        s.hash(&mut hasher);
                        let h = hasher.finish();
                        let mut line = s;
                        line.push('\n');
                        (line, h)
                    });
                    SubscriberSnapshot::FullJson {
                        payload_hash: *hash,
                    }
                }
                _ => state.current_subscriber_snapshot(&listener.subscription),
            };

            if listener.last_snapshot.as_ref() == Some(&snapshot) {
                return true;
            }

            let msg_to_send: &[u8] = match listener.subscription {
                StatusSubscription::Tag(tag) => {
                    let (is_active, is_occupied) = match snapshot {
                        SubscriberSnapshot::Tag { focused, occupied } => (focused, occupied),
                        _ => (false, false),
                    };
                    let idx = (tag.saturating_sub(1) as usize).min(31);
                    let msg = cached_tag_msgs[idx].get_or_insert_with(|| {
                        let mut s = format_tag_status_from_state(tag, is_active, is_occupied);
                        s.push('\n');
                        s
                    });
                    msg.as_bytes()
                }
                StatusSubscription::Window => {
                    let msg = cached_window_msg.get_or_insert_with(|| {
                        let mut s = format_window_status(state);
                        s.push('\n');
                        s
                    });
                    msg.as_bytes()
                }
                StatusSubscription::WaybarLegacy => {
                    let msg = cached_waybar_msg.get_or_insert_with(|| {
                        let mut s = format_waybar_status(state);
                        s.push('\n');
                        s
                    });
                    msg.as_bytes()
                }
                StatusSubscription::FullJson => cached_json_msg
                    .as_ref()
                    .map(|(s, _)| s.as_bytes())
                    .unwrap_or(b""),
                StatusSubscription::Scratchpad => {
                    let msg = cached_scratchpad_msg.get_or_insert_with(|| {
                        let mut s = format_scratchpad_status(state);
                        s.push('\n');
                        s
                    });
                    msg.as_bytes()
                }
                StatusSubscription::Pinned => {
                    let msg = cached_pinned_msg.get_or_insert_with(|| {
                        let mut s = format_pinned_status(state);
                        s.push('\n');
                        s
                    });
                    msg.as_bytes()
                }
            };

            let _ = listener.stream.set_nonblocking(true);
            let mut bytes = msg_to_send;
            let mut fatal_err = false;

            use std::io::Write;
            while !bytes.is_empty() {
                match listener.stream.write(bytes) {
                    Ok(0) => {
                        fatal_err = true;
                        break;
                    }
                    Ok(n) => {
                        bytes = &bytes[n..];
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // Buffer temporarily full: preserve connection, retry next broadcast
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        tracing::debug!(error = %e, "Status subscriber disconnected");
                        fatal_err = true;
                        break;
                    }
                }
            }

            if fatal_err {
                false
            } else {
                if bytes.is_empty() {
                    let _ = listener.stream.flush();
                    listener.last_snapshot = Some(snapshot);
                }
                true
            }
        });
    }
}

/// Broadcasts differential status updates to all active IPC status listeners.
pub fn broadcast_status(state: &mut AppState) {
    let mut broadcaster = std::mem::take(&mut state.broadcaster);
    broadcaster.broadcast(state);
    state.broadcaster = broadcaster;
}

/// Serializes the full window manager state into a single-line JSON string.
pub fn format_json_status(state: &AppState) -> String {
    let focused_id = state.focused_window_id();
    let hovered_id = state.hovered_window_id();
    let focused_win = focused_id.and_then(|id| state.windows.iter().find(|w| w.id == id));

    let title = focused_win.and_then(|w| w.title.as_deref()).unwrap_or("");
    let app_id = focused_win.and_then(|w| w.app_id.as_deref()).unwrap_or("");
    let floating = focused_win.map(|w| w.floating).unwrap_or(false);

    let mut window_list: Vec<serde_json::Value> = state
        .windows
        .iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id,
                "app_id": w.app_id.clone().unwrap_or_default(),
                "title": w.title.clone().unwrap_or_default(),
                "tags": w.tags,
                "floating": w.floating,
                "fullscreen": w.fullscreen,
                "x": w.x,
                "y": w.y,
                "width": w.effective_width(),
                "height": w.effective_height(),
                "content_width": w.content_width,
                "content_height": w.content_height,
                "capture_sessions": w.capture_sessions(),
            })
        })
        .collect();
    window_list.sort_by_key(|v| v["id"].as_u64().unwrap_or(0));

    let is_floating_hidden = state.is_focused_output_floating_hidden();
    let hidden_floating_count = if is_floating_hidden {
        let focused_out = state.get_focused_output_id();
        state
            .windows
            .iter()
            .filter(|w| {
                !w.is_closed()
                    && (focused_out.is_none()
                        || state.outputs.len() <= 1
                        || w.output() == focused_out.as_ref())
                    && state.window_visibility(w)
                        == crate::state::WindowVisibility::HiddenByCollapse
            })
            .count()
    } else {
        0
    };

    let total_capture_sessions = total_capture_sessions(state);
    let capturing = total_capture_sessions > 0;

    let focused_out_id = state.get_focused_output_id();
    let mut output_list: Vec<serde_json::Value> = state
        .outputs
        .iter()
        .map(|(id, o)| {
            let is_focused = focused_out_id.as_ref() == Some(id);
            serde_json::json!({
                "name": o.name.clone().unwrap_or_default(),
                "focused": is_focused,
                "x": o.x,
                "y": o.y,
                "width": o.width,
                "height": o.height,
                "usable_area": {
                    "x": o.usable_area.x,
                    "y": o.usable_area.y,
                    "width": o.usable_area.width,
                    "height": o.usable_area.height,
                },
                "focused_tags": o.tag_state.focused,
                "occupied_tags": o.tag_state.occupied,
                "active_tag_numbers": o.tag_state.focused_tag_indices(),
                "occupied_tag_numbers": o.tag_state.occupied_tag_indices(),
                "monocle": o.monocle,
                "floating_hidden": (o.floating_hidden_tags & o.tag_state.focused) != 0,
                "floating_hidden_tags": o.floating_hidden_tags,
                "capture_sessions": o.capture_sessions,
            })
        })
        .collect();
    output_list.sort_by_key(|v| v["name"].as_str().unwrap_or("").to_string());

    let obj = serde_json::json!({
        "focused_tags": state.tag_state.focused,
        "occupied_tags": state.tag_state.occupied,
        "active_tag_numbers": state.tag_state.focused_tag_indices(),
        "occupied_tag_numbers": state.tag_state.occupied_tag_indices(),
        "focused_window": {
            "title": title,
            "app_id": app_id,
            "floating": floating,
        },
        "layout": if state.layout_config.monocle { "monocle" } else { "master-stack" },
        "main_location": format!("{:?}", state.layout_config.main_location).to_lowercase(),
        "mode": state.bindings.active_mode,
        "focused_window_id": focused_id,
        "hovered_window_id": hovered_id,
        "pointer": { "x": state.pointer.0, "y": state.pointer.1 },
        "floating_hidden": is_floating_hidden,
        "hidden_floating_count": hidden_floating_count,
        "capturing": capturing,
        "capture_sessions": total_capture_sessions,
        "scratchpad": {
            "focused": state.tag_state.is_scratchpad_focused(),
            "occupied": state.tag_state.is_scratchpad_occupied(),
            "count": state.windows.iter().filter(|w| !w.closed && (w.tags & crate::tag::TAG_SCRATCHPAD) != 0 && w.tags != crate::tag::TAG_ALL).count(),
        },
        "pinned": {
            "active": state.windows.iter().any(|w| !w.closed && w.tags == crate::tag::TAG_ALL),
            "count": state.windows.iter().filter(|w| !w.closed && w.tags == crate::tag::TAG_ALL).count(),
        },
        "windows": window_list,
        "outputs": output_list,
    });

    serde_json::to_string(&obj).unwrap_or_default()
}

/// Formats the Waybar legacy status JSON payload including active tags, mode, and recording state.
pub fn format_waybar_status(state: &AppState) -> String {
    let active = state.tag_state.focused_tag_indices();
    let tags_text = active
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    let focused_id = state.focused_window_id();
    let focused_win = focused_id.and_then(|id| state.windows.iter().find(|w| w.id == id));
    let title = focused_win.and_then(|w| w.title.as_deref()).unwrap_or("");
    let is_floating_hidden = state.is_focused_output_floating_hidden();
    let hidden_floating_count = if is_floating_hidden {
        let focused_out = state.get_focused_output_id();
        state
            .windows
            .iter()
            .filter(|w| {
                !w.is_closed()
                    && (focused_out.is_none()
                        || state.outputs.len() <= 1
                        || w.output() == focused_out.as_ref())
                    && state.window_visibility(w)
                        == crate::state::WindowVisibility::HiddenByCollapse
            })
            .count()
    } else {
        0
    };
    let capture_sessions = total_capture_sessions(state);
    let capturing = capture_sessions > 0;

    let mut classes = vec![if state.layout_config.monocle {
        "monocle"
    } else {
        "tiled"
    }];
    if is_floating_hidden {
        classes.push("floating-hidden");
    }
    if capturing {
        classes.push("recording");
    }

    let tooltip = if is_floating_hidden {
        format!(
            "Mode: [{}], Focused: {} (floating views hidden)",
            state.bindings.active_mode, title
        )
    } else {
        format!("Mode: [{}], Focused: {}", state.bindings.active_mode, title)
    };
    let tooltip = if capturing {
        format!("{tooltip} (recording)")
    } else {
        tooltip
    };

    let obj = serde_json::json!({
        "text": tags_text,
        "tooltip": tooltip,
        "class": classes,
        "floating_hidden": is_floating_hidden,
        "hidden_floating_count": hidden_floating_count,
        "capturing": capturing,
        "capture_sessions": capture_sessions,
    });

    serde_json::to_string(&obj).unwrap_or_default()
}

/// Counts every active screen capture session known to the daemon, regardless of
/// which output or tag is currently focused: output-level recordings plus
/// window-level shares of open windows. Waybar and JSON consumers both use this
/// single source of truth so they can never disagree.
pub fn total_capture_sessions(state: &AppState) -> u32 {
    let output_sessions = state
        .outputs
        .values()
        .map(|o| o.capture_sessions)
        .fold(0u32, u32::saturating_add);
    let window_sessions = state
        .windows
        .iter()
        .filter(|w| !w.is_closed())
        .map(|w| w.capture_sessions())
        .fold(0u32, u32::saturating_add);
    output_sessions.saturating_add(window_sessions)
}

/// Returns `true` when any output or any open window has an active capture session.
pub fn is_capturing(state: &AppState) -> bool {
    total_capture_sessions(state) > 0
}

/// Formats a workspace tag status string directly from boolean flags without serde reflection.
pub fn format_tag_status_from_state(tag: u8, is_active: bool, is_occupied: bool) -> String {
    let class_str = match (is_active, is_occupied) {
        (true, true) => "[\"focused\",\"occupied\"]",
        (true, false) => "[\"focused\"]",
        (false, true) => "[\"occupied\"]",
        (false, false) => "[\"empty\"]",
    };
    format!("{{\"class\":{class_str},\"text\":\"{tag}\",\"tooltip\":\"Workspace {tag}\"}}")
}

pub fn format_tag_status(state: &AppState, tag: u8) -> String {
    let mask = 1u32
        .checked_shl((tag.saturating_sub(1)) as u32)
        .unwrap_or(0);
    let focused_tag_state = state.focused_output_tag_state();
    let is_active = (focused_tag_state.focused & mask) != 0;
    let is_occupied = (focused_tag_state.occupied & mask) != 0;
    format_tag_status_from_state(tag, is_active, is_occupied)
}

pub fn format_window_status(state: &AppState) -> String {
    let focused_id = state.focused_window_id();
    let focused_win = focused_id.and_then(|id| state.windows.iter().find(|w| w.id == id));
    let title = focused_win.and_then(|w| w.title.as_deref()).unwrap_or("");
    let app_id = focused_win.and_then(|w| w.app_id.as_deref()).unwrap_or("");
    let floating = focused_win.map(|w| w.floating).unwrap_or(false);

    let obj = serde_json::json!({
        "text": title,
        "tooltip": if app_id.is_empty() { title.to_string() } else { format!("{app_id}: {title}") },
        "class": if floating { "floating" } else { "tiled" },
    });
    serde_json::to_string(&obj).unwrap_or_default()
}

/// Formats scratchpad drawer status JSON for Waybar modules.
pub fn format_scratchpad_status(state: &AppState) -> String {
    let is_focused = state.tag_state.is_scratchpad_focused();
    let is_occupied = state.tag_state.is_scratchpad_occupied();
    let count = state
        .windows
        .iter()
        .filter(|w| {
            !w.closed && (w.tags & crate::tag::TAG_SCRATCHPAD) != 0 && w.tags != crate::tag::TAG_ALL
        })
        .count();

    let classes = match (is_focused, is_occupied) {
        (true, true) => serde_json::json!(["scratchpad", "focused", "occupied"]),
        (true, false) => serde_json::json!(["scratchpad", "focused"]),
        (false, true) => serde_json::json!(["scratchpad", "occupied"]),
        (false, false) => serde_json::json!(["scratchpad", "empty"]),
    };

    let tooltip = match (is_focused, count) {
        (true, 0) => "Scratchpad (empty, open)".to_string(),
        (true, 1) => "Scratchpad (1 window, open)".to_string(),
        (true, n) => format!("Scratchpad ({n} windows, open)"),
        (false, 0) => "Scratchpad (empty)".to_string(),
        (false, 1) => "Scratchpad (1 window)".to_string(),
        (false, n) => format!("Scratchpad ({n} windows)"),
    };

    let obj = serde_json::json!({
        "text": "󰎤",
        "class": classes,
        "tooltip": tooltip,
        "count": count,
    });
    serde_json::to_string(&obj).unwrap_or_default()
}

/// Formats global pinned/sticky windows status JSON for Waybar modules.
pub fn format_pinned_status(state: &AppState) -> String {
    let pinned_windows: Vec<&crate::state::WindowItem> = state
        .windows
        .iter()
        .filter(|w| !w.closed && w.tags == crate::tag::TAG_ALL)
        .collect();
    let count = pinned_windows.len();
    let active = count > 0;

    let classes = if active {
        serde_json::json!(["pinned", "occupied"])
    } else {
        serde_json::json!(["pinned", "empty"])
    };

    let tooltip = if count == 0 {
        "No pinned windows".to_string()
    } else {
        let titles: Vec<&str> = pinned_windows
            .iter()
            .map(|w| {
                w.app_id
                    .as_deref()
                    .or(w.title.as_deref())
                    .unwrap_or("window")
            })
            .collect();
        format!("Pinned ({count}): {}", titles.join(", "))
    };

    let text = if active { "󰐃" } else { "" };

    let obj = serde_json::json!({
        "text": text,
        "class": classes,
        "tooltip": tooltip,
        "count": count,
    });
    serde_json::to_string(&obj).unwrap_or_default()
}
