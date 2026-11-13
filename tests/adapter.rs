//! Unit tests for CompositorAdapter proxy mapping and domain effects execution.

use wayland_backend::client::ObjectId;
use xrwm::adapter::CompositorAdapter;
use xrwm::domain::DomainEffect;
use xrwm::state::AppState;

#[test]
fn test_compositor_adapter_initial_state() {
    let adapter = CompositorAdapter::new();
    assert!(adapter.window_proxies.is_empty());
    assert!(adapter.output_proxies.is_empty());
    assert!(adapter.seat_proxies.is_empty());
    assert!(adapter.river_wm.is_none());
    assert!(adapter.broadcaster.is_empty());
}

#[test]
fn test_compositor_adapter_proxy_queries() {
    let mut adapter = CompositorAdapter::new();
    assert_eq!(adapter.window_proxy(42), None);
    assert_eq!(adapter.output_proxy(&ObjectId::null()), None);
    assert_eq!(adapter.seat_proxy(&ObjectId::null()), None);

    assert_eq!(adapter.remove_window(42), None);
    assert_eq!(adapter.remove_output(&ObjectId::null()), None);
    assert_eq!(adapter.remove_seat(&ObjectId::null()), None);
}

#[test]
fn test_compositor_adapter_effects_execution_graceful_on_missing_proxies() {
    let mut adapter = CompositorAdapter::new();
    let mut state = AppState::new();

    // Invariant: executing effects against unregistered proxies must be safe, non-panicking, and no-op.
    let effects = vec![
        DomainEffect::ProposeDimensions {
            window_id: 99,
            width: 800,
            height: 600,
        },
        DomainEffect::InformFullscreen {
            window_id: 99,
            output_id: None,
        },
        DomainEffect::InformNotFullscreen { window_id: 99 },
        DomainEffect::InformResizeStart { window_id: 99 },
        DomainEffect::InformResizeEnd { window_id: 99 },
        DomainEffect::CloseWindow { window_id: 99 },
        DomainEffect::DestroyWindow { window_id: 99 },
        DomainEffect::SetTiled {
            window_id: 99,
            tiled: true,
        },
        DomainEffect::SetDecorationMode {
            window_id: 99,
            ssd: true,
        },
        DomainEffect::SetSeatFocus {
            seat_id: ObjectId::null(),
            window_id: Some(99),
        },
        DomainEffect::SeatOpStart {
            seat_id: ObjectId::null(),
        },
        DomainEffect::SeatOpEnd {
            seat_id: ObjectId::null(),
        },
        DomainEffect::ManageFinish,
        DomainEffect::BroadcastStatus,
    ];

    adapter.execute_effects(&effects, &mut state);
}
