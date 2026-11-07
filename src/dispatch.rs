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
use crate::seat::SeatItem;
use crate::state::{AppState, OutputItem, WindowItem};

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
                        registry.bind::<RiverWindowManagerV1, _, _>(name, version.min(5), qh, ());
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
                    for (seat_id, seat) in state.seats.iter_mut() {
                        if seat.ls_seat.is_none() {
                            seat.ls_seat = Some(layer.get_seat(&seat.proxy, qh, seat_id.clone()));
                        }
                    }
                    for (out_id, out) in state.outputs.iter_mut() {
                        if out.ls_output.is_none() {
                            out.ls_output = Some(layer.get_output(&out.proxy, qh, out_id.clone()));
                        }
                    }
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
                tracing::error!(
                    error = "RiverWindowManagerV1 unavailable",
                    "Another window manager is already running on this river compositor"
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
            Event::SessionLocked => {
                state.handle_session_locked();
            }
            Event::SessionUnlocked => {
                state.handle_session_unlocked();
            }
            Event::Window { id } => {
                let node = id.get_node(qh, ());
                let vid = state.next_view_id;
                state.next_view_id += 1;
                let current_out = state.get_focused_output_id();
                let out_tags = current_out
                    .as_ref()
                    .and_then(|id| state.outputs.get(id))
                    .map(|o| o.tag_state.focused)
                    .unwrap_or(state.tag_state.focused);
                let mut current_tags = out_tags & state.spawn_tagmask;
                if current_tags == crate::tag::TAG_NONE {
                    current_tags = out_tags;
                }

                state.attach_window(WindowItem::new(
                    vid,
                    id.clone(),
                    node,
                    current_tags,
                    current_out,
                ));

                for seat in state.seats.values_mut() {
                    seat.set_focused_window(Some(id.clone()));
                }
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
                        wl_output: None,
                        name: None,
                        ls_output: ls_out,
                        removed: false,
                        usable_area: Rect::default(),
                        has_custom_usable_area: false,
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                        tag_state: crate::tag::TagState::new(),
                        previous_focused_tags: 1,
                        monocle: false,
                        floating_hidden_tags: 0,
                        capture_sessions: 0,
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
                    w.mark_closed();
                }
                for seat in state.seats.values_mut() {
                    if seat.focused.as_ref() == Some(proxy) {
                        seat.set_focused_window(None);
                    }
                }
                state.reconcile_focus();
                state.manage_dirty();
            }
            Event::CaptureSessions { count } => {
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.set_capture_sessions(count);
                    state.broadcast_status();
                }
            }
            Event::AppId { app_id } => {
                let usable = state
                    .get_focused_output_id()
                    .and_then(|id| state.outputs.get(&id))
                    .map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.set_app_id(app_id);
                    AppState::apply_rules_to_window(&state.rules, w, usable, &state.outputs);
                }
                state.uncollapse_floating_for_window_by_proxy(proxy);
                state.manage_dirty();
            }
            Event::Title { title } => {
                let usable = state
                    .get_focused_output_id()
                    .and_then(|id| state.outputs.get(&id))
                    .map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.set_title(title);
                    AppState::apply_rules_to_window(&state.rules, w, usable, &state.outputs);
                }
                state.uncollapse_floating_for_window_by_proxy(proxy);
                state.manage_dirty();
            }
            Event::Parent { parent } => {
                let usable = state
                    .get_focused_output_id()
                    .and_then(|id| state.outputs.get(&id))
                    .map(|o| o.usable_area);
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.set_has_parent(parent.is_some());
                    AppState::apply_rules_to_window(&state.rules, w, usable, &state.outputs);
                }
                state.uncollapse_floating_for_window_by_proxy(proxy);
                state.manage_dirty();
            }
            Event::Dimensions { width, height } => {
                let w = width as u32;
                let h = height as u32;
                let is_resizing = state.seats.values().any(
                    |s| matches!(&s.op, crate::seat::SeatOp::Resize { proxy: p, .. } if p == proxy),
                );
                let focused_out_id = state.get_focused_output_id();
                if let Some(win) = state.windows.iter_mut().find(|win| &win.proxy == proxy) {
                    let has_dimensions_rule = state.rules.iter().any(|r| {
                        r.dimensions.is_some()
                            && r.app_id.as_deref().is_none_or(|pat| {
                                crate::rule::glob_match(pat, win.app_id.as_deref().unwrap_or(""))
                            })
                            && r.title.as_deref().is_none_or(|pat| {
                                crate::rule::glob_match(pat, win.title.as_deref().unwrap_or(""))
                            })
                    });

                    let has_position_rule = state.rules.iter().any(|r| {
                        r.position.is_some()
                            && r.app_id.as_deref().is_none_or(|pat| {
                                crate::rule::glob_match(pat, win.app_id.as_deref().unwrap_or(""))
                            })
                            && r.title.as_deref().is_none_or(|pat| {
                                crate::rule::glob_match(pat, win.title.as_deref().unwrap_or(""))
                            })
                    });

                    let usable = win
                        .output
                        .as_ref()
                        .or(focused_out_id.as_ref())
                        .and_then(|id| state.outputs.get(id))
                        .map(|o| {
                            if o.usable_area.width > 0 && o.usable_area.height > 0 {
                                o.usable_area
                            } else {
                                Rect::new(o.x, o.y, o.width, o.height)
                            }
                        });

                    win.apply_client_dimensions(
                        w,
                        h,
                        is_resizing,
                        has_dimensions_rule,
                        has_position_rule,
                        usable,
                    );
                }
                state.manage_dirty();
            }
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
            Event::WlOutput { name } => {
                if let Some(ref reg) = state.wl_registry {
                    let wl_out = reg.bind::<wayland_client::protocol::wl_output::WlOutput, _, _>(
                        name,
                        4,
                        _qh,
                        proxy.id(),
                    );
                    if let Some(out) = state.outputs.get_mut(&proxy.id()) {
                        out.wl_output = Some(wl_out);
                    }
                }
            }
            Event::CaptureSessions { count } => {
                if let Some(out) = state.outputs.get_mut(&proxy.id()) {
                    out.capture_sessions = count;
                    state.broadcast_status();
                }
            }
            Event::Removed => {
                let out_id = proxy.id();
                let removed_usable_area = state.outputs.get(&out_id).map(|o| o.usable_area);
                if let Some(mut out) = state.outputs.remove(&out_id) {
                    if let Some(ls_out) = out.ls_output.take() {
                        ls_out.destroy();
                    }
                    if let Some(wl_out) = out.wl_output.take()
                        && wl_out.version() >= 2
                    {
                        wl_out.release();
                    }
                }
                proxy.destroy();
                let fallback = state.outputs.keys().next().cloned();
                let fallback_usable_area = fallback
                    .as_ref()
                    .and_then(|id| state.outputs.get(id))
                    .map(|o| o.usable_area);

                for w in &mut state.windows {
                    if w.output == Some(out_id.clone()) {
                        AppState::migrate_window_to_output(
                            w,
                            fallback.clone(),
                            removed_usable_area,
                            fallback_usable_area,
                        );
                        if w.fullscreen {
                            w.exit_fullscreen_on_output_removed();
                        }
                    }
                }
                if state.focused_output == Some(out_id) {
                    state.focused_output = fallback;
                    if let Some(out) = state
                        .focused_output
                        .as_ref()
                        .and_then(|id| state.outputs.get(id))
                    {
                        state.tag_state = out.tag_state;
                        state.previous_focused_tags = out.previous_focused_tags;
                    }
                }
                state.manage_dirty();
            }
        }
    }
}

