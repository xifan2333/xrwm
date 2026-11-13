//! Window matching rules, rule-action vocabulary, and pattern matching engine.

use std::collections::HashMap;
use std::fmt;
use wayland_backend::client::ObjectId;

use crate::layout::Rect;
use crate::state::{OutputItem, WindowItem};
use crate::tag::{TAG_NONE, TagMask};

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

/// The closed vocabulary of window-rule actions.
///
/// A rule action is decoded exactly once, at the IPC parsing seam, and is then
/// carried as this typed value all the way to the rule store. Illegal tokens or
/// argument shapes can therefore never reach a state mutation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "kebab-case")]
pub enum RuleAction {
    /// Force floating (`true`) or tiled (`false`) placement.
    Float(bool),
    /// Force server-side (`true`) or client-side (`false`) decorations.
    Ssd(bool),
    /// Initial tag bitmask; must contain at least one tag.
    Tags(TagMask),
    /// Initial floating window dimensions.
    Dimensions { width: u32, height: u32 },
    /// Initial floating window position.
    Position { x: i32, y: i32 },
    /// Force fullscreen (`true`) or explicitly not (`false`) on mapping.
    Fullscreen(bool),
    /// Target output selector (name, protocol id, or 1-based index).
    Output(String),
}

/// The selector half of the rule-action vocabulary: which [`WindowRule`] property
/// an action targets. `rule-del` and `list-rules` only ever need this projection,
/// never the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleActionKind {
    Float,
    Ssd,
    Tags,
    Dimensions,
    Position,
    Fullscreen,
    Output,
}

/// Every accepted flag token, mapped to the property it targets and its payload.
/// This table is the single source of truth for the CLI flag vocabulary.
const FLAG_TOKENS: &[(&str, RuleActionKind, bool)] = &[
    ("float", RuleActionKind::Float, true),
    ("no-float", RuleActionKind::Float, false),
    ("ssd", RuleActionKind::Ssd, true),
    ("csd", RuleActionKind::Ssd, false),
    ("no-ssd", RuleActionKind::Ssd, false),
    ("no-border", RuleActionKind::Ssd, false),
    ("fullscreen", RuleActionKind::Fullscreen, true),
    ("no-fullscreen", RuleActionKind::Fullscreen, false),
];

/// Every accepted token that takes positional arguments, mapped to its property.
const VALUE_TOKENS: &[(&str, RuleActionKind)] = &[
    ("tags", RuleActionKind::Tags),
    ("dimensions", RuleActionKind::Dimensions),
    ("position", RuleActionKind::Position),
    ("output", RuleActionKind::Output),
];

/// Builds the flag variant for a targeted property.
///
/// Flag tables can only reference the three boolean properties, so the remaining
/// kinds are rejected instead of being silently coerced.
fn flag_action(kind: RuleActionKind, value: bool) -> Option<RuleAction> {
    match kind {
        RuleActionKind::Float => Some(RuleAction::Float(value)),
        RuleActionKind::Ssd => Some(RuleAction::Ssd(value)),
        RuleActionKind::Fullscreen => Some(RuleAction::Fullscreen(value)),
        RuleActionKind::Tags
        | RuleActionKind::Dimensions
        | RuleActionKind::Position
        | RuleActionKind::Output => None,
    }
}

impl RuleActionKind {
    /// Resolves a CLI token (including its synonyms) to the property it targets.
    pub fn parse(token: &str) -> Result<Self, String> {
        let token = token.trim().to_ascii_lowercase();
        if let Some(&(_, kind, _)) = FLAG_TOKENS.iter().find(|(name, ..)| *name == token) {
            return Ok(kind);
        }
        if let Some(&(_, kind)) = VALUE_TOKENS.iter().find(|(name, _)| *name == token) {
            return Ok(kind);
        }
        Err(format!("Unknown rule action: {token}"))
    }

