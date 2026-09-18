//! Wayland event dispatch handlers for AppState.

use wayland_backend::client::ObjectId;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::layout::Rect;
use crate::protocol::{
    river_layer_shell_output_v1::RiverLayerShellOutputV1,
    river_layer_shell_seat_v1::RiverLayerShellSeatV1, river_layer_shell_v1::RiverLayerShellV1,
    river_node_v1::RiverNodeV1, river_output_v1::RiverOutputV1,
    river_pointer_binding_v1::RiverPointerBindingV1, river_seat_v1::RiverSeatV1,
    river_window_manager_v1::RiverWindowManagerV1, river_window_v1::RiverWindowV1,
    river_xkb_bindings_v1::RiverXkbBindingsV1,
};
use crate::wm::seat::SeatItem;
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
        state.wl_registry = Some(registry.clone());
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
                "wp_cursor_shape_manager_v1" => {
                    let mgr = registry.bind::<crate::protocol::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1, _, _>(
                        name,
                        version.min(2),
                        qh,
                        (),
                    );
                    state.cursor_shape_manager = Some(mgr);
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
                let vid = state.next_view_id;
                state.next_view_id += 1;
                let mut current_tags = state.tag_state.focused & state.spawn_tagmask;
                if current_tags == crate::tag::TAG_NONE {
                    current_tags = state.tag_state.focused;
                }
                let current_out = state.get_focused_output_id();

                state.attach_window(WindowItem {
                    id: vid,
                    proxy: id,
                    node,
                    initial_managed: false,
                    initial_rendered: false,
                    last_applied_ssd: None,
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
                    output: current_out,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    visual_geo: None,
                    anim_start_geo: None,
                    anim_target_geo: None,
                    last_proposed_w: None,
                    last_proposed_h: None,
                });
            }
            Event::Output { id } => {
                let ls_out = state
                    .river_layer
                    .as_ref()
                    .map(|ls| ls.get_output(&id, qh, id.id()));

                let out_id = id.id();
                state.outputs.insert(
                    out_id.clone(),
                    OutputItem {
                        proxy: id,
                        ls_output: ls_out,
                        removed: false,
                        usable_area: Rect::default(),
                        has_custom_usable_area: false,
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    },
                );
                if state.focused_output.is_none() {
                    state.focused_output = Some(out_id);
                }
            }
            Event::Seat { id } => {
                let ls_seat = state
                    .river_layer
                    .as_ref()
                    .map(|ls| ls.get_seat(&id, qh, id.id()));
                let mut seat = SeatItem::new(id.clone());
                seat.ls_seat = ls_seat;
                state.seats.insert(id.id(), seat);
                state.manage_dirty();
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
                let usable = state
                    .get_focused_output_id()
                    .and_then(|id| state.outputs.get(&id))
                    .map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.app_id = app_id;
                    AppState::apply_rules_to_window(&state.rules, w, usable, &state.outputs);
                }
                state.manage_dirty();
            }
            Event::Title { title } => {
                let usable = state
                    .get_focused_output_id()
                    .and_then(|id| state.outputs.get(&id))
                    .map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.title = title;
                    AppState::apply_rules_to_window(&state.rules, w, usable, &state.outputs);
                }
                state.manage_dirty();
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
        match event {
            Event::Position { x, y } => {
                if let Some(out) = state.outputs.get_mut(&proxy.id()) {
                    out.x = x;
                    out.y = y;
                    if !out.has_custom_usable_area {
                        out.usable_area.x = x;
                        out.usable_area.y = y;
                    }
                }
            }
            Event::Dimensions { width, height } => {
                let w = width as u32;
                let h = height as u32;
                if let Some(out) = state.outputs.get_mut(&proxy.id()) {
                    out.width = w;
                    out.height = h;
                    if !out.has_custom_usable_area {
                        out.usable_area.width = w;
                        out.usable_area.height = h;
                    }
                }
            }
            Event::Removed => {
                state.outputs.remove(&proxy.id());
                let fallback = state.outputs.keys().next().cloned();
                for w in &mut state.windows {
                    if w.output == Some(proxy.id()) {
                        w.output = fallback.clone();
                    }
                }
                if state.focused_output == Some(proxy.id()) {
                    state.focused_output = fallback;
                }
                state.manage_dirty();
            }
            _ => {}
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
            out.has_custom_usable_area = true;
        }
    }
}

impl Dispatch<RiverLayerShellSeatV1, ObjectId> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &RiverLayerShellSeatV1,
        event: <RiverLayerShellSeatV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_layer_shell_seat_v1::Event;
        if let Some(seat) = state.seats.get_mut(data) {
            match event {
                Event::FocusExclusive => {
                    seat.layer_focus = crate::wm::seat::LayerShellFocus::Exclusive;
                }
                Event::FocusNonExclusive => {
                    seat.layer_focus = crate::wm::seat::LayerShellFocus::NonExclusive;
                }
                Event::FocusNone => {
                    seat.layer_focus = crate::wm::seat::LayerShellFocus::None;
                }
            }
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
        qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_seat_v1::Event;
        match event {
            Event::PointerEnter { window } => {
                state.unhide_cursor();
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.hovered = Some(window.clone());
                    if state.focus_follows_cursor != crate::wm::FocusFollowsCursor::Disabled {
                        seat.focused = Some(window.clone());
                        if let Some(win) = state.windows.iter().find(|w| w.proxy == window)
                            && let Some(ref out_id) = win.output
                        {
                            state.focused_output = Some(out_id.clone());
                        }
                    }
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
                    seat.interacted = Some(window.clone());
                    seat.focused = Some(window.clone());
                    if let Some(win) = state.windows.iter().find(|w| w.proxy == window)
                        && let Some(ref out_id) = win.output
                    {
                        state.focused_output = Some(out_id.clone());
                    }
                }
                state.manage_dirty();
            }
            Event::OpDelta { dx, dy } => {
                state.unhide_cursor();
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.op_dx = dx;
                    seat.op_dy = dy;
                }
            }
            Event::OpRelease => {
                if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                    seat.op_release = true;
                }
            }
            Event::PointerPosition { x, y } => {
                state.unhide_cursor();
                state.pointer = (x, y);
            }
            Event::WlSeat { name } => {
                if let Some(ref reg) = state.wl_registry {
                    let wl_seat = reg.bind::<wayland_client::protocol::wl_seat::WlSeat, _, _>(
                        name,
                        1,
                        qh,
                        (),
                    );
                    let pointer = wl_seat.get_pointer(qh, ());
                    if let Some(ref shape_mgr) = state.cursor_shape_manager {
                        let device = shape_mgr.get_pointer(&pointer, qh, ());
                        if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                            seat.cursor_shape_device = Some(device);
                            seat.wl_pointer = Some(pointer);
                        }
                    }
                }
            }
            Event::ShellSurfaceInteraction { .. } => {}
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
                seat.pending_action = binding.action.clone();
            }
            state.manage_dirty();
        }
    }
}

wayland_client::delegate_noop!(AppState: ignore RiverLayerShellV1);
wayland_client::delegate_noop!(AppState: ignore RiverXkbBindingsV1);
wayland_client::delegate_noop!(AppState: ignore RiverNodeV1);
wayland_client::delegate_noop!(AppState: ignore wayland_client::protocol::wl_seat::WlSeat);
wayland_client::delegate_noop!(AppState: ignore wayland_client::protocol::wl_pointer::WlPointer);
wayland_client::delegate_noop!(AppState: ignore crate::protocol::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1);
wayland_client::delegate_noop!(AppState: ignore crate::protocol::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1);

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
