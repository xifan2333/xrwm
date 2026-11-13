//! XKB keybinding parsing and mapping for River 0.4.

use std::collections::HashMap;

use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPointerBinding {
    pub mode: String,
    pub modifiers: Modifiers,
    pub button: u32,
    pub action: crate::seat::PointerAction,
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

/// Deep module managing modal states, key/pointer binding configurations,
/// and River compositor proxy synchronization.
#[derive(Debug)]
pub struct BindingManager {
    pub configured_key_bindings: Vec<PendingKeyBinding>,
    pub pending_key_bindings: Vec<PendingKeyBinding>,
    pub key_bindings: HashMap<ObjectId, ActiveKeyBinding>,
    pub configured_pointer_bindings: Vec<PendingPointerBinding>,
    pub pending_pointer_bindings: Vec<PendingPointerBinding>,
    pub active_mode: String,
    pub modes: Vec<String>,
    pub mode_dirty: bool,
    pub session_locked: bool,
    pub pre_lock_mode: Option<String>,
}

impl Default for BindingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingManager {
    pub fn new() -> Self {
        Self {
            configured_key_bindings: Vec::new(),
            pending_key_bindings: Vec::new(),
            key_bindings: HashMap::new(),
            configured_pointer_bindings: Vec::new(),
            pending_pointer_bindings: Vec::new(),
            active_mode: "normal".to_string(),
            modes: vec!["normal".to_string(), "locked".to_string()],
            mode_dirty: false,
            session_locked: false,
            pre_lock_mode: None,
        }
    }

    pub fn declare_mode(&mut self, mode: &str) -> Result<String, String> {
        let name = mode.trim();
        if name.is_empty() {
            return Err("mode name cannot be empty".to_string());
        }
        let canonical = name.to_ascii_lowercase();
        if !self.modes.iter().any(|m| m == &canonical) {
            self.modes.push(canonical.clone());
        }
        Ok(format!("declared mode {canonical}"))
    }

    pub fn enter_mode(&mut self, mode: &str) -> Result<String, String> {
        let name = mode.trim();
        let canonical = self
            .modes
            .iter()
            .find(|m| m.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| format!("unknown mode '{name}', declare it first"))?;
        if self.session_locked && canonical != "locked" {
            return Err("cannot switch mode while session is locked".to_string());
        }
        if self.active_mode != canonical {
            self.active_mode = canonical.clone();
            self.mode_dirty = true;
        }
        Ok(format!("entered mode {canonical}"))
    }

    pub fn handle_session_locked(&mut self) -> bool {
        self.session_locked = true;
        if self.active_mode != "locked" {
            self.pre_lock_mode = Some(self.active_mode.clone());
            self.active_mode = "locked".to_string();
            self.mode_dirty = true;
            true
        } else {
            false
        }
    }

    pub fn handle_session_unlocked(&mut self) -> bool {
        self.session_locked = false;
        let target_mode = self
            .pre_lock_mode
            .take()
            .filter(|m| {
                self.modes
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(m))
            })
            .unwrap_or_else(|| "normal".to_string());

        if self.active_mode != target_mode {
            self.active_mode = target_mode;
            self.mode_dirty = true;
            true
        } else {
            false
        }
    }

    pub fn map_key(&mut self, mode: &str, modifiers: Modifiers, keysym: u32, action: Vec<String>) {
        // Replace existing binding for same (mode, modifiers, keysym)
        self.configured_key_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.keysym == keysym)
        });
        self.pending_key_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.keysym == keysym)
        });

        let to_remove: Vec<ObjectId> = self
            .key_bindings
            .iter()
            .filter(|(_, b)| {
                b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.keysym == keysym
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_remove {
            if let Some(b) = self.key_bindings.remove(&id) {
                b.proxy.destroy();
            }
        }

        let pending = PendingKeyBinding {
            mode: mode.to_string(),
            modifiers,
            keysym,
            action,
        };
        self.configured_key_bindings.push(pending.clone());
        self.pending_key_bindings.push(pending);
    }

    pub fn unmap_key(&mut self, mode: &str, modifiers: Modifiers, keysym: u32) -> bool {
        let before_configured = self.configured_key_bindings.len();
        self.configured_key_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.keysym == keysym)
        });
        self.pending_key_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.keysym == keysym)
        });

        let to_remove: Vec<ObjectId> = self
            .key_bindings
            .iter()
            .filter(|(_, b)| {
                b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.keysym == keysym
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in &to_remove {
            if let Some(b) = self.key_bindings.remove(id) {
                b.proxy.destroy();
            }
        }

        before_configured != self.configured_key_bindings.len() || !to_remove.is_empty()
    }

    pub fn map_pointer(
        &mut self,
        mode: &str,
        modifiers: Modifiers,
        button: u32,
        action: crate::seat::PointerAction,
    ) {
        self.configured_pointer_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.button == button)
        });
        self.pending_pointer_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.button == button)
        });
        let pending = PendingPointerBinding {
            mode: mode.to_string(),
            modifiers,
            button,
            action,
        };
        self.configured_pointer_bindings.push(pending.clone());
        self.pending_pointer_bindings.push(pending);
    }

    pub fn unmap_pointer(
        &mut self,
        mode: &str,
        modifiers: Modifiers,
        button: u32,
        seats: &mut HashMap<ObjectId, crate::seat::SeatItem>,
    ) -> bool {
        let before_configured = self.configured_pointer_bindings.len();
        self.configured_pointer_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.button == button)
        });
        self.pending_pointer_bindings.retain(|b| {
            !(b.mode.eq_ignore_ascii_case(mode) && b.modifiers == modifiers && b.button == button)
        });

        let mut removed_active = false;
        for seat in seats.values_mut() {
            let to_remove: Vec<ObjectId> = seat
                .pointer_bindings
                .iter()
                .filter(|(_, b)| {
                    b.mode.eq_ignore_ascii_case(mode)
                        && b.modifiers == modifiers
                        && b.button == button
                })
                .map(|(id, _)| id.clone())
                .collect();

            for id in &to_remove {
                if let Some(b) = seat.pointer_bindings.remove(id) {
                    b.proxy.destroy();
                    removed_active = true;
                }
            }
        }

        before_configured != self.configured_pointer_bindings.len() || removed_active
    }

    /// Cleans up active keybindings associated with a removed seat.
    pub fn remove_seat_bindings(&mut self, seat_id: &ObjectId) {
        let to_remove: Vec<ObjectId> = self
            .key_bindings
            .iter()
            .filter(|(_, b)| &b.seat_id == seat_id)
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_remove {
            if let Some(b) = self.key_bindings.remove(&id) {
                b.proxy.destroy();
            }
        }
    }

    /// Synchronizes key and pointer bindings with the River compositor during the manage sequence.
    pub fn sync_compositor(
        &mut self,
        river_xkb: Option<&crate::protocol::river_xkb_bindings_v1::RiverXkbBindingsV1>,
        seats: &mut HashMap<ObjectId, crate::seat::SeatItem>,
        qh: &wayland_client::QueueHandle<crate::state::AppState>,
    ) {
        if let Some(xkb_mgr) = river_xkb {
            for seat in seats.values_mut() {
                if seat.needs_key_binding_sync {
                    for def in &self.configured_key_bindings {
                        let binding =
                            xkb_mgr.get_xkb_binding(&seat.proxy, def.keysym, def.modifiers, qh, ());
                        if def.mode.eq_ignore_ascii_case(&self.active_mode) {
                            binding.enable();
                        } else {
                            binding.disable();
                        }
                        self.key_bindings.insert(
                            binding.id(),
                            ActiveKeyBinding {
                                proxy: binding,
                                seat_id: seat.proxy.id(),
                                mode: def.mode.clone(),
                                modifiers: def.modifiers,
                                keysym: def.keysym,
                                action: def.action.clone(),
                            },
                        );
                    }
                    seat.needs_key_binding_sync = false;
                } else if !self.pending_key_bindings.is_empty() {
                    for pending in &self.pending_key_bindings {
                        let binding = xkb_mgr.get_xkb_binding(
                            &seat.proxy,
                            pending.keysym,
                            pending.modifiers,
                            qh,
                            (),
                        );
                        if pending.mode.eq_ignore_ascii_case(&self.active_mode) {
                            binding.enable();
                        } else {
                            binding.disable();
                        }
                        self.key_bindings.insert(
                            binding.id(),
                            ActiveKeyBinding {
                                proxy: binding,
                                seat_id: seat.proxy.id(),
                                mode: pending.mode.clone(),
                                modifiers: pending.modifiers,
                                keysym: pending.keysym,
                                action: pending.action.clone(),
                            },
                        );
                    }
                }
            }
            self.pending_key_bindings.clear();
        }

        for (seat_id, seat) in seats.iter_mut() {
            if seat.needs_pointer_binding_sync {
                for def in &self.configured_pointer_bindings {
                    let pb = seat.proxy.get_pointer_binding(
                        def.button,
                        def.modifiers,
                        qh,
                        seat_id.clone(),
                    );
                    if def.mode.eq_ignore_ascii_case(&self.active_mode) {
                        pb.enable();
                    } else {
                        pb.disable();
                    }
                    seat.pointer_bindings.insert(
                        pb.id(),
                        crate::seat::PointerBinding {
                            proxy: pb,
                            mode: def.mode.clone(),
                            modifiers: def.modifiers,
                            button: def.button,
                            action: def.action.clone(),
                        },
                    );
                }
                seat.needs_pointer_binding_sync = false;
            } else if !self.pending_pointer_bindings.is_empty() {
                for pending in &self.pending_pointer_bindings {
                    let pb = seat.proxy.get_pointer_binding(
                        pending.button,
                        pending.modifiers,
                        qh,
                        seat_id.clone(),
                    );
                    if pending.mode.eq_ignore_ascii_case(&self.active_mode) {
                        pb.enable();
                    } else {
                        pb.disable();
                    }
                    seat.pointer_bindings.insert(
                        pb.id(),
                        crate::seat::PointerBinding {
                            proxy: pb,
                            mode: pending.mode.clone(),
                            modifiers: pending.modifiers,
                            button: pending.button,
                            action: pending.action.clone(),
                        },
                    );
                }
            }
        }
        self.pending_pointer_bindings.clear();

        if self.mode_dirty {
            for kb in self.key_bindings.values() {
                if kb.mode.eq_ignore_ascii_case(&self.active_mode) {
                    kb.proxy.enable();
                } else {
                    kb.proxy.disable();
                }
            }
            for seat in seats.values() {
                for pb in seat.pointer_bindings.values() {
                    if pb.mode.eq_ignore_ascii_case(&self.active_mode) {
                        pb.proxy.enable();
                    } else {
                        pb.proxy.disable();
                    }
                }
            }
            self.mode_dirty = false;
        }
    }
}
