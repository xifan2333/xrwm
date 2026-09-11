//! Dynamic tiling layout engine for xrwm (rivertile master-stack style).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutConfig {
    pub split_ratio: f32,
    pub main_count: u32,
    pub gaps: u32,
    pub monocle: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            split_ratio: 0.55,
            main_count: 1,
            gaps: 4,
            monocle: false,
        }
    }
}

pub trait Layout: Send + Sync {
    fn name(&self) -> &'static str;
    fn arrange(&self, usable_area: Rect, count: usize, config: &LayoutConfig) -> Vec<Rect>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MasterStackLayout;

impl Layout for MasterStackLayout {
    fn name(&self) -> &'static str {
        "master-stack"
    }

    fn arrange(&self, usable_area: Rect, count: usize, config: &LayoutConfig) -> Vec<Rect> {
        if count == 0 {
            return Vec::new();
        }

        // Monocle mode fills the entire usable area
        if config.monocle {
            return vec![usable_area];
        }

        let gaps = config.gaps as i32;

        // Single window with gaps enabled
        if count == 1 {
            let w = (usable_area.width as i32 - 2 * gaps).max(1) as u32;
            let h = (usable_area.height as i32 - 2 * gaps).max(1) as u32;
            return vec![Rect::new(usable_area.x + gaps, usable_area.y + gaps, w, h)];
        }

        let mut rects = Vec::with_capacity(count);
        let main_count = (config.main_count as usize).clamp(1, count);
        let has_stack = count > main_count;

        let total_w = usable_area.width as i32;
        let total_h = usable_area.height as i32;

        let (main_w, stack_w) = if has_stack {
            let mw = ((total_w as f32) * config.split_ratio) as i32;
            let sw = total_w - mw;
            (mw, sw)
        } else {
            (total_w, 0)
        };

        // 1. Arrange Master column
        let main_h_step = (total_h - gaps * (main_count as i32 + 1)) / (main_count as i32);
        for i in 0..main_count {
            let x = usable_area.x + gaps;
            let y = usable_area.y + gaps + i as i32 * (main_h_step + gaps);
            let w = (main_w - if has_stack { gaps / 2 + gaps } else { 2 * gaps }).max(1) as u32;
            let h = main_h_step.max(1) as u32;
            rects.push(Rect::new(x, y, w, h));
        }

        // 2. Arrange Stack column
        if has_stack {
            let stack_count = count - main_count;
            let stack_h_step = (total_h - gaps * (stack_count as i32 + 1)) / (stack_count as i32);
            let start_x = usable_area.x + main_w + gaps / 2;

            for i in 0..stack_count {
                let x = start_x;
                let y = usable_area.y + gaps + i as i32 * (stack_h_step + gaps);
                let w = (stack_w - gaps - gaps / 2).max(1) as u32;
                let h = stack_h_step.max(1) as u32;
                rects.push(Rect::new(x, y, w, h));
            }
        }

        rects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_layout() {
        let layout = MasterStackLayout;
        let config = LayoutConfig::default();
        let area = Rect::new(0, 0, 1920, 1080);
        assert_eq!(layout.arrange(area, 0, &config), vec![]);
    }

    #[test]
    fn test_single_window_gaps() {
        let layout = MasterStackLayout;
        let config = LayoutConfig::default(); // gaps is 4
        let area = Rect::new(0, 0, 1920, 1080);
        let rects = layout.arrange(area, 1, &config);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], Rect::new(4, 4, 1912, 1072));
    }

    #[test]
    fn test_monocle_mode() {
        let layout = MasterStackLayout;
        let config = LayoutConfig {
            monocle: true,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 1920, 1080);
        let rects = layout.arrange(area, 3, &config);
        assert_eq!(rects, vec![area]);
    }

    #[test]
    fn test_master_stack_tiling() {
        let layout = MasterStackLayout;
        let config = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 1000, 1000);

        let rects = layout.arrange(area, 2, &config);
        assert_eq!(rects.len(), 2);
        // Master window on left (0..500)
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[0].width, 500);
        assert_eq!(rects[0].height, 1000);

        // Stack window on right (500..1000)
        assert_eq!(rects[1].x, 500);
        assert_eq!(rects[1].width, 500);
        assert_eq!(rects[1].height, 1000);
    }
}
