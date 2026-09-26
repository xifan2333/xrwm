//! Window matching rules and pattern matching engine.

use std::collections::HashMap;
use wayland_backend::client::ObjectId;

use crate::layout::Rect;
use crate::tag::TagMask;
use crate::wm::state::{OutputItem, WindowItem};

#[derive(Debug, Clone)]
pub struct WindowRule {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub float: Option<bool>,
    pub ssd: Option<bool>,
    pub tags: Option<TagMask>,
    pub dimensions: Option<(u32, u32)>,
    pub position: Option<(i32, i32)>,
    pub fullscreen: Option<bool>,
    pub output: Option<String>,
}

/// Performs wildcard matching where `*` matches zero or more characters at any position,
/// supporting multi-segment wildcards (e.g. `org.*.App`, `*foo*bar*`).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();

    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t_chars.len() {
        if p_idx < p_chars.len() && p_chars[p_idx] == t_chars[t_idx] {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_chars.len() && p_chars[p_idx] == '*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p_chars.len() && p_chars[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_chars.len()
}

/// Applies matching window rules to a newly mapped or metadata-updated window.
///
/// Rules specifying floating mode or decorations apply on initial creation and
/// update whenever metadata changes.
pub fn apply_rules_to_window(
    rules: &[WindowRule],
    w: &mut WindowItem,
    usable_area: Option<Rect>,
    outputs: &HashMap<ObjectId, OutputItem>,
) {
    // Collect all matching rules for this window
    let matching_rules: Vec<&WindowRule> = rules
        .iter()
        .filter(|r| {
            let app_matches = match &r.app_id {
                Some(pat) => glob_match(pat, w.app_id.as_deref().unwrap_or("")),
                None => true,
            };
            let title_matches = match &r.title {
                Some(pat) => glob_match(pat, w.title.as_deref().unwrap_or("")),
                None => true,
            };
            app_matches && title_matches
        })
        .collect();

    // Pass 1: resolve output, floating, ssd, tags, fullscreen
    for r in &matching_rules {
        if let Some(float) = r.float {
            w.floating = float;
        }
        if let Some(ssd) = r.ssd {
            w.ssd = ssd;
        }
        if !w.initial_managed {
            if let Some(tags) = r.tags {
                w.tags = tags;
            }
            if let Some(fs) = r.fullscreen {
                w.fullscreen = fs;
                w.pending_fullscreen_change = true;
            }
            if let Some(ref out_str) = r.output {
                let trimmed = out_str.trim();

                // 1. Match by display name (e.g. "DP-1", "eDP-1", case-insensitive)
                let by_name = outputs.iter().find(|(_, o)| {
                    o.name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(trimmed))
                });

                // 2. Match by exact ObjectId string or protocol ID string
                let by_id = outputs.iter().find(|(id, _)| {
                    id.to_string() == trimmed || id.protocol_id().to_string() == trimmed
                });

                // 3. Match by 1-based index with deterministic ordering (sorted by x, y, name, id)
                let by_index = if let Ok(num) = trimmed.parse::<usize>()
                    && num >= 1
                    && num <= outputs.len()
                {
                    let mut sorted: Vec<(&ObjectId, &OutputItem)> = outputs.iter().collect();
                    sorted.sort_by_key(|(id, o)| {
                        (o.x, o.y, o.name.as_deref().unwrap_or(""), id.protocol_id())
                    });
                    sorted.get(num - 1).map(|(id, o)| (*id, *o))
                } else {
                    None
                };

                let matched_out = by_name.or(by_id).or(by_index);
                if let Some((id, _)) = matched_out {
                    w.output = Some(id.clone());
                }
            }
        }
    }

    // Initial dimension and position rules only apply before initial management
    if !w.initial_managed {
        // Determine effective usable area based on resolved target output
        let effective_usable_area = w
            .output
            .as_ref()
            .and_then(|id| outputs.get(id))
            .map(|o| {
                if o.usable_area.width > 0 && o.usable_area.height > 0 {
                    o.usable_area
                } else if o.width > 0 && o.height > 0 {
                    Rect::new(o.x, o.y, o.width, o.height)
                } else {
                    o.usable_area
                }
            })
            .or(usable_area);

        // Pass 2: apply dimensions and position using effective_usable_area
        for r in &matching_rules {
            if let Some((width, height)) = r.dimensions {
                w.width = width;
                w.height = height;
                if let Some(usable) = effective_usable_area {
                    let cx = (usable.x as i64 + ((usable.width as i64 - width as i64) / 2).max(0))
                        .clamp(i32::MIN as i64, i32::MAX as i64)
                        as i32;
                    let cy = (usable.y as i64 + ((usable.height as i64 - height as i64) / 2).max(0))
                        .clamp(i32::MIN as i64, i32::MAX as i64)
                        as i32;
                    w.x = cx;
                    w.y = cy;
                    w.float_geo = Some(Rect::new(cx, cy, width, height));
                }
            }
            if let Some((px, py)) = r.position {
                w.x = px;
                w.y = py;
                w.float_geo = Some(Rect::new(px, py, w.width, w.height));
            }
        }
    }
}
