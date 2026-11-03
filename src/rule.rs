//! Window matching rules and pattern matching engine.

use std::collections::HashMap;
use wayland_backend::client::ObjectId;

use crate::layout::Rect;
use crate::state::{OutputItem, WindowItem};
use crate::tag::TagMask;

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
/// Designed strictly after river-classic:
/// - Floating windows or windows with dimension rules without an explicit position
///   rule are centered on the target output's usable area.
/// - Floating geometry is clean and never polluted by residual tiled layout coordinates.
pub fn apply_rules_to_window(
    rules: &[WindowRule],
    w: &mut WindowItem,
    usable_area: Option<Rect>,
    outputs: &HashMap<ObjectId, OutputItem>,
) {
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

    let mut rule_float: Option<bool> = None;
    let mut matched_ssd = None;
    let mut matched_tags = None;
    let mut matched_fullscreen = None;
    let mut matched_output = None;
    let mut matched_dimensions = None;
    let mut matched_position = None;

    // Pass 1: resolve output, floating, ssd, tags, fullscreen
    for r in &matching_rules {
        if let Some(float) = r.float {
            rule_float = Some(float);
        }
        if let Some(ssd) = r.ssd {
            matched_ssd = Some(ssd);
        }
        if !w.initial_managed {
            if let Some(tags) = r.tags {
                matched_tags = Some(tags);
            }
            if let Some(fs) = r.fullscreen {
                matched_fullscreen = Some(fs);
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
                    matched_output = Some(id.clone());
                }
            }
        }
        if r.dimensions.is_some() {
            matched_dimensions = r.dimensions;
        }
        if r.position.is_some() {
            matched_position = r.position;
        }
    }

    // Transient top-levels (dialogs, file pickers, popup menus) reported via the river
    // `parent` event float by default; an explicit `no-float` rule still wins.
    let effective_float = rule_float.or(if w.has_parent { Some(true) } else { None });

    // Determine effective usable area based on resolved target output
    let target_out = matched_output.as_ref().or(w.output.as_ref());
    let effective_usable_area = target_out
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

    let config = crate::window::RuleConfig {
        float: effective_float,
        ssd: matched_ssd,
        tags: matched_tags,
        fullscreen: matched_fullscreen,
        output: matched_output,
        dimensions: matched_dimensions,
        position: matched_position,
    };
    w.apply_rule_properties(config, effective_usable_area);
}
