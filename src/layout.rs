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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MainLocation {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutConfig {
    pub split_ratio: f32,
    pub stack_split_ratio: f32,
    pub main_count: u32,
    pub main_location: MainLocation,
    pub gaps: u32,
    pub monocle: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            split_ratio: 0.55,
            stack_split_ratio: 0.5,
            main_count: 1,
            main_location: MainLocation::Left,
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

        match config.main_location {
            MainLocation::Left | MainLocation::Right => {
                let (main_w, stack_w) = if has_stack {
                    let mw = ((total_w as f32) * config.split_ratio) as i32;
                    let sw = total_w - mw;
                    (mw, sw)
                } else {
                    (total_w, 0)
                };

                let (master_start_x, stack_start_x) = if config.main_location == MainLocation::Left
                {
                    (usable_area.x + gaps, usable_area.x + main_w + gaps / 2)
                } else {
                    (usable_area.x + stack_w + gaps / 2, usable_area.x + gaps)
                };

                // Arrange Master column
                let main_h_step = (total_h - gaps * (main_count as i32 + 1)) / (main_count as i32);
                for i in 0..main_count {
                    let x = master_start_x;
                    let y = usable_area.y + gaps + i as i32 * (main_h_step + gaps);
                    let w =
                        (main_w - if has_stack { gaps / 2 + gaps } else { 2 * gaps }).max(1) as u32;
                    let h = main_h_step.max(1) as u32;
                    rects.push(Rect::new(x, y, w, h));
                }

                // Arrange Stack column
                if has_stack {
                    let stack_count = count - main_count;
                    let stack_w_win = (stack_w - gaps - gaps / 2).max(1) as u32;
                    let total_stack_h = total_h - gaps * (stack_count as i32 + 1);

                    if stack_count == 1 {
                        let h = total_stack_h.max(1) as u32;
                        rects.push(Rect::new(
                            stack_start_x,
                            usable_area.y + gaps,
                            stack_w_win,
                            h,
                        ));
                    } else {
                        let rem_count = (stack_count - 1) as i32;
                        let first_h = ((total_stack_h as f32)
                            * config.stack_split_ratio.clamp(0.1, 0.9))
                        .round() as i32;
                        let max_first_h = (total_stack_h - rem_count).max(1);
                        let first_h = first_h.clamp(1, max_first_h);

                        rects.push(Rect::new(
                            stack_start_x,
                            usable_area.y + gaps,
                            stack_w_win,
                            first_h as u32,
                        ));

                        let rem_total_h = total_stack_h - first_h;
                        let rem_step = rem_total_h / rem_count;
                        let mut curr_y = usable_area.y + gaps + first_h + gaps;

                        for i in 1..stack_count {
                            let h = if i == stack_count - 1 {
                                (usable_area.y + total_h - gaps - curr_y).max(1) as u32
                            } else {
                                rem_step.max(1) as u32
                            };
                            rects.push(Rect::new(stack_start_x, curr_y, stack_w_win, h));
                            curr_y += h as i32 + gaps;
                        }
                    }
                }
            }
            MainLocation::Top | MainLocation::Bottom => {
                let (main_h, stack_h) = if has_stack {
                    let mh = ((total_h as f32) * config.split_ratio) as i32;
                    let sh = total_h - mh;
                    (mh, sh)
                } else {
                    (total_h, 0)
                };

                let (master_start_y, stack_start_y) = if config.main_location == MainLocation::Top {
                    (usable_area.y + gaps, usable_area.y + main_h + gaps / 2)
                } else {
                    (usable_area.y + stack_h + gaps / 2, usable_area.y + gaps)
                };

                // Arrange Master row
                let main_w_step = (total_w - gaps * (main_count as i32 + 1)) / (main_count as i32);
                for i in 0..main_count {
                    let x = usable_area.x + gaps + i as i32 * (main_w_step + gaps);
                    let y = master_start_y;
                    let w = main_w_step.max(1) as u32;
                    let h =
                        (main_h - if has_stack { gaps / 2 + gaps } else { 2 * gaps }).max(1) as u32;
                    rects.push(Rect::new(x, y, w, h));
                }

                // Arrange Stack row
                if has_stack {
                    let stack_count = count - main_count;
                    let stack_h_win = (stack_h - gaps - gaps / 2).max(1) as u32;
                    let total_stack_w = total_w - gaps * (stack_count as i32 + 1);

                    if stack_count == 1 {
                        let w = total_stack_w.max(1) as u32;
                        rects.push(Rect::new(
                            usable_area.x + gaps,
                            stack_start_y,
                            w,
                            stack_h_win,
                        ));
                    } else {
                        let rem_count = (stack_count - 1) as i32;
                        let first_w = ((total_stack_w as f32)
                            * config.stack_split_ratio.clamp(0.1, 0.9))
                        .round() as i32;
                        let max_first_w = (total_stack_w - rem_count).max(1);
                        let first_w = first_w.clamp(1, max_first_w);

                        rects.push(Rect::new(
                            usable_area.x + gaps,
                            stack_start_y,
                            first_w as u32,
                            stack_h_win,
                        ));

                        let rem_total_w = total_stack_w - first_w;
                        let rem_step = rem_total_w / rem_count;
                        let mut curr_x = usable_area.x + gaps + first_w + gaps;

                        for i in 1..stack_count {
                            let w = if i == stack_count - 1 {
                                (usable_area.x + total_w - gaps - curr_x).max(1) as u32
                            } else {
                                rem_step.max(1) as u32
                            };
                            rects.push(Rect::new(curr_x, stack_start_y, w, stack_h_win));
                            curr_x += w as i32 + gaps;
                        }
                    }
                }
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

    #[test]
    fn test_main_locations() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);

        // Right
        let config_right = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            main_location: MainLocation::Right,
            ..Default::default()
        };
        let r_right = layout.arrange(area, 2, &config_right);
        // Master on right (500..1000)
        assert_eq!(r_right[0].x, 500);
        // Stack on left (0..500)
        assert_eq!(r_right[1].x, 0);

        // Top
        let config_top = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            main_location: MainLocation::Top,
            ..Default::default()
        };
        let r_top = layout.arrange(area, 2, &config_top);
        // Master on top (0..500)
        assert_eq!(r_top[0].y, 0);
        assert_eq!(r_top[0].height, 500);
        // Stack on bottom (500..1000)
        assert_eq!(r_top[1].y, 500);
        assert_eq!(r_top[1].height, 500);
    }

    #[test]
    fn test_secondary_stack_split_ratio() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);

        // 3 windows: 1 master (split 0.5 -> 500px), 2 stack windows
        // default stack_split_ratio is 0.5 -> 500px each in height
        let config_default = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            stack_split_ratio: 0.5,
            ..Default::default()
        };
        let rects = layout.arrange(area, 3, &config_default);
        assert_eq!(rects.len(), 3);
        // Master
        assert_eq!(rects[0], Rect::new(0, 0, 500, 1000));
        // Stack top window: height 500
        assert_eq!(rects[1], Rect::new(500, 0, 500, 500));
        // Stack bottom window: height 500
        assert_eq!(rects[2], Rect::new(500, 500, 500, 500));

        // Adjusted stack_split_ratio = 0.7 (70% top, 30% bottom)
        let config_70 = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            stack_split_ratio: 0.7,
            ..Default::default()
        };
        let rects_70 = layout.arrange(area, 3, &config_70);
        assert_eq!(rects_70.len(), 3);
        assert_eq!(rects_70[0], Rect::new(0, 0, 500, 1000));
        assert_eq!(rects_70[1], Rect::new(500, 0, 500, 700));
        assert_eq!(rects_70[2], Rect::new(500, 700, 500, 300));

        // Adjusted stack_split_ratio = 0.3 (30% top, 70% bottom)
        let config_30 = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            stack_split_ratio: 0.3,
            ..Default::default()
        };
        let rects_30 = layout.arrange(area, 3, &config_30);
        assert_eq!(rects_30.len(), 3);
        assert_eq!(rects_30[0], Rect::new(0, 0, 500, 1000));
        assert_eq!(rects_30[1], Rect::new(500, 0, 500, 300));
        assert_eq!(rects_30[2], Rect::new(500, 300, 500, 700));
    }

    #[test]
    fn test_secondary_stack_top_bottom_location() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);

        // Top main location, 3 windows: 1 master on top (height 500), 2 stack windows on bottom
        let config = LayoutConfig {
            gaps: 0,
            split_ratio: 0.5,
            stack_split_ratio: 0.6,
            main_location: MainLocation::Top,
            ..Default::default()
        };
        let rects = layout.arrange(area, 3, &config);
        assert_eq!(rects.len(), 3);
        // Master
        assert_eq!(rects[0], Rect::new(0, 0, 1000, 500));
        // Stack left: width 600
        assert_eq!(rects[1], Rect::new(0, 500, 600, 500));
        // Stack right: width 400
        assert_eq!(rects[2], Rect::new(600, 500, 400, 500));
    }
}
