#![allow(clippy::unwrap_used)]

use xkbcommon::xkb;
use xrwm::binds::*;
use xrwm::protocol::river_seat_v1::Modifiers;

#[test]
fn test_parse_modifiers() {
    assert_eq!(parse_modifiers("None").unwrap(), Modifiers::empty());
    assert_eq!(parse_modifiers("none").unwrap(), Modifiers::empty());
    assert_eq!(parse_modifiers("").unwrap(), Modifiers::empty());

    assert_eq!(parse_modifiers("Super").unwrap(), Modifiers::Mod4);
    assert_eq!(parse_modifiers("Mod4").unwrap(), Modifiers::Mod4);
    assert_eq!(
        parse_modifiers("Super+Shift").unwrap(),
        Modifiers::Mod4 | Modifiers::Shift
    );
    assert_eq!(
        parse_modifiers("Ctrl+Alt").unwrap(),
        Modifiers::Ctrl | Modifiers::Mod1
    );
    assert_eq!(
        parse_modifiers("Super+Ctrl+Alt+Shift").unwrap(),
        Modifiers::Mod4 | Modifiers::Ctrl | Modifiers::Mod1 | Modifiers::Shift
    );

    assert!(parse_modifiers("Supr").is_err());
    assert!(parse_modifiers("Super+InvalidMod").is_err());
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

#[test]
fn test_parse_button() {
    assert_eq!(parse_button("BTN_LEFT"), Some(0x110));
    assert_eq!(parse_button("btn_left"), Some(0x110));
    assert_eq!(parse_button("left"), Some(0x110));
    assert_eq!(parse_button("BTN_RIGHT"), Some(0x111));
    assert_eq!(parse_button("BTN_MIDDLE"), Some(0x112));
    assert_eq!(parse_button("BTN_SIDE"), Some(0x113));
    assert_eq!(parse_button("BTN_EXTRA"), Some(0x114));
    assert_eq!(parse_button("0x110"), Some(0x110));
    assert_eq!(parse_button("272"), Some(272));
}

#[test]
fn test_binding_manager_mode_lifecycle() {
    let mut bm = BindingManager::new();
    assert_eq!(bm.active_mode, "normal");
    assert!(!bm.session_locked);

    // Declare mode
    assert!(bm.declare_mode("passthrough").is_ok());
    assert!(bm.modes.contains(&"passthrough".to_string()));

    // Enter mode
    assert!(bm.enter_mode("passthrough").is_ok());
    assert_eq!(bm.active_mode, "passthrough");
    assert!(bm.mode_dirty);

    // Lock session
    assert!(bm.handle_session_locked());
    assert_eq!(bm.active_mode, "locked");
    assert!(bm.session_locked);
    assert_eq!(bm.pre_lock_mode, Some("passthrough".to_string()));

    // Unlock session restores pre-lock mode
    assert!(bm.handle_session_unlocked());
    assert_eq!(bm.active_mode, "passthrough");
    assert!(!bm.session_locked);
}

#[test]
fn test_binding_manager_key_mapping() {
    let mut bm = BindingManager::new();
    bm.map_key("normal", Modifiers::Mod4, 0x71, vec!["close".to_string()]);
    assert_eq!(bm.configured_key_bindings.len(), 1);
    assert_eq!(bm.pending_key_bindings.len(), 1);

    // Duplicate replaces existing
    bm.map_key(
        "normal",
        Modifiers::Mod4,
        0x71,
        vec!["close-other".to_string()],
    );
    assert_eq!(bm.configured_key_bindings.len(), 1);
    assert_eq!(
        bm.configured_key_bindings[0].action,
        vec!["close-other".to_string()]
    );

    // Unmap removes configured and pending
    assert!(bm.unmap_key("normal", Modifiers::Mod4, 0x71));
    assert!(bm.configured_key_bindings.is_empty());
    assert!(bm.pending_key_bindings.is_empty());
}
