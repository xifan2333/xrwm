//! XKB keybinding parsing and mapping for River 0.4.

use xkbcommon::xkb;

use crate::protocol::{river_seat_v1::Modifiers, river_xkb_binding_v1::RiverXkbBindingV1};

/// Parses a string of modifiers into River's `Modifiers` bitflags.
///
/// Supports combinations like `"Super"`, `"Mod4"`, `"Super+Shift"`,
/// `"Ctrl+Alt"`, `"Control_Alt"`, or `"None"`.
pub fn parse_modifiers(s: &str) -> Result<Modifiers, String> {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
        return Ok(Modifiers::empty());
    }

    let mut mods = Modifiers::empty();
    let parts = trimmed.split(['+', '_', '-', ' ', ',']);

    for part in parts {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match p.to_ascii_lowercase().as_str() {
            "none" => {}
            "shift" => mods |= Modifiers::Shift,
            "ctrl" | "control" => mods |= Modifiers::Ctrl,
            "alt" | "mod1" => mods |= Modifiers::Mod1,
            "mod3" => mods |= Modifiers::Mod3,
            "super" | "mod4" | "logo" | "win" => mods |= Modifiers::Mod4,
            "mod5" => mods |= Modifiers::Mod5,
            _ => return Err(format!("Unknown modifier: {p}")),
        }
    }

    Ok(mods)
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
    let effective_name = match name.as_bytes() {
        [byte] if byte.is_ascii_alphabetic() => name.to_ascii_lowercase(),
        _ => name.to_string(),
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

/// Parses a Linux input event code name into its numerical button code.
///
/// Supports names like `BTN_LEFT`, `BTN_RIGHT`, `BTN_MIDDLE`, `BTN_SIDE`,
/// `BTN_EXTRA`, `BTN_FORWARD`, `BTN_BACK`, or hex/integer codes.
pub fn parse_button(s: &str) -> Option<u32> {
    let trimmed = s.trim();
    match trimmed.to_ascii_uppercase().as_str() {
        "BTN_LEFT" | "LEFT" => Some(0x110),
        "BTN_RIGHT" | "RIGHT" => Some(0x111),
        "BTN_MIDDLE" | "MIDDLE" => Some(0x112),
        "BTN_SIDE" | "SIDE" => Some(0x113),
        "BTN_EXTRA" | "EXTRA" => Some(0x114),
        "BTN_FORWARD" | "FORWARD" => Some(0x115),
        "BTN_BACK" | "BACK" => Some(0x116),
        "BTN_TASK" | "TASK" => Some(0x117),
        other => {
            if let Some(hex) = other.strip_prefix("0X") {
                u32::from_str_radix(hex, 16).ok()
            } else {
                other.parse::<u32>().ok()
            }
        }
    }
}

/// A pending pointer binding requested via IPC, queued to be registered with seats.
#[derive(Debug, Clone)]
pub struct PendingPointerBinding {
    pub mode: String,
    pub modifiers: Modifiers,
    pub button: u32,
    pub action: crate::wm::seat::PointerAction,
}

/// An active keybinding registered with the compositor.
#[derive(Debug)]
pub struct ActiveKeyBinding {
    pub proxy: RiverXkbBindingV1,
    pub seat_id: wayland_backend::client::ObjectId,
    pub mode: String,
    pub modifiers: Modifiers,
    pub keysym: u32,
    pub action: Vec<String>,
}

#[cfg(test)]
#[path = "binds_test.rs"]
mod tests;
