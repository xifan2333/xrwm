#![allow(clippy::unwrap_used)]

use xrwm::seat::*;

#[test]
fn test_cursor_warp_parse() {
    assert_eq!(CursorWarp::parse("disabled").unwrap(), CursorWarp::Disabled);
    assert_eq!(
        CursorWarp::parse("on-output-change").unwrap(),
        CursorWarp::OnOutputChange
    );
    assert_eq!(
        CursorWarp::parse("on-focus-change").unwrap(),
        CursorWarp::OnFocusChange
    );
    assert_eq!(
        CursorWarp::parse("output").unwrap(),
        CursorWarp::OnOutputChange
    );
    assert!(CursorWarp::parse("invalid").is_err());
}

#[test]
fn test_focus_follows_cursor_parse() {
    assert_eq!(
        FocusFollowsCursor::parse("disabled").unwrap(),
        FocusFollowsCursor::Disabled
    );
    assert_eq!(
        FocusFollowsCursor::parse("normal").unwrap(),
        FocusFollowsCursor::Normal
    );
    assert_eq!(
        FocusFollowsCursor::parse("always").unwrap(),
        FocusFollowsCursor::Always
    );
    assert!(FocusFollowsCursor::parse("invalid").is_err());
}

#[test]
fn test_seat_gesture_state_invariants() {
    let idle = SeatGestureState::default();
    assert!(idle.is_idle());
    assert!(!idle.is_active());
    assert_eq!(idle.target_proxy(), None);

    let tiled_resize = SeatGestureState::TiledResize {
        start_ratio: 0.5,
        dx: 15,
    };
    assert!(!tiled_resize.is_idle());
    assert!(tiled_resize.is_active());
    assert_eq!(tiled_resize.target_proxy(), None);

    let tiled_stack = SeatGestureState::TiledStackResize {
        start_ratio: 0.5,
        dy: -10,
    };
    assert!(tiled_stack.is_active());
    assert_eq!(tiled_stack.target_proxy(), None);
}
