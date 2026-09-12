//! Wayland event dispatch handlers for AppState.

use wayland_backend::client::ObjectId;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::layout::Rect;
use crate::protocol::{
    river_layer_shell_output_v1::RiverLayerShellOutputV1,
    river_layer_shell_v1::RiverLayerShellV1,
    river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1,
    river_pointer_binding_v1::RiverPointerBindingV1,
    river_seat_v1::{Modifiers, RiverSeatV1},
    river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::RiverWindowV1,
    river_xkb_bindings_v1::RiverXkbBindingsV1,
};
use crate::wm::seat::{PointerAction, PointerBinding, SeatItem};
use crate::wm::state::{AppState, OutputItem, WindowItem};

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "river_window_manager_v1" => {
                    let wm =
                        registry.bind::<RiverWindowManagerV1, _, _>(name, version.min(4), qh, ());
                    state.river_wm = Some(wm);
                }
                "river_xkb_bindings_v1" => {
                    let xkb =
                        registry.bind::<RiverXkbBindingsV1, _, _>(name, version.min(2), qh, ());
                    state.river_xkb = Some(xkb);
                }
                "river_layer_shell_v1" => {
                    let layer =
                        registry.bind::<RiverLayerShellV1, _, _>(name, version.min(1), qh, ());
                    state.river_layer = Some(layer);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<RiverWindowManagerV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &RiverWindowManagerV1,
        event: <RiverWindowManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_manager_v1::Event;
        match event {
            Event::Unavailable => {
                eprintln!(
                    "Error: Another window manager is already running on this river compositor."
                );
                std::process::exit(1);
            }
            Event::Finished => {
                state.should_exit = true;
            }
            Event::ManageStart => {
                state.handle_manage_start(proxy, qh);
            }
            Event::RenderStart => {
                state.handle_render_start(proxy);
            }
            Event::SessionLocked => {}
            Event::SessionUnlocked => {}
            Event::Window { id } => {
                let node = id.get_node(qh, ());
                id.use_ssd();
                let vid = state.next_view_id;
                state.next_view_id += 1;
                let current_tags = state.tag_state.focused;

                state.windows.push(WindowItem {
                    id: vid,
                    proxy: id,
                    node,
                    new: true,
                    closed: false,
                    app_id: None,
                    title: None,
                    tags: current_tags,
                    floating: false,
                    fullscreen: false,
                    pending_close: false,
                    pending_fullscreen_change: false,
                    float_geo: None,
                    ssd: true,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    visual_geo: None,
                    anim_start_geo: None,
                    anim_target_geo: None,
                    last_proposed_w: 0,
                    last_proposed_h: 0,
                });
            }
            Event::Output { id } => {
                let ls_out = state
                    .river_layer
                    .as_ref()
                    .map(|ls| ls.get_output(&id, qh, id.id()));

                state.outputs.insert(
                    id.id(),
                    OutputItem {
                        proxy: id,
                        ls_output: ls_out,
                        removed: false,
                        usable_area: Rect::new(0, 30, 1280, 770),
                    },
                );
            }
            Event::Seat { id } => {
                let mut seat = SeatItem::new(id.clone());

                // Preset pointer bindings (Mod4 = Super):
                //   Super+LMB    drag a floating window
                //   Super+RMB    resize a floating window
                //   Super+MMB    toggle floating <-> tiled
                const BTN_LEFT: u32 = 0x110;
                const BTN_RIGHT: u32 = 0x111;
                const BTN_MIDDLE: u32 = 0x112;

                let pb_move = id.get_pointer_binding(BTN_LEFT, Modifiers::Mod4, qh, id.id());
                seat.pointer_bindings.insert(
                    pb_move.id(),
                    PointerBinding {
                        proxy: pb_move,
                        action: PointerAction::Move,
                    },
                );

                let pb_resize = id.get_pointer_binding(BTN_RIGHT, Modifiers::Mod4, qh, id.id());
                seat.pointer_bindings.insert(
                    pb_resize.id(),
                    PointerBinding {
                        proxy: pb_resize,
                        action: PointerAction::Resize,
                    },
                );

                let pb_toggle = id.get_pointer_binding(BTN_MIDDLE, Modifiers::Mod4, qh, id.id());
                seat.pointer_bindings.insert(
                    pb_toggle.id(),
                    PointerBinding {
                        proxy: pb_toggle,
                        action: PointerAction::ToggleFloating,
                    },
                );

                state.seats.insert(id.id(), seat);
            }
        }
    }

    wayland_client::event_created_child!(AppState, RiverWindowManagerV1, [
        crate::protocol::river_window_manager_v1::EVT_WINDOW_OPCODE => (RiverWindowV1, ()),
        crate::protocol::river_window_manager_v1::EVT_OUTPUT_OPCODE => (RiverOutputV1, ()),
        crate::protocol::river_window_manager_v1::EVT_SEAT_OPCODE => (RiverSeatV1, ())
    ]);
}