impl Dispatch<wayland_client::protocol::wl_output::WlOutput, ObjectId> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &wayland_client::protocol::wl_output::WlOutput,
        event: <wayland_client::protocol::wl_output::WlOutput as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_output::Event;
        if let Event::Name { name } = event
            && let Some(out) = state.outputs.get_mut(data)
        {
            out.name = Some(name);
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
                    seat.layer_focus = crate::seat::LayerShellFocus::Exclusive;
                }
                Event::FocusNonExclusive => {
                    seat.layer_focus = crate::seat::LayerShellFocus::NonExclusive;
                }
                Event::FocusNone => {
                    seat.layer_focus = crate::seat::LayerShellFocus::None;
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
                    if state.focus_follows_cursor != crate::FocusFollowsCursor::Disabled {
                        seat.set_focused_window(Some(window.clone()));
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
                    seat.set_focused_window(Some(window.clone()));
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
                if state.focus_follows_cursor == crate::FocusFollowsCursor::Always
                    && let Some(seat) = state.seats.get_mut(&proxy.id())
                    && let Some(window) = seat.hovered.clone()
                    && seat.focused.as_ref() != Some(&window)
                {
                    seat.set_focused_window(Some(window.clone()));
                    if let Some(win) = state.windows.iter().find(|w| w.proxy == window)
                        && let Some(ref out_id) = win.output
                    {
                        state.focused_output = Some(out_id.clone());
                    }
                    state.manage_dirty();
                }
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
                let seat_id = proxy.id();
                if let Some(mut seat) = state.seats.remove(&seat_id) {
                    if let Some(ls_seat) = seat.ls_seat.take() {
                        ls_seat.destroy();
                    }
                    if let Some(shape_dev) = seat.cursor_shape_device.take() {
                        shape_dev.destroy();
                    }
                    if let Some(pointer) = seat.wl_pointer.take()
                        && pointer.version() >= 3
                    {
                        pointer.release();
                    }
                    for pb in seat.pointer_bindings.values() {
                        pb.proxy.destroy();
                    }
                    let to_remove: Vec<wayland_backend::client::ObjectId> = state
                        .key_bindings
                        .iter()
                        .filter(|(_, b)| b.seat_id == seat_id)
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in to_remove {
                        if let Some(b) = state.key_bindings.remove(&id) {
                            b.proxy.destroy();
                        }
                    }
                }
                proxy.destroy();
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
