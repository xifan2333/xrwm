use super::*;

#[test]
fn test_tag_masks() {
    assert_eq!(TagState::tag_index_to_mask(1), 1);
    assert_eq!(TagState::tag_index_to_mask(2), 2);
    assert_eq!(TagState::tag_index_to_mask(3), 4);
    assert_eq!(TagState::tag_index_to_mask(32), 1 << 31);
    assert_eq!(TagState::tag_index_to_mask(0), 0);
    assert_eq!(TagState::tag_index_to_mask(33), 0);
}

#[test]
fn test_tag_visibility() {
    let mut state = TagState::new();
    assert_eq!(state.focused, TAG_1);

    // View on Tag 1 is visible
    assert!(state.is_view_visible(TAG_1));
    // View on Tag 2 is not visible
    assert!(!state.is_view_visible(TagState::tag_index_to_mask(2)));

    // Multi-tag view (Tags 1 & 2 active)
    state.toggle_focused_tags(TagState::tag_index_to_mask(2));
    assert!(state.is_view_visible(TAG_1));
    assert!(state.is_view_visible(TagState::tag_index_to_mask(2)));
    assert!(!state.is_view_visible(TagState::tag_index_to_mask(3)));
}

#[test]
fn test_occupied_tags() {
    let mut state = TagState::new();
    let views = vec![
        TagState::tag_index_to_mask(1),
        TagState::tag_index_to_mask(3),
        TagState::tag_index_to_mask(1) | TagState::tag_index_to_mask(5),
    ];
    state.update_occupied_tags(&views);

    assert_eq!(state.occupied_tag_indices(), vec![1, 3, 5]);
}