impl Dispatch<RiverWindowV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &RiverWindowV1,
        event: <RiverWindowV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_v1::Event;
        match event {
            Event::Closed => {
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.closed = true;
                }
                state.manage_dirty();
            }
            Event::AppId { app_id } => {
                let usable = state.outputs.values().next().map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.app_id = app_id;
                    AppState::apply_rules_to_window(&state.rules, w, usable);
                }
            }
            Event::Title { title } => {
                let usable = state.outputs.values().next().map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.title = title;
                    AppState::apply_rules_to_window(&state.rules, w, usable);
                }
            }
            Event::Dimensions { .. } => {}
            _ => {}
        }
    }
}

impl Dispatch<RiverOutputV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &RiverOutputV1,
        event: <RiverOutputV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_output_v1::Event;
        if let Event::Removed = event {
            state.outputs.remove(&proxy.id());
        }
    }
}

impl Dispatch<RiverLayerShellOutputV1, ObjectId> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &RiverLayerShellOutputV1,
        event: <RiverLayerShellOutputV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_layer_shell_output_v1::Event;
        let Event::NonExclusiveArea {
            x,
            y,
            width,
            height,
        } = event;
        if let Some(out) = state.outputs.get_mut(data) {
            out.usable_area = Rect::new(x, y, width.max(0) as u32, height.max(0) as u32);
        }
    }
}

impl Dispatch<RiverSeatV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &RiverSeatV1,
        event: <RiverSeatV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_seat_v1::Event;
        match event {
            Event::PointerEnter { window } => {
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.hovered = Some(window.clone());
                    seat.focused = Some(window);
                }
                state.manage_dirty();
            }
            Event::PointerLeave => {
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.hovered = None;
                }
                state.manage_dirty();
            }
            Event::WindowInteraction { window } => {
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.interacted = Some(window);
                }
                state.manage_dirty();
            }
            Event::OpDelta { dx, dy } => {
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.op_dx = dx;
                    seat.op_dy = dy;
                }
                state.manage_dirty();
            }
            Event::OpRelease => {
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.op_release = true;
                }
                state.manage_dirty();
            }
            Event::PointerPosition { x, y } => {
                state.pointer = (x, y);
            }
            Event::ShellSurfaceInteraction { .. } | Event::WlSeat { .. } => {}
            Event::Removed => {
                state.seats.remove(&proxy.id());
            }
        }
    }
}

impl Dispatch<RiverPointerBindingV1, ObjectId> for AppState {
    fn event(
        state: &mut Self,
        proxy: &RiverPointerBindingV1,
        event: <RiverPointerBindingV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_pointer_binding_v1::Event;
        if let Event::Pressed = event {
            if let Some(seat) = state.seats.get_mut(data)
                && let Some(binding) = seat.pointer_bindings.get(&proxy.id())
            {
                seat.pending_action = binding.action;
            }
            state.manage_dirty();
        }
    }
}

wayland_client::delegate_noop!(AppState: ignore RiverLayerShellV1);
wayland_client::delegate_noop!(AppState: ignore RiverXkbBindingsV1);
wayland_client::delegate_noop!(AppState: ignore RiverNodeV1);

impl Dispatch<crate::protocol::river_xkb_binding_v1::RiverXkbBindingV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &crate::protocol::river_xkb_binding_v1::RiverXkbBindingV1,
        event: <crate::protocol::river_xkb_binding_v1::RiverXkbBindingV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_xkb_binding_v1::Event;
        if let Event::Pressed = event {
            state.handle_key_binding_pressed(&proxy.id());
        }
    }
}