    /// The canonical CLI token rendering this property.
    pub const fn keyword(self) -> &'static str {
        match self {
            RuleActionKind::Float => "float",
            RuleActionKind::Ssd => "ssd",
            RuleActionKind::Tags => "tags",
            RuleActionKind::Dimensions => "dimensions",
            RuleActionKind::Position => "position",
            RuleActionKind::Fullscreen => "fullscreen",
            RuleActionKind::Output => "output",
        }
    }

    /// Whether the given rule currently configures this property.
    pub const fn is_set_for(self, rule: &WindowRule) -> bool {
        match self {
            RuleActionKind::Float => rule.float.is_some(),
            RuleActionKind::Ssd => rule.ssd.is_some(),
            RuleActionKind::Tags => rule.tags.is_some(),
            RuleActionKind::Dimensions => rule.dimensions.is_some(),
            RuleActionKind::Position => rule.position.is_some(),
            RuleActionKind::Fullscreen => rule.fullscreen.is_some(),
            RuleActionKind::Output => rule.output.is_some(),
        }
    }
}

impl RuleAction {
    /// Decodes CLI tokens into a validated rule action.
    ///
    /// This is the only place rule-action syntax is understood: arity, numeric
    /// parsing, and domain invariants (non-empty tag mask, in-range dimensions)
    /// are all enforced here, before any state mutation.
    pub fn parse(tokens: &[String]) -> Result<Self, String> {
        let (head, args) = tokens
            .split_first()
            .ok_or_else(|| "Rule action cannot be empty".to_string())?;
        let token = head.trim().to_ascii_lowercase();

        if let Some(&(_, kind, value)) = FLAG_TOKENS.iter().find(|(name, ..)| *name == token) {
            expect_arity(&token, args, 0)?;
            return flag_action(kind, value).ok_or_else(|| format!("Unknown rule action: {token}"));
        }

        if let Some(&(_, kind)) = VALUE_TOKENS.iter().find(|(name, _)| *name == token) {
            return Self::parse_value(kind, &token, args);
        }

        Err(format!("Unknown rule action: {token}"))
    }

    fn parse_value(kind: RuleActionKind, token: &str, args: &[String]) -> Result<Self, String> {
        match kind {
            RuleActionKind::Tags => {
                expect_arity(token, args, 1)?;
                let action = RuleAction::Tags(parse_tag_mask(&args[0])?);
                action.validate()?;
                Ok(action)
            }
            RuleActionKind::Dimensions => {
                expect_arity(token, args, 2)?;
                let width = args[0].parse::<u32>().map_err(|_| "Invalid width")?;
                let height = args[1].parse::<u32>().map_err(|_| "Invalid height")?;
                let action = RuleAction::Dimensions { width, height };
                action.validate()?;
                Ok(action)
            }
            RuleActionKind::Position => {
                expect_arity(token, args, 2)?;
                let x = args[0].parse::<i32>().map_err(|_| "Invalid x coordinate")?;
                let y = args[1].parse::<i32>().map_err(|_| "Invalid y coordinate")?;
                Ok(RuleAction::Position { x, y })
            }
            RuleActionKind::Output => {
                expect_arity(token, args, 1)?;
                Ok(RuleAction::Output(args[0].clone()))
            }
            // Value tokens never carry flag kinds; keep the match total without panicking.
            RuleActionKind::Float | RuleActionKind::Ssd | RuleActionKind::Fullscreen => {
                Err(format!("Missing value for rule action: {token}"))
            }
        }
    }

    /// The property this action targets.
    pub const fn kind(&self) -> RuleActionKind {
        match self {
            RuleAction::Float(_) => RuleActionKind::Float,
            RuleAction::Ssd(_) => RuleActionKind::Ssd,
            RuleAction::Tags(_) => RuleActionKind::Tags,
            RuleAction::Dimensions { .. } => RuleActionKind::Dimensions,
            RuleAction::Position { .. } => RuleActionKind::Position,
            RuleAction::Fullscreen(_) => RuleActionKind::Fullscreen,
            RuleAction::Output(_) => RuleActionKind::Output,
        }
    }

