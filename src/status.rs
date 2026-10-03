//! Window manager status serialization, formatting, and IPC broadcasting.

use crate::state::AppState;

pub fn parse_hex_color(hex_str: &str) -> Result<(u32, u32, u32, u32), String> {
    let h = hex_str
        .trim_start_matches("0x")
        .trim_start_matches('#')
        .trim();

    if !h.is_ascii() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "Color '{hex_str}' contains non-hexadecimal characters"
        ));
    }

    match h.len() {
        6 => {
            let r =
                u32::from_str_radix(&h[0..2], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let g =
                u32::from_str_radix(&h[2..4], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let b =
                u32::from_str_radix(&h[4..6], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let a = u32::MAX;
            Ok((r, g, b, a))
        }
        8 => {
            let r =
                u32::from_str_radix(&h[0..2], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let g =
                u32::from_str_radix(&h[2..4], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let b =
                u32::from_str_radix(&h[4..6], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let a =
                u32::from_str_radix(&h[6..8], 16).map_err(|e| e.to_string())? * (u32::MAX / 255);
            let premultiply = |channel: u32| ((channel as u64 * a as u64) / u32::MAX as u64) as u32;
            Ok((premultiply(r), premultiply(g), premultiply(b), a))
        }
        _ => Err(format!(
            "Color '{hex_str}' has invalid length (expected 6 or 8 hex digits, got {})",
            h.len()
        )),
    }
}

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
    Pinned { active: bool, window_count: usize },
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

pub fn hex_to_river_rgba(hex_str: &str) -> (u32, u32, u32, u32) {
    parse_hex_color(hex_str).unwrap_or((u32::MAX, u32::MAX, u32::MAX, u32::MAX))
}

pub fn broadcast_status(state: &mut AppState) {
    if state.status_listeners.is_empty() {
        return;
    }

    let mut listeners = std::mem::take(&mut state.status_listeners);

    // Resolve active output masks once for all tag subscribers
    let (focused_mask, occupied_mask) = if let Some(out_id) = state.get_focused_output_id()
        && let Some(out) = state.outputs.get(&out_id)
    {
        (out.tag_state.focused, out.tag_state.occupied)
    } else {
        (state.tag_state.focused, state.tag_state.occupied)
    };

    // Lazy buffers for formatted messages (only computed on demand when a listener actually needs to send)
    let mut cached_tag_msgs: [Option<String>; 32] = Default::default();
    let mut cached_window_msg: Option<String> = None;
    let mut cached_waybar_msg: Option<String> = None;
    let mut cached_json_msg: Option<(String, u64)> = None;
    let mut cached_scratchpad_msg: Option<String> = None;
    let mut cached_pinned_msg: Option<String> = None;

    listeners.retain_mut(|listener| {
        let (current_snapshot, msg_to_send) = match listener.subscription {
            StatusSubscription::Tag(tag) => {
                let mask = 1u32
                    .checked_shl((tag.saturating_sub(1)) as u32)
                    .unwrap_or(0);
                let is_active = (focused_mask & mask) != 0;
                let is_occupied = (occupied_mask & mask) != 0;
                let snapshot = SubscriberSnapshot::Tag {
                    focused: is_active,
                    occupied: is_occupied,
                };
                if listener.last_snapshot.as_ref() == Some(&snapshot) {
                    return true;
                }
                let idx = (tag.saturating_sub(1) as usize).min(31);
                let msg = cached_tag_msgs[idx].get_or_insert_with(|| {
                    let mut s = format_tag_status_from_state(tag, is_active, is_occupied);
                    s.push('\n');
                    s
                });
                (snapshot, msg.as_bytes())
            }
            StatusSubscription::Window => {
                let focused_id = state.focused_window_id();
                let focused_win =
                    focused_id.and_then(|id| state.windows.iter().find(|w| w.id == id));
                let snapshot = SubscriberSnapshot::Window {
                    focused_id,
                    title: focused_win.and_then(|w| w.title.clone()),
                    app_id: focused_win.and_then(|w| w.app_id.clone()),
                    floating: focused_win.map(|w| w.floating).unwrap_or(false),
                };
                if listener.last_snapshot.as_ref() == Some(&snapshot) {
                    return true;
                }
                let msg = cached_window_msg.get_or_insert_with(|| {
                    let mut s = format_window_status(state);
                    s.push('\n');
                    s
                });
                (snapshot, msg.as_bytes())
            }
            StatusSubscription::WaybarLegacy => {
                let active = state.tag_state.focused_tag_indices();
                let tags_text = active
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let focused_id = state.focused_window_id();
                let focused_win =
                    focused_id.and_then(|id| state.windows.iter().find(|w| w.id == id));
                let snapshot = SubscriberSnapshot::WaybarLegacy {
                    active_tags: tags_text,
                    focused_title: focused_win.and_then(|w| w.title.clone()),
                    active_mode: state.active_mode.clone(),
                    monocle: state.layout_config.monocle,
                };
                if listener.last_snapshot.as_ref() == Some(&snapshot) {
                    return true;
                }
                let msg = cached_waybar_msg.get_or_insert_with(|| {
                    let mut s = format_waybar_status(state);
                    s.push('\n');
                    s
                });
                (snapshot, msg.as_bytes())
            }
            StatusSubscription::FullJson => {
                let (json_str, hash) = cached_json_msg.get_or_insert_with(|| {
                    let s = format_json_status(state);
                    use std::hash::{DefaultHasher, Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    s.hash(&mut hasher);
                    let h = hasher.finish();
                    let mut line = s;
                    line.push('\n');
                    (line, h)
                });
                let snapshot = SubscriberSnapshot::FullJson {
                    payload_hash: *hash,
                };
                if listener.last_snapshot.as_ref() == Some(&snapshot) {
                    return true;
                }
                (snapshot, json_str.as_bytes())
            }
            StatusSubscription::Scratchpad => {
                let is_focused = state.tag_state.is_scratchpad_focused();
                let is_occupied = state.tag_state.is_scratchpad_occupied();
                let window_count = state
                    .windows
                    .iter()
                    .filter(|w| {
                        !w.closed
                            && (w.tags & crate::tag::TAG_SCRATCHPAD) != 0
                            && w.tags != crate::tag::TAG_ALL
                    })
                    .count();
                let snapshot = SubscriberSnapshot::Scratchpad {
                    focused: is_focused,
                    occupied: is_occupied,
                    window_count,
                };
                if listener.last_snapshot.as_ref() == Some(&snapshot) {
                    return true;
                }
                let msg = cached_scratchpad_msg.get_or_insert_with(|| {
                    let mut s = format_scratchpad_status(state);
                    s.push('\n');
                    s
                });
                (snapshot, msg.as_bytes())
            }
            StatusSubscription::Pinned => {
                let window_count = state
                    .windows
                    .iter()
                    .filter(|w| !w.closed && w.tags == crate::tag::TAG_ALL)
                    .count();
                let active = window_count > 0;
                let snapshot = SubscriberSnapshot::Pinned {
                    active,
                    window_count,
                };
                if listener.last_snapshot.as_ref() == Some(&snapshot) {
                    return true;
                }
                let msg = cached_pinned_msg.get_or_insert_with(|| {
                    let mut s = format_pinned_status(state);
                    s.push('\n');
                    s
                });
                (snapshot, msg.as_bytes())
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
                listener.last_snapshot = Some(current_snapshot);
            }
            true
        }
    });

    state.status_listeners = listeners;
}

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
            })
        })
        .collect();
    window_list.sort_by_key(|v| v["id"].as_u64().unwrap_or(0));

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
        "mode": state.active_mode,
        "focused_window_id": focused_id,
        "hovered_window_id": hovered_id,
        "pointer": { "x": state.pointer.0, "y": state.pointer.1 },
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
    });

    serde_json::to_string(&obj).unwrap_or_default()
}

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

    let obj = serde_json::json!({
        "text": tags_text,
        "tooltip": format!("Mode: [{}], Focused: {}", state.active_mode, title),
        "class": if state.layout_config.monocle { "monocle" } else { "tiled" },
    });

    serde_json::to_string(&obj).unwrap_or_default()
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
    let (focused_mask, occupied_mask) = if let Some(out_id) = state.get_focused_output_id()
        && let Some(out) = state.outputs.get(&out_id)
    {
        (out.tag_state.focused, out.tag_state.occupied)
    } else {
        (state.tag_state.focused, state.tag_state.occupied)
    };
    let is_active = (focused_mask & mask) != 0;
    let is_occupied = (occupied_mask & mask) != 0;
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
