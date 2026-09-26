use super::*;

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
