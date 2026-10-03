use xrwm::state::AppState;
use xrwm::status::{format_tag_status, format_window_status};

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
