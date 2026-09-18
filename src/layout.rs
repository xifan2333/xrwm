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
    pub view_padding: u32,
    pub outer_padding: u32,
    pub monocle: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            split_ratio: 0.55,
            stack_split_ratio: 0.5,
            main_count: 1,
            main_location: MainLocation::Left,
            view_padding: 4,
            outer_padding: 4,
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

        let max_op = ((usable_area.width.min(usable_area.height) / 2).saturating_sub(1)) as i32;
        let op = (config.outer_padding as i32).clamp(0, max_op.max(0));

        // Single window with outer padding
        if count == 1 {
            let w = (usable_area.width as i32 - 2 * op).max(1) as u32;
            let h = (usable_area.height as i32 - 2 * op).max(1) as u32;
            return vec![Rect::new(usable_area.x + op, usable_area.y + op, w, h)];
        }

        let mut rects = Vec::with_capacity(count);
        let main_count = (config.main_count as usize).clamp(1, count);
        let has_stack = count > main_count;

        let inner_x = usable_area.x + op;
        let inner_y = usable_area.y + op;
        let inner_w = (usable_area.width as i32 - 2 * op).max(1);
        let inner_h = (usable_area.height as i32 - 2 * op).max(1);

        // Clamp view padding based on usable inner dimensions and window count gaps
        let max_gaps = (count as i32 - 1).max(1);
        let max_vp_w = (inner_w - count as i32).max(0) / max_gaps;
        let max_vp_h = (inner_h - count as i32).max(0) / max_gaps;
        let max_vp = max_vp_w.min(max_vp_h);
        let vp = (config.view_padding as i32).clamp(0, max_vp);

        let split_ratio = if config.split_ratio.is_finite() {
            config.split_ratio.clamp(0.1, 0.9)
        } else {
            0.55
        };
        let stack_split_ratio = if config.stack_split_ratio.is_finite() {
            config.stack_split_ratio.clamp(0.1, 0.9)
        } else {
            0.50
        };

        match config.main_location {
            MainLocation::Left | MainLocation::Right => {
                let (main_w, stack_w) = if has_stack {
                    let total_available = inner_w - vp;
                    let mw = ((total_available as f32) * split_ratio).round() as i32;
                    let sw = total_available - mw;
                    (mw.max(1), sw.max(1))
                } else {
                    (inner_w, 0)
                };

                let (master_start_x, stack_start_x) = if !has_stack {
                    (inner_x, inner_x)
                } else if config.main_location == MainLocation::Left {
                    (inner_x, inner_x + main_w + vp)
                } else {
                    (inner_x + stack_w + vp, inner_x)
                };

                // Arrange Master column
                let main_total_h = inner_h - vp * (main_count as i32 - 1);
                let main_h_step = main_total_h / (main_count as i32);
                let mut curr_y = inner_y;
                for i in 0..main_count {
                    let rem_h = (inner_y + inner_h - curr_y).max(1);
                    let h = if i == main_count - 1 {
                        rem_h as u32
                    } else {
                        main_h_step.clamp(1, rem_h) as u32
                    };
                    rects.push(Rect::new(master_start_x, curr_y, main_w as u32, h));
                    curr_y = (curr_y + h as i32 + vp).min(inner_y + inner_h);
                }

                // Arrange Stack column
                if has_stack {
                    let stack_count = count - main_count;
                    let total_stack_h = inner_h - vp * (stack_count as i32 - 1);

                    if stack_count == 1 {
                        let h = total_stack_h.max(1) as u32;
                        rects.push(Rect::new(stack_start_x, inner_y, stack_w as u32, h));
                    } else {
                        let rem_count = (stack_count - 1) as i32;
                        let first_h = ((total_stack_h as f32) * stack_split_ratio).round() as i32;
                        let max_first_h = (total_stack_h - rem_count).max(1);
                        let first_h = first_h.clamp(1, max_first_h);

                        rects.push(Rect::new(
                            stack_start_x,
                            inner_y,
                            stack_w as u32,
                            first_h as u32,
                        ));

                        let rem_total_h = total_stack_h - first_h;
                        let rem_step = rem_total_h / rem_count;
                        let mut curr_y = (inner_y + first_h + vp).min(inner_y + inner_h);

                        for i in 1..stack_count {
                            let rem_h = (inner_y + inner_h - curr_y).max(1);
                            let h = if i == stack_count - 1 {
                                rem_h as u32
                            } else {
                                rem_step.clamp(1, rem_h) as u32
                            };
                            rects.push(Rect::new(stack_start_x, curr_y, stack_w as u32, h));
                            curr_y = (curr_y + h as i32 + vp).min(inner_y + inner_h);
                        }
                    }
                }
            }
            MainLocation::Top | MainLocation::Bottom => {
                let (main_h, stack_h) = if has_stack {
                    let total_available = inner_h - vp;
                    let mh = ((total_available as f32) * split_ratio).round() as i32;
                    let sh = total_available - mh;
                    (mh.max(1), sh.max(1))
                } else {
                    (inner_h, 0)
                };

                let (master_start_y, stack_start_y) = if !has_stack {
                    (inner_y, inner_y)
                } else if config.main_location == MainLocation::Top {
                    (inner_y, inner_y + main_h + vp)
                } else {
                    (inner_y + stack_h + vp, inner_y)
                };

                // Arrange Master row
                let main_total_w = inner_w - vp * (main_count as i32 - 1);
                let main_w_step = main_total_w / (main_count as i32);
                let mut curr_x = inner_x;
                for i in 0..main_count {
                    let rem_w = (inner_x + inner_w - curr_x).max(1);
                    let w = if i == main_count - 1 {
                        rem_w as u32
                    } else {
                        main_w_step.clamp(1, rem_w) as u32
                    };
                    rects.push(Rect::new(curr_x, master_start_y, w, main_h as u32));
                    curr_x = (curr_x + w as i32 + vp).min(inner_x + inner_w);
                }

                // Arrange Stack row
                if has_stack {
                    let stack_count = count - main_count;
                    let total_stack_w = inner_w - vp * (stack_count as i32 - 1);

                    if stack_count == 1 {
                        let w = total_stack_w.max(1) as u32;
                        rects.push(Rect::new(inner_x, stack_start_y, w, stack_h as u32));
                    } else {
                        let rem_count = (stack_count - 1) as i32;
                        let first_w = ((total_stack_w as f32) * stack_split_ratio).round() as i32;
                        let max_first_w = (total_stack_w - rem_count).max(1);
                        let first_w = first_w.clamp(1, max_first_w);

                        rects.push(Rect::new(
                            inner_x,
                            stack_start_y,
                            first_w as u32,
                            stack_h as u32,
                        ));

                        let rem_total_w = total_stack_w - first_w;
                        let rem_step = rem_total_w / rem_count;
                        let mut curr_x = (inner_x + first_w + vp).min(inner_x + inner_w);

                        for i in 1..stack_count {
                            let rem_w = (inner_x + inner_w - curr_x).max(1);
                            let w = if i == stack_count - 1 {
                                rem_w as u32
                            } else {
                                rem_step.clamp(1, rem_w) as u32
                            };
                            rects.push(Rect::new(curr_x, stack_start_y, w, stack_h as u32));
                            curr_x = (curr_x + w as i32 + vp).min(inner_x + inner_w);
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
        let config = LayoutConfig {
            outer_padding: 6,
            view_padding: 4,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 1920, 1080);
        let rects = layout.arrange(area, 1, &config);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], Rect::new(6, 6, 1908, 1068));
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
            view_padding: 0,
            outer_padding: 0,
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
            view_padding: 0,
            outer_padding: 0,
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
            view_padding: 0,
            outer_padding: 0,
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
            view_padding: 0,
            outer_padding: 0,
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
            view_padding: 0,
            outer_padding: 0,
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
            view_padding: 0,
            outer_padding: 0,
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
            view_padding: 0,
            outer_padding: 0,
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

    #[test]
    fn test_secondary_stack_with_gaps() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);
        let config = LayoutConfig {
            view_padding: 10,
            outer_padding: 10,
            split_ratio: 0.5,
            stack_split_ratio: 0.5,
            ..Default::default()
        };
        let rects = layout.arrange(area, 3, &config);
        assert_eq!(rects.len(), 3);
        // Master window on left
        assert_eq!(rects[0].x, 10);
        // Stack windows on right: total height 1000 - 20 - 10 = 970 -> 485 each
        assert_eq!(rects[1].height, 485);
        assert_eq!(rects[2].height, 485);
        assert_eq!(rects[1].y, 10);
        assert_eq!(rects[2].y, 10 + 485 + 10);
    }

    #[test]
    fn test_distinct_outer_and_view_padding() {
        let layout = MasterStackLayout;
        let config = LayoutConfig {
            outer_padding: 10,
            view_padding: 4,
            split_ratio: 0.5,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 1000, 1000);
        // Total available width: 1000 - 2 * 10 = 980
        // Less view_padding between columns: 980 - 4 = 976 -> 488 each
        let rects = layout.arrange(area, 2, &config);
        assert_eq!(rects.len(), 2);
        // Master on left
        assert_eq!(rects[0], Rect::new(10, 10, 488, 980));
        // Stack on right
        assert_eq!(rects[1], Rect::new(10 + 488 + 4, 10, 488, 980));
    }

    #[test]
    fn test_secondary_stack_many_windows() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);
        // 4 windows: 1 master, 3 stack windows. First stack window gets 0.4 of total stack height (1000px -> 400px), remaining 2 share 600px -> 300px each
        let config = LayoutConfig {
            view_padding: 0,
            outer_padding: 0,
            split_ratio: 0.5,
            stack_split_ratio: 0.4,
            ..Default::default()
        };
        let rects = layout.arrange(area, 4, &config);
        assert_eq!(rects.len(), 4);
        assert_eq!(rects[1].height, 400);
        assert_eq!(rects[2].height, 300);
        assert_eq!(rects[3].height, 300);
    }

    #[test]
    fn test_excessive_padding_clamping() {
        let layout = MasterStackLayout;
        let config = LayoutConfig {
            outer_padding: 60000,
            view_padding: 60000,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 100, 100);
        let rects = layout.arrange(area, 1, &config);
        assert_eq!(rects.len(), 1);
        assert!(rects[0].x >= area.x);
        assert!(rects[0].y >= area.y);
        assert!(rects[0].x + rects[0].width as i32 <= area.x + area.width as i32);
        assert!(rects[0].y + rects[0].height as i32 <= area.y + area.height as i32);

        // Greptile case: 10 windows on 100x100 with op=49, vp=24
        let config_multi = LayoutConfig {
            outer_padding: 49,
            view_padding: 24,
            ..Default::default()
        };
        let multi_rects = layout.arrange(area, 10, &config_multi);
        assert_eq!(multi_rects.len(), 10);
        for r in multi_rects {
            assert!(r.x >= area.x);
            assert!(r.y >= area.y);
            assert!(r.x + r.width as i32 <= area.x + area.width as i32);
            assert!(r.y + r.height as i32 <= area.y + area.height as i32);
        }
    }

    #[test]
    fn test_right_and_bottom_no_stack_column_no_overflow() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);

        // Case 1: Reproduce Issue #132 (2 windows, main_count = 2, op = 0, vp = 10)
        for loc in [MainLocation::Right, MainLocation::Bottom] {
            let config = LayoutConfig {
                outer_padding: 0,
                view_padding: 10,
                main_count: 2,
                main_location: loc,
                ..Default::default()
            };
            let rects = layout.arrange(area, 2, &config);
            assert_eq!(rects.len(), 2);
            for r in &rects {
                if loc == MainLocation::Right {
                    assert_eq!(r.x, 0, "Window x must start at inner_x=0 for Right");
                    assert_eq!(r.width, 1000);
                    assert!(r.x + r.width as i32 <= 1000);
                } else {
                    assert_eq!(r.y, 0, "Window y must start at inner_y=0 for Bottom");
                    assert!(r.y + r.height as i32 <= 1000);
                }
            }
        }

        // Case 2: main_count > window count with non-zero outer_padding and view_padding
        for loc in [MainLocation::Right, MainLocation::Bottom] {
            let config = LayoutConfig {
                outer_padding: 15,
                view_padding: 10,
                main_count: 5,
                main_location: loc,
                ..Default::default()
            };
            let rects = layout.arrange(area, 3, &config);
            assert_eq!(rects.len(), 3);
            let inner_x = area.x + 15;
            let inner_y = area.y + 15;
            let inner_w = 1000 - 2 * 15;
            let inner_h = 1000 - 2 * 15;

            for r in &rects {
                assert!(r.x >= inner_x);
                assert!(r.y >= inner_y);
                assert!(r.x + r.width as i32 <= inner_x + inner_w);
                assert!(r.y + r.height as i32 <= inner_y + inner_h);
            }
        }
    }

    #[test]
    fn test_layout_defensive_against_nan_ratio() {
        let layout = MasterStackLayout;
        let area = Rect::new(0, 0, 1000, 1000);
        let config = LayoutConfig {
            split_ratio: f32::NAN,
            stack_split_ratio: f32::NAN,
            view_padding: 0,
            outer_padding: 0,
            ..Default::default()
        };
        let rects = layout.arrange(area, 3, &config);
        assert_eq!(rects.len(), 3);
        for r in &rects {
            assert!(r.width > 0);
            assert!(r.height > 0);
            assert!(r.x + r.width as i32 <= 1000);
            assert!(r.y + r.height as i32 <= 1000);
        }
    }
}
