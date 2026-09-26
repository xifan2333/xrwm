use std::time::Duration;

use super::*;
use crate::layout::Rect;

#[test]
fn test_ease_out_cubic() {
    assert_eq!(ease_out_cubic(0.0), 0.0);
    assert_eq!(ease_out_cubic(1.0), 1.0);
    assert_eq!(ease_out_cubic(-0.5), 0.0);
    assert_eq!(ease_out_cubic(1.5), 1.0);
    // Halfway progress has moved substantially (> 0.5) due to ease-out deceleration
    let mid = ease_out_cubic(0.5);
    assert!(mid > 0.8 && mid < 0.9, "mid = {mid}");
}

#[test]
fn test_interpolate_scalar() {
    assert_eq!(interpolate(100, 200, 0.0), 100);
    assert_eq!(interpolate(100, 200, 1.0), 200);
    let mid = interpolate(100, 200, 0.5);
    assert!(mid > 180 && mid < 190);
}

#[test]
fn test_interpolate_rect() {
    let r1 = Rect::new(0, 0, 100, 200);
    let r2 = Rect::new(50, 50, 200, 400);

    let start = interpolate_rect(r1, r2, 0.0);
    assert_eq!(start, r1);

    let end = interpolate_rect(r1, r2, 1.0);
    assert_eq!(end, r2);
}

#[test]
fn test_calculate_clip_box() {
    let screen = Rect::new(0, 0, 1920, 1080);

    // 1. Window fully inside screen (includes 4-side symmetric border)
    let win = Rect::new(100, 100, 800, 600);
    let clip = calculate_clip_box(win, screen, 2);
    assert_eq!(clip, Some((-2, -2, 804, 604)));

    // 2. Window partially off left edge
    let win_left = Rect::new(-200, 100, 800, 600);
    let clip_left = calculate_clip_box(win_left, screen, 2);
    assert_eq!(clip_left, Some((200, -2, 602, 604)));

    // 3. Window partially off right edge
    let win_right = Rect::new(1500, 100, 800, 600);
    let clip_right = calculate_clip_box(win_right, screen, 2);
    assert_eq!(clip_right, Some((-2, -2, 422, 604)));

    // 4. Window partially off top edge
    let win_top = Rect::new(100, -200, 800, 600);
    let clip_top = calculate_clip_box(win_top, screen, 2);
    assert_eq!(clip_top, Some((-2, 200, 804, 402)));

    // 5. Window partially off bottom edge
    let win_bottom = Rect::new(100, 800, 800, 600);
    let clip_bottom = calculate_clip_box(win_bottom, screen, 2);
    assert_eq!(clip_bottom, Some((-2, -2, 804, 282)));

    // 6. Window covering the entire screen
    let win_oversized = Rect::new(-100, -100, 2120, 1280);
    let clip_oversized = calculate_clip_box(win_oversized, screen, 2);
    assert_eq!(clip_oversized, Some((100, 100, 1920, 1080)));

    // 7. Window completely outside (no intersection) - right edge
    let win_out_right = Rect::new(1922, 0, 800, 600);
    assert_eq!(calculate_clip_box(win_out_right, screen, 2), None);

    // 8. Window completely outside (no intersection) - left edge
    let win_out_left = Rect::new(-802, 0, 800, 600);
    assert_eq!(calculate_clip_box(win_out_left, screen, 2), None);

    // 9. Window completely outside (no intersection) - top & bottom
    let win_out_top = Rect::new(100, -602, 800, 600);
    assert_eq!(calculate_clip_box(win_out_top, screen, 2), None);

    let win_out_bottom = Rect::new(100, 1082, 800, 600);
    assert_eq!(calculate_clip_box(win_out_bottom, screen, 2), None);

    // 10. Equal boundary (zero-width intersection touching outer edge)
    let win_touch_right = Rect::new(1920, 0, 800, 600);
    assert_eq!(calculate_clip_box(win_touch_right, screen, 0), None);

    let win_touch_left = Rect::new(-800, 0, 800, 600);
    assert_eq!(calculate_clip_box(win_touch_left, screen, 0), None);

    // 11. Large border width (i32::MAX) does not overflow or panic
    let clip_large_b = calculate_clip_box(win, screen, i32::MAX);
    assert!(clip_large_b.is_some());

    // 12. Extreme coordinates near i32 limits do not overflow or panic
    let win_extreme = Rect::new(i32::MAX - 100, i32::MAX - 100, 800, 600);
    assert_eq!(calculate_clip_box(win_extreme, screen, 2), None);

    let win_extreme_min = Rect::new(i32::MIN, i32::MIN, 800, 600);
    assert_eq!(calculate_clip_box(win_extreme_min, screen, 2), None);
}

#[test]
fn test_animation_controller() {
    let mut ctrl = AnimationController::new(true, 50);
    assert!(!ctrl.is_animating());
    assert_eq!(ctrl.progress(), 1.0);

    ctrl.start();
    assert!(ctrl.is_animating());
    assert!(ctrl.progress() >= 0.0 && ctrl.progress() <= 1.0);

    std::thread::sleep(Duration::from_millis(60));
    assert!(!ctrl.is_animating());
    assert_eq!(ctrl.progress(), 1.0);

    ctrl.start();
    ctrl.stop();
    assert!(!ctrl.is_animating());
}
