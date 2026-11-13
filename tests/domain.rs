//! Unit tests for pure in-memory DomainModel, WindowModel, and DomainEffect commands.

use wayland_backend::client::ObjectId;
use xrwm::domain::{
    BindingModel, DomainEffect, DomainModel, OutputModel, SeatModel, SeatOpModel, WindowModel,
};
use xrwm::layout::Rect;
use xrwm::protocol::river_window_v1::Edges;
use xrwm::rule::WindowRule;
use xrwm::seat::{LayerShellFocus, PointerAction};
use xrwm::state::{AppState, AttachMode, WindowLayer, WindowVisibility};
use xrwm::tag::{TAG_ALL, TagState};

#[test]
fn test_domain_model_initial_state() {
    let domain = DomainModel::new();
    assert!(domain.windows.is_empty());
    assert!(domain.outputs.is_empty());
    assert!(domain.seats.is_empty());
    assert_eq!(domain.next_view_id, 1);
    assert_eq!(domain.tag_state.focused, 1);
    assert_eq!(domain.tag_state.occupied, 0);
    assert_eq!(domain.border_width, 2);
    assert_eq!(domain.attach_mode, AttachMode::Top);
}

#[test]
fn test_window_model_layer_and_propose_dimensions() {
    let mut win = WindowModel::new(1, 1, None);
    assert_eq!(win.layer(), WindowLayer::Tiled);
    assert!(win.layer_policy().tiled_layout);
    assert!(!win.layer_policy().collapsible);

    win.floating = true;
    assert_eq!(win.layer(), WindowLayer::WorkspaceFloating);
    assert!(!win.layer_policy().tiled_layout);
    assert!(win.layer_policy().collapsible);

    win.tags = 1 << 31;
    assert_eq!(win.layer(), WindowLayer::Scratchpad);

    win.tags = TAG_ALL;
    assert_eq!(win.layer(), WindowLayer::Pinned);

    win.fullscreen = true;
    assert_eq!(win.layer(), WindowLayer::Fullscreen);

    // Initial dimension proposal generates an effect
    let eff1 = win.propose_dimensions(800, 600);
    assert_eq!(
        eff1,
        Some(DomainEffect::ProposeDimensions {
            window_id: 1,
            width: 800,
            height: 600,
        })
    );

    // Duplicate dimension proposal is suppressed (invariant)
    let eff2 = win.propose_dimensions(800, 600);
    assert_eq!(eff2, None);

    // Changed dimensions produce an effect
    let eff3 = win.propose_dimensions(1024, 768);
    assert_eq!(
        eff3,
        Some(DomainEffect::ProposeDimensions {
            window_id: 1,
            width: 1024,
            height: 768,
        })
    );
}

#[test]
fn test_attach_window_with_rules_and_effects() {
    let mut domain = DomainModel::new();
    domain.rules.push(WindowRule {
        app_id: Some("floating-term".to_string()),
        title: None,
        float: Some(true),
        ssd: Some(true),
        tags: Some(2),
        dimensions: Some((640, 480)),
        position: Some((100, 100)),
        fullscreen: None,
        output: None,
    });

    let mut win = WindowModel::new(42, 1, None);
    win.app_id = Some("floating-term".to_string());

    let effects = domain.attach_window(win);

    // Check effects
    assert!(effects.contains(&DomainEffect::SetDecorationMode {
        window_id: 42,
        ssd: true,
    }));

    // Check state mutations
    assert_eq!(domain.windows.len(), 1);
    let attached = &domain.windows[0];
    assert_eq!(attached.id, 42);
    assert!(attached.floating);
    assert_eq!(attached.tags, 2);
    assert_eq!(attached.width, 640);
    assert_eq!(attached.height, 480);
    assert_eq!(attached.x, 100);
    assert_eq!(attached.y, 100);
}

#[test]
fn test_visibility_resolution_and_floating_collapse() {
    let mut domain = DomainModel::new();
    domain.tag_state = TagState {
        focused: 1,
        occupied: 1,
        urgent: 0,
    };

    let mut tiled = WindowModel::new(1, 1, None);
    assert_eq!(
        domain.resolve_window_visibility(&tiled),
        WindowVisibility::Visible
    );

    // Hidden on other tag
    tiled.tags = 2;
    assert_eq!(
        domain.resolve_window_visibility(&tiled),
        WindowVisibility::HiddenByTags
    );
    assert!(!domain.is_window_visible(&tiled));
}

#[test]
fn test_remove_window_effects() {
    let mut domain = DomainModel::new();
    let win = WindowModel::new(10, 1, None);
    domain.attach_window(win);
    assert_eq!(domain.windows.len(), 1);

    let effects = domain.remove_window(10);
    assert_eq!(effects, vec![DomainEffect::DestroyWindow { window_id: 10 }]);
    assert!(domain.windows.is_empty());
}

#[test]
fn test_binding_model_lifecycle() {
    let mut bm = BindingModel::new();
    assert_eq!(bm.active_mode, "normal");
    assert!(!bm.session_locked);

    bm.session_locked = true;
    bm.active_mode = "locked".to_string();
    assert!(bm.session_locked);
    assert_eq!(bm.active_mode, "locked");
}

#[test]
fn test_seat_model_and_seat_op_model_invariants() {
    let op = SeatOpModel::Resize {
        window_id: 7,
        start_x: 50,
        start_y: 60,
        start_width: 800,
        start_height: 600,
        edges: Edges::all(),
    };
    assert!(matches!(op, SeatOpModel::Resize { window_id: 7, .. }));

    let seat = SeatModel {
        id: ObjectId::null(),
        focused_window: Some(7),
        hovered_window: None,
        op,
        pending_action: PointerAction::None,
        layer_focus: LayerShellFocus::None,
    };
    assert_eq!(seat.focused_window, Some(7));
    assert_eq!(seat.layer_focus, LayerShellFocus::None);
}

#[test]
fn test_output_model_invariants() {
    let out = OutputModel {
        id: ObjectId::null(),
        name: Some("DP-1".to_string()),
        removed: false,
        usable_area: Rect::new(0, 0, 1920, 1080),
        has_custom_usable_area: false,
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        tag_state: TagState::new(),
        previous_focused_tags: 1,
        monocle: false,
        floating_hidden_tags: 0,
        capture_sessions: 0,
    };
    assert_eq!(out.name.as_deref(), Some("DP-1"));
    assert_eq!(out.usable_area.width, 1920);
}

#[test]
fn test_domain_model_sync_round_trip() {
    let mut state = AppState::new();
    state.next_view_id = 15;
    state.tag_state.focused = 4;
    state.border_width = 3;

    // Convert to domain
    let domain = DomainModel::from_app_state(&state);
    assert_eq!(domain.next_view_id, 15);
    assert_eq!(domain.tag_state.focused, 4);
    assert_eq!(domain.border_width, 3);

    // Sync back
    let mut state2 = AppState::new();
    domain.sync_to_app_state(&mut state2);
    assert_eq!(state2.next_view_id, 15);
    assert_eq!(state2.tag_state.focused, 4);
    assert_eq!(state2.border_width, 3);
}
