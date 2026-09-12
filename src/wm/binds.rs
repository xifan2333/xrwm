//! XKB keybinding parsing and mapping for River 0.4.

use xkbcommon::xkb;

use crate::protocol::{river_seat_v1::Modifiers, river_xkb_binding_v1::RiverXkbBindingV1};

/// Parses a string of modifiers into River's `Modifiers` bitflags.
///
/// Supports combinations like `"Super"`, `"Mod4"`, `"Super+Shift"`,
/// `"Ctrl+Alt"`, `"Control_Alt"`, or `"None"`.
pub fn parse_modifiers(s: &str) -> Modifiers {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
        return Modifiers::empty();
    }

    let mut mods = Modifiers::empty();
    let parts = trimmed.split(['+', '_', '-', ' ', ',']);

    for part in parts {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match p.to_ascii_lowercase().as_str() {
            "shift" => mods |= Modifiers::Shift,
            "ctrl" | "control" => mods |= Modifiers::Ctrl,
            "alt" | "mod1" => mods |= Modifiers::Mod1,
            "mod3" => mods |= Modifiers::Mod3,
            "super" | "mod4" | "logo" | "win" => mods |= Modifiers::Mod4,
            "mod5" => mods |= Modifiers::Mod5,
            _ => tracing::warn!("Unknown modifier: {p}"),
        }
    }

    mods
}

/// Resolves an XKB keysym name and modifier context to its 32-bit keysym value.
///
/// If the key is a single alphabetic character, automatically normalizes case
/// based on whether Shift is held (e.g. `Super+Q` -> lowercase `q`, `Super+Shift+Q` -> uppercase `Q`).
pub fn resolve_keysym(key_name: &str, _modifiers: Modifiers) -> Option<u32> {
    let name = key_name.trim();
    if name.is_empty() {
        return None;
    }

    // River matches keybindings against the base keysym (lowercase for letters),
    // with modifiers like Shift tracked separately in the modifiers bitmask.
    let effective_name = if name.len() == 1 && name.chars().next().unwrap().is_ascii_alphabetic() {
        name.to_ascii_lowercase()
    } else {
        name.to_string()
    };

    parse_keysym(&effective_name)
}

/// Resolves an XKB keysym name to its 32-bit keysym value.
///
/// Tries exact case first, then common aliases, then lowercase.
pub fn parse_keysym(key_name: &str) -> Option<u32> {
    let name = key_name.trim();
    if name.is_empty() {
        return None;
    }

    // 1. Case-insensitive lookup (matches river-classic)
    let sym = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
    if sym.raw() != xkb::keysyms::KEY_NoSymbol {
        return Some(sym.raw());
    }

    // 2. Common aliases
    let alias = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => "Return",
        "esc" | "escape" => "Escape",
        "space" => "space",
        "backspace" => "BackSpace",
        "tab" => "Tab",
        "del" | "delete" => "Delete",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        _ => "",
    };
    if !alias.is_empty() {
        let sym = xkb::keysym_from_name(alias, xkb::KEYSYM_NO_FLAGS);
        if sym.raw() != xkb::keysyms::KEY_NoSymbol {
            return Some(sym.raw());
        }
    }

    // 3. Lowercase fallback
    let sym = xkb::keysym_from_name(&name.to_ascii_lowercase(), xkb::KEYSYM_NO_FLAGS);
    if sym.raw() != xkb::keysyms::KEY_NoSymbol {
        return Some(sym.raw());
    }

    None
}

/// A pending keybinding requested via IPC, queued to be registered
/// during the next manage sequence.
#[derive(Debug, Clone)]
pub struct PendingKeyBinding {
    pub mode: String,
    pub modifiers: Modifiers,
    pub keysym: u32,
    pub action: Vec<String>,
}

/// An active keybinding registered with the compositor.
#[derive(Debug)]
pub struct ActiveKeyBinding {
    pub proxy: RiverXkbBindingV1,
    pub mode: String,
    pub action: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_modifiers() {
        assert_eq!(parse_modifiers("None"), Modifiers::empty());
        assert_eq!(parse_modifiers("none"), Modifiers::empty());
        assert_eq!(parse_modifiers(""), Modifiers::empty());

        assert_eq!(parse_modifiers("Super"), Modifiers::Mod4);
        assert_eq!(parse_modifiers("Mod4"), Modifiers::Mod4);
        assert_eq!(
            parse_modifiers("Super+Shift"),
            Modifiers::Mod4 | Modifiers::Shift
        );
        assert_eq!(
            parse_modifiers("Ctrl+Alt"),
            Modifiers::Ctrl | Modifiers::Mod1
        );
        assert_eq!(
            parse_modifiers("Super+Ctrl+Alt+Shift"),
            Modifiers::Mod4 | Modifiers::Ctrl | Modifiers::Mod1 | Modifiers::Shift
        );
    }

    #[test]
    fn test_parse_keysym() {
        assert_eq!(parse_keysym("Return"), Some(xkb::keysyms::KEY_Return));
        assert_eq!(parse_keysym("Enter"), Some(xkb::keysyms::KEY_Return));
        assert_eq!(parse_keysym("space"), Some(xkb::keysyms::KEY_space));
        assert_eq!(parse_keysym("q"), Some(xkb::keysyms::KEY_q));
        assert_eq!(parse_keysym("Q"), Some(xkb::keysyms::KEY_q));
        assert_eq!(parse_keysym("1"), Some(xkb::keysyms::KEY_1));
        assert_eq!(parse_keysym("9"), Some(xkb::keysyms::KEY_9));
        assert_eq!(parse_keysym("UnknownNonExistentKey123"), None);
    }
}
