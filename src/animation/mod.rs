//! Animation easing, geometry interpolation, and River clip-box calculation.

use std::time::{Duration, Instant};

use crate::layout::Rect;

/// Default animation duration in milliseconds.
pub const DEFAULT_ANIMATION_DURATION_MS: u64 = 150;

/// Direction for workspace slide transition animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
    Left,
    Right,
}

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
/// window borders and decoration surfaces, so coordinates and dimensions include
/// `border_width`.
///
/// Returns `None` if the window (including its borders) has no intersection with
/// the screen/usable area.
pub fn calculate_clip_box(
    window: Rect,
    screen: Rect,
    border_width: i32,
) -> Option<(i32, i32, i32, i32)> {
    let b = border_width.max(0) as i64;

    let win_left = window.x as i64 - b;
    let win_right = window.x as i64 + window.width as i64 + b;
    let win_top = window.y as i64 - b;
    let win_bottom = window.y as i64 + window.height as i64 + b;

    let scr_left = screen.x as i64;
    let scr_right = screen.x as i64 + screen.width as i64;
    let scr_top = screen.y as i64;
    let scr_bottom = screen.y as i64 + screen.height as i64;

    let inter_left = win_left.max(scr_left);
    let inter_right = win_right.min(scr_right);
    let inter_top = win_top.max(scr_top);
    let inter_bottom = win_bottom.min(scr_bottom);

    if inter_left >= inter_right || inter_top >= inter_bottom {
        return None;
    }

    let clip_x = (inter_left - window.x as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let clip_y = (inter_top - window.y as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let clip_width = (inter_right - inter_left).clamp(0, i32::MAX as i64) as i32;
    let clip_height = (inter_bottom - inter_top).clamp(0, i32::MAX as i64) as i32;

    Some((clip_x, clip_y, clip_width, clip_height))
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
mod tests;
