//! Window manager status serialization, formatting, and IPC broadcasting.

use crate::wm::state::AppState;

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

pub fn hex_to_river_rgba(hex_str: &str) -> (u32, u32, u32, u32) {
    parse_hex_color(hex_str).unwrap_or((u32::MAX, u32::MAX, u32::MAX, u32::MAX))
}

pub fn broadcast_status(state: &mut AppState) {
    if state.status_listeners.is_empty() {
        return;
    }
    let broadcast_deadline = std::time::Instant::now() + std::time::Duration::from_millis(10);
    let json_status = format_json_status(state);
    let waybar_status = format_waybar_status(state);

    state.status_listeners.retain_mut(|(client, fmt)| {
        let now = std::time::Instant::now();
        if now >= broadcast_deadline {
            // Do not evict healthy listeners whose writes were not attempted due to deadline exhaustion
            return true;
        }
        let client_deadline = (now + std::time::Duration::from_millis(2)).min(broadcast_deadline);
        let text = if fmt.as_deref() == Some("waybar") {
            &waybar_status
        } else {
            &json_status
        };
        let mut msg = text.clone();
        msg.push('\n');
        let _ = client.set_nonblocking(true);
        crate::ipc::write_ipc_response(client, msg.as_bytes(), client_deadline).is_ok()
    });
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
