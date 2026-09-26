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
    assert_eq!(rects, vec![area; 3]);
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

    // Main on Top
    let config_top = LayoutConfig {
        main_location: MainLocation::Top,
        split_ratio: 0.6,
        view_padding: 0,
        outer_padding: 0,
        ..Default::default()
    };
    let rects = layout.arrange(area, 2, &config_top);
    assert_eq!(rects[0], Rect::new(0, 0, 1000, 600));
    assert_eq!(rects[1], Rect::new(0, 600, 1000, 400));

    // Main on Bottom
    let config_bottom = LayoutConfig {
        main_location: MainLocation::Bottom,
        split_ratio: 0.6,
        view_padding: 0,
        outer_padding: 0,
        ..Default::default()
    };
    let rects = layout.arrange(area, 2, &config_bottom);
    assert_eq!(rects[0], Rect::new(0, 400, 1000, 600));
    assert_eq!(rects[1], Rect::new(0, 0, 1000, 400));

    // Main on Right
    let config_right = LayoutConfig {
        main_location: MainLocation::Right,
        split_ratio: 0.7,
        view_padding: 0,
        outer_padding: 0,
        ..Default::default()
    };
    let rects = layout.arrange(area, 2, &config_right);
    assert_eq!(rects[0], Rect::new(300, 0, 700, 1000));
    assert_eq!(rects[1], Rect::new(0, 0, 300, 1000));
}

#[test]
fn test_secondary_stack_split_ratio() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config = LayoutConfig {
        split_ratio: 0.5,
        stack_split_ratio: 0.7,
        view_padding: 0,
        outer_padding: 0,
        ..Default::default()
    };

    let rects = layout.arrange(area, 3, &config);
    assert_eq!(rects.len(), 3);
    assert_eq!(rects[0], Rect::new(0, 0, 500, 1000));
    assert_eq!(rects[1], Rect::new(500, 0, 500, 700));
    assert_eq!(rects[2], Rect::new(500, 700, 500, 300));
}

#[test]
fn test_secondary_stack_top_bottom_location() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config = LayoutConfig {
        main_location: MainLocation::Top,
        split_ratio: 0.5,
        stack_split_ratio: 0.6,
        view_padding: 0,
        outer_padding: 0,
        ..Default::default()
    };

    let rects = layout.arrange(area, 3, &config);
    assert_eq!(rects[0], Rect::new(0, 0, 1000, 500));
    assert_eq!(rects[1], Rect::new(0, 500, 600, 500));
    assert_eq!(rects[2], Rect::new(600, 500, 400, 500));
}

#[test]
fn test_secondary_stack_with_gaps() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config = LayoutConfig {
        split_ratio: 0.5,
        stack_split_ratio: 0.5,
        view_padding: 10,
        outer_padding: 10,
        ..Default::default()
    };

    let rects = layout.arrange(area, 3, &config);
    assert_eq!(rects.len(), 3);
    assert_eq!(rects[0], Rect::new(10, 10, 485, 980));
    assert_eq!(rects[1], Rect::new(505, 10, 485, 485));
    assert_eq!(rects[2], Rect::new(505, 505, 485, 485));
}

#[test]
fn test_secondary_stack_many_windows() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config = LayoutConfig {
        split_ratio: 0.5,
        stack_split_ratio: 0.4,
        view_padding: 0,
        outer_padding: 0,
        ..Default::default()
    };

    let rects = layout.arrange(area, 4, &config);
    assert_eq!(rects.len(), 4);
    assert_eq!(rects[0], Rect::new(0, 0, 500, 1000));
    assert_eq!(rects[1], Rect::new(500, 0, 500, 400));
    assert_eq!(rects[2], Rect::new(500, 400, 500, 300));
    assert_eq!(rects[3], Rect::new(500, 700, 500, 300));
}

#[test]
fn test_excessive_padding_clamping() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 100, 100);

    let config = LayoutConfig {
        view_padding: 200,
        outer_padding: 200,
        ..Default::default()
    };

    let rects = layout.arrange(area, 3, &config);
    assert_eq!(rects.len(), 3);
    for r in &rects {
        assert!(r.width >= 1);
        assert!(r.height >= 1);
    }
}

#[test]
fn test_layout_defensive_against_nan_ratio() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config = LayoutConfig {
        split_ratio: f32::NAN,
        stack_split_ratio: f32::NAN,
        ..Default::default()
    };

    let rects = layout.arrange(area, 3, &config);
    assert_eq!(rects.len(), 3);
    for r in &rects {
        assert!(r.width >= 1);
        assert!(r.height >= 1);
    }
}

#[test]
fn test_distinct_outer_and_view_padding() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config = LayoutConfig {
        outer_padding: 20,
        view_padding: 10,
        split_ratio: 0.5,
        stack_split_ratio: 0.5,
        ..Default::default()
    };

    let rects = layout.arrange(area, 2, &config);
    assert_eq!(rects.len(), 2);

    assert_eq!(rects[0], Rect::new(20, 20, 475, 960));
    assert_eq!(rects[1], Rect::new(505, 20, 475, 960));
}

#[test]
fn test_right_and_bottom_no_stack_column_no_overflow() {
    let layout = MasterStackLayout;
    let area = Rect::new(0, 0, 1000, 1000);

    let config_right = LayoutConfig {
        main_location: MainLocation::Right,
        main_count: 3,
        view_padding: 10,
        outer_padding: 10,
        ..Default::default()
    };
    let rects = layout.arrange(area, 2, &config_right);
    assert_eq!(rects.len(), 2);
    for r in &rects {
        assert_eq!(r.x, 10);
        assert_eq!(r.width, 980);
    }

    let config_bottom = LayoutConfig {
        main_location: MainLocation::Bottom,
        main_count: 3,
        view_padding: 10,
        outer_padding: 10,
        ..Default::default()
    };
    let rects_bottom = layout.arrange(area, 2, &config_bottom);
    assert_eq!(rects_bottom.len(), 2);
    for r in &rects_bottom {
        assert_eq!(r.y, 10);
        assert_eq!(r.height, 980);
    }
}
