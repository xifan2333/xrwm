use super::*;

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