    /// Enforces the domain invariants that a raw payload cannot express.
    ///
    /// Used by [`RuleAction::parse`] and by the IPC handler, so a command that
    /// arrives over the socket without passing through the CLI parser is still
    /// rejected before it mutates state.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            RuleAction::Tags(TAG_NONE) => {
                Err("a window rule must specify at least one tag".to_string())
            }
            RuleAction::Dimensions { width, height }
                if *width > i32::MAX as u32 || *height > i32::MAX as u32 =>
            {
                Err("window dimensions exceed maximum allowed value".to_string())
            }
            _ => Ok(()),
        }
    }
}

fn expect_arity(token: &str, args: &[String], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        return Ok(());
    }
    Err(match token {
        "tags" => "Usage: rule-add ... tags <mask>".to_string(),
        "dimensions" => "Usage: rule-add ... dimensions <width> <height>".to_string(),
        "position" => "Usage: rule-add ... position <x> <y>".to_string(),
        "output" => "Usage: rule-add ... output <name|id>".to_string(),
        other => format!("Unexpected arguments for rule action: {other}"),
    })
}

/// Parses a tag bitmask given either in decimal or as a `0x`-prefixed hex literal.
fn parse_tag_mask(raw: &str) -> Result<TagMask, String> {
    let raw = raw.trim();
    let parsed = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => raw.parse::<u32>(),
    };
    parsed.map_err(|_| "Invalid tag mask".to_string())
}

impl fmt::Display for RuleAction {
    /// Renders the canonical CLI form, so `list-rules` remains a faithful
    /// round-trip of what `rule-add` accepted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleAction::Float(true) => f.write_str("float"),
            RuleAction::Float(false) => f.write_str("no-float"),
            RuleAction::Ssd(true) => f.write_str("ssd"),
            RuleAction::Ssd(false) => f.write_str("csd"),
            RuleAction::Tags(mask) => write!(f, "tags {mask}"),
            RuleAction::Dimensions { width, height } => write!(f, "dimensions {width} {height}"),
            RuleAction::Position { x, y } => write!(f, "position {x} {y}"),
            RuleAction::Fullscreen(true) => f.write_str("fullscreen"),
            RuleAction::Fullscreen(false) => f.write_str("no-fullscreen"),
            RuleAction::Output(name) => write!(f, "output {name}"),
        }
    }
}

impl WindowRule {
    /// Creates a rule that configures exactly one property.
    pub fn new(app_id: Option<String>, title: Option<String>, action: &RuleAction) -> Self {
        let mut rule = Self {
            app_id,
            title,
            float: None,
            ssd: None,
            tags: None,
            dimensions: None,
            position: None,
            fullscreen: None,
            output: None,
        };
        rule.set(action);
        rule
    }

    /// Applies a typed action to this rule.
    pub fn set(&mut self, action: &RuleAction) {
        match action {
            RuleAction::Float(value) => self.float = Some(*value),
            RuleAction::Ssd(value) => self.ssd = Some(*value),
            RuleAction::Tags(mask) => self.tags = Some(*mask),
            RuleAction::Dimensions { width, height } => self.dimensions = Some((*width, *height)),
            RuleAction::Position { x, y } => self.position = Some((*x, *y)),
            RuleAction::Fullscreen(value) => self.fullscreen = Some(*value),
            RuleAction::Output(name) => self.output = Some(name.clone()),
        }
    }

    /// Projects the configured properties back into the action vocabulary,
    /// in the canonical reporting order used by `list-rules`.
    pub fn actions(&self) -> Vec<RuleAction> {
        let mut actions = Vec::new();
        if let Some(float) = self.float {
            actions.push(RuleAction::Float(float));
        }
        if let Some(ssd) = self.ssd {
            actions.push(RuleAction::Ssd(ssd));
        }
        if let Some(tags) = self.tags {
            actions.push(RuleAction::Tags(tags));
        }
        if let Some((width, height)) = self.dimensions {
            actions.push(RuleAction::Dimensions { width, height });
        }
        if let Some((x, y)) = self.position {
            actions.push(RuleAction::Position { x, y });
        }
        if self.fullscreen == Some(true) {
            actions.push(RuleAction::Fullscreen(true));
        }
        if let Some(output) = &self.output {
            actions.push(RuleAction::Output(output.clone()));
        }
        actions
    }
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
