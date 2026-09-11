//! Animation easing, geometry interpolation, and River clip-box calculation.

use std::time::{Duration, Instant};

use crate::layout::Rect;

/// Default animation duration in milliseconds.
pub const DEFAULT_ANIMATION_DURATION_MS: u64 = 150;

/// Cubic Ease-Out curve: `1 - (1 - p)^3`.
///
/// Fast initial burst, deceleration near the end, producing a natural physical feel.
#[inline]
pub fn ease_out_cubic(progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(3)
}

/// Interpolates between `start` and `end` using `ease_out_cubic(progress)`.
#[inline]
pub fn interpolate(start: i32, end: i32, progress: f32) -> i32 {
    let eased = ease_out_cubic(progress);
    start + ((end - start) as f32 * eased).round() as i32
}

/// Interpolates a `Rect` geometry between `start` and `end`.
pub fn interpolate_rect(start: Rect, end: Rect, progress: f32) -> Rect {
    let x = interpolate(start.x, end.x, progress);
    let y = interpolate(start.y, end.y, progress);
    let width = interpolate(start.width as i32, end.width as i32, progress).max(1) as u32;
    let height = interpolate(start.height as i32, end.height as i32, progress).max(1) as u32;
    Rect::new(x, y, width, height)
}

/// Calculates River 0.4 clip-box `(clip_x, clip_y, clip_width, clip_height)`.
///
/// Clips windows that slide outside the screen/usable boundary during workspace
/// animations to prevent them from bleeding into neighboring displays or panels.
/// River's `set_clip_box` is relative to the window content area, but also clips
/// window borders, so `clip_x` and `clip_y` are adjusted by `border_width`.
pub fn calculate_clip_box(window: Rect, screen: Rect, border_width: i32) -> (i32, i32, i32, i32) {
    let win_left = window.x;
    let win_right = window.x + window.width as i32;
    let win_top = window.y;
    let win_bottom = window.y + window.height as i32;

    let scr_left = screen.x;
    let scr_right = screen.x + screen.width as i32;
    let scr_top = screen.y;
    let scr_bottom = screen.y + screen.height as i32;

    let mut clip_x = 0;
    let mut clip_y = 0;
    let mut clip_width = window.width as i32;
    let mut clip_height = window.height as i32;

    // Horizontal clipping
    if scr_left > win_left && scr_left < win_right {
        clip_x = scr_left - win_left;
        clip_width = (win_right - scr_left).min(screen.width as i32);
    } else if scr_right > win_left && scr_right < win_right {
        clip_width = scr_right - win_left;
    }

    // Vertical clipping
    if scr_top > win_top && scr_top < win_bottom {
        clip_y = scr_top - win_top;
        clip_height = (win_bottom - scr_top).min(screen.height as i32);
    } else if scr_bottom > win_top && scr_bottom < win_bottom {
        clip_height = scr_bottom - win_top;
    }

    (
        clip_x - border_width,
        clip_y - border_width,
        clip_width.max(0),
        clip_height.max(0),
    )
}

/// Animation controller managing duration and elapsed progress.
#[derive(Debug, Clone)]
pub struct AnimationController {
    pub enabled: bool,
    pub duration: Duration,
    pub start_time: Option<Instant>,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self {
            enabled: true,
            duration: Duration::from_millis(DEFAULT_ANIMATION_DURATION_MS),
            start_time: None,
        }
    }
}

impl AnimationController {
    pub fn new(enabled: bool, duration_ms: u64) -> Self {
        Self {
            enabled,
            duration: Duration::from_millis(duration_ms),
            start_time: None,
        }
    }

    pub fn start(&mut self) {
        if self.enabled {
            self.start_time = Some(Instant::now());
        }
    }

    pub fn stop(&mut self) {
        self.start_time = None;
    }

    pub fn is_animating(&self) -> bool {
        if !self.enabled {
            return false;
        }
        match self.start_time {
            Some(t) => t.elapsed() < self.duration,
            None => false,
        }
    }

    /// Returns the progress ratio in `[0.0, 1.0]`. Returns 1.0 if not animating.
    pub fn progress(&self) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        match self.start_time {
            Some(t) => {
                let elapsed_ms = t.elapsed().as_secs_f32() * 1000.0;
                let dur_ms = self.duration.as_secs_f32() * 1000.0;
                if dur_ms <= 0.0 {
                    1.0
                } else {
                    (elapsed_ms / dur_ms).clamp(0.0, 1.0)
                }
            }
            None => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Window fully inside screen
        let win = Rect::new(100, 100, 800, 600);
        let (cx, cy, cw, ch) = calculate_clip_box(win, screen, 2);
        assert_eq!((cx, cy, cw, ch), (-2, -2, 800, 600));

        // Window partially off left edge
        let win_left = Rect::new(-200, 100, 800, 600);
        let (cx, cy, cw, ch) = calculate_clip_box(win_left, screen, 2);
        assert_eq!(cx, 200 - 2);
        assert_eq!(cy, -2);
        assert_eq!(cw, 600);
        assert_eq!(ch, 600);
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
}
