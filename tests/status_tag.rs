use xrwm::state::AppState;
use xrwm::status::{
    StatusSubscription, format_pinned_status, format_scratchpad_status, format_tag_status,
    format_window_status,
};

#[test]
fn test_format_tag_status_classes() {
    let mut state = AppState::new();
    state.tag_state.focused = 1; // Tag 1 active
    state.tag_state.occupied = 3; // Tag 1 and 2 occupied

    let t1: serde_json::Value = serde_json::from_str(&format_tag_status(&state, 1)).unwrap();
    assert_eq!(t1["text"], "1");
    assert_eq!(t1["class"], serde_json::json!(["focused", "occupied"]));

    let t2: serde_json::Value = serde_json::from_str(&format_tag_status(&state, 2)).unwrap();
    assert_eq!(t2["text"], "2");
    assert_eq!(t2["class"], serde_json::json!(["occupied"]));

    let t3: serde_json::Value = serde_json::from_str(&format_tag_status(&state, 3)).unwrap();
    assert_eq!(t3["text"], "3");
    assert_eq!(t3["class"], serde_json::json!(["empty"]));
}

#[test]
fn test_format_scratchpad_tag_status() {
    let mut state = AppState::new();
    state.tag_state.focused = 1;
    // Scratchpad (tag 32) occupied
    state.tag_state.occupied = xrwm::tag::TAG_SCRATCHPAD;

    let t32: serde_json::Value = serde_json::from_str(&format_tag_status(&state, 32)).unwrap();
    assert_eq!(t32["text"], "32");
    assert_eq!(t32["class"], serde_json::json!(["occupied"]));

    // When scratchpad drawer is toggled/focused
    state.tag_state.focused |= xrwm::tag::TAG_SCRATCHPAD;
    let t32_focused: serde_json::Value =
        serde_json::from_str(&format_tag_status(&state, 32)).unwrap();
    assert_eq!(
        t32_focused["class"],
        serde_json::json!(["focused", "occupied"])
    );

    // Regular tag 1 is focused but not occupied
    let t1: serde_json::Value = serde_json::from_str(&format_tag_status(&state, 1)).unwrap();
    assert_eq!(t1["class"], serde_json::json!(["focused"]));
}

#[test]
fn test_format_window_status() {
    let state = AppState::new();
    let win: serde_json::Value = serde_json::from_str(&format_window_status(&state)).unwrap();
    assert_eq!(win["text"], "");
    assert_eq!(win["class"], "tiled");
}

#[test]
fn test_format_scratchpad_special_status() {
    let mut state = AppState::new();

    // 1. Empty scratchpad
    let s_empty: serde_json::Value =
        serde_json::from_str(&format_scratchpad_status(&state)).unwrap();
    assert_eq!(s_empty["text"], "󰎤");
    assert_eq!(s_empty["class"], serde_json::json!(["scratchpad", "empty"]));
    assert_eq!(s_empty["count"], 0);
    assert!(s_empty["tooltip"].as_str().unwrap().contains("empty"));

    // 2. Focused scratchpad
    state.tag_state.focused |= xrwm::tag::TAG_SCRATCHPAD;
    let s_focused: serde_json::Value =
        serde_json::from_str(&format_scratchpad_status(&state)).unwrap();
    assert_eq!(
        s_focused["class"],
        serde_json::json!(["scratchpad", "focused"])
    );
}

#[test]
fn test_format_pinned_special_status() {
    let state = AppState::new();

    // Empty pinned status
    let p_empty: serde_json::Value = serde_json::from_str(&format_pinned_status(&state)).unwrap();
    assert_eq!(p_empty["text"], "");
    assert_eq!(p_empty["class"], serde_json::json!(["pinned", "empty"]));
    assert_eq!(p_empty["count"], 0);
}

#[test]
fn test_status_subscription_formats_every_target() {
    let state = AppState::new();

    assert_eq!(StatusSubscription::default(), StatusSubscription::FullJson);
    assert_eq!(
        state.format_status(StatusSubscription::FullJson),
        state.format_json_status()
    );
    assert_eq!(
        state.format_status(StatusSubscription::WaybarLegacy),
        state.format_waybar_status()
    );
    assert_eq!(
        state.format_status(StatusSubscription::Tag(1)),
        state.format_tag_status(1)
    );
    assert_eq!(
        state.format_status(StatusSubscription::Window),
        state.format_window_status()
    );
    assert_eq!(
        state.format_status(StatusSubscription::Scratchpad),
        state.format_scratchpad_status()
    );
    assert_eq!(
        state.format_status(StatusSubscription::Pinned),
        state.format_pinned_status()
    );
}

#[test]
fn test_status_subscription_validates_tag_range_and_serde() {
    for valid in [
        StatusSubscription::FullJson,
        StatusSubscription::WaybarLegacy,
        StatusSubscription::Tag(1),
        StatusSubscription::Tag(32),
        StatusSubscription::Window,
        StatusSubscription::Scratchpad,
        StatusSubscription::Pinned,
    ] {
        assert!(valid.validate().is_ok());
        let json = serde_json::to_string(&valid).unwrap();
        let decoded: StatusSubscription = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, valid);
    }

    for invalid_tag in [0u8, 33, u8::MAX] {
        assert!(StatusSubscription::Tag(invalid_tag).validate().is_err());
        let bad_json = format!(r#"{{"target":"tag","tag":{invalid_tag}}}"#);
        assert!(
            serde_json::from_str::<StatusSubscription>(&bad_json).is_err(),
            "{bad_json} must be rejected on deserialize"
        );
    }
}
