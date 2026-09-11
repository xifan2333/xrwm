pub mod animation;
pub mod ipc;
pub mod layout;
pub mod protocol;
pub mod state;
pub mod tag;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use layout::Layout;
use protocol::{
    river_layer_shell_output_v1::RiverLayerShellOutputV1,
    river_layer_shell_v1::RiverLayerShellV1,
    river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1,
    river_pointer_binding_v1::RiverPointerBindingV1,
    river_seat_v1::{Modifiers, RiverSeatV1},
    river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::{Edges, RiverWindowV1},
    river_xkb_binding_v1::RiverXkbBindingV1,
    river_xkb_bindings_v1::RiverXkbBindingsV1,
};
use state::{AppState, spawn_init_script};
use wayland_backend::client::ObjectId;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, protocol::wl_registry};

pub fn hex_to_river_rgba(hex_str: &str) -> (u32, u32, u32, u32) {
    let h = hex_str
        .trim_start_matches("0x")
        .trim_start_matches('#')
        .trim();
    if h.len() < 6 {
        return (u32::MAX, u32::MAX, u32::MAX, u32::MAX);
    }
    let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(255) * (u32::MAX / 255);
    let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(255) * (u32::MAX / 255);
    let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(255) * (u32::MAX / 255);
    let a = u32::MAX;
    (r, g, b, a)
}

#[derive(Debug)]
pub struct WindowItem {
    pub id: u32,
    pub proxy: RiverWindowV1,
    pub node: RiverNodeV1,
    pub new: bool,
    pub closed: bool,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub tags: u32,
    pub floating: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct OutputItem {
    pub proxy: RiverOutputV1,
    pub ls_output: Option<RiverLayerShellOutputV1>,
    pub removed: bool,
    pub usable_area: layout::Rect,
}

#[derive(Debug)]
pub struct SeatItem {
    pub proxy: RiverSeatV1,
    pub removed: bool,
    pub focused: Option<RiverWindowV1>,
    pub hovered: Option<RiverWindowV1>,
    pub interacted: Option<RiverWindowV1>,
    pub pending_action: PointerAction,
    pub op: SeatOp,
    pub op_dx: i32,
    pub op_dy: i32,
    pub op_release: bool,
    pub pointer_bindings: HashMap<ObjectId, PointerBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerAction {
    None,
    Move,
    Resize,
    ToggleFloating,
}

#[derive(Debug)]
pub struct PointerBinding {
    pub proxy: RiverPointerBindingV1,
    pub action: PointerAction,
}

#[derive(Debug)]
pub enum SeatOp {
    None,
    Move {
        proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
    },
    Resize {
        proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
        start_width: u32,
        start_height: u32,
        edges: Edges,
    },
}

pub struct AppData {
    pub river_wm: Option<RiverWindowManagerV1>,
    pub river_xkb: Option<RiverXkbBindingsV1>,
    pub river_layer: Option<RiverLayerShellV1>,
    pub state: Arc<Mutex<AppState>>,
    pub windows: Vec<WindowItem>,
    pub outputs: HashMap<ObjectId, OutputItem>,
    pub seats: HashMap<ObjectId, SeatItem>,
}

impl AppData {
    pub fn new(state: Arc<Mutex<AppState>>) -> Self {
        Self {
            river_wm: None,
            river_xkb: None,
            river_layer: None,
            state,
            windows: Vec::new(),
            outputs: HashMap::new(),
            seats: HashMap::new(),
        }
    }

    pub fn handle_manage_start(&mut self, proxy: &RiverWindowManagerV1) {
        // 1. Remove closed windows, ending any interactive operation that was
        //    still holding them (otherwise the pointer grab would leak).
        let closed: Vec<RiverWindowV1> = self
            .windows
            .iter()
            .filter(|w| w.closed)
            .map(|w| w.proxy.clone())
            .collect();
        if !closed.is_empty() {
            for seat in self.seats.values_mut() {
                let op_target = match &seat.op {
                    SeatOp::None => None,
                    SeatOp::Move { proxy, .. } | SeatOp::Resize { proxy, .. } => {
                        Some(proxy.clone())
                    }
                };
                if let Some(target) = op_target {
                    if closed.iter().any(|c| c == &target) {
                        if let SeatOp::Resize { proxy, .. } = &seat.op {
                            proxy.inform_resize_end();
                        }
                        seat.proxy.op_end();
                        seat.op = SeatOp::None;
                        seat.op_release = false;
                        seat.op_dx = 0;
                        seat.op_dy = 0;
                    }
                }
                if seat
                    .focused
                    .as_ref()
                    .is_some_and(|f| closed.iter().any(|c| c == f))
                {
                    seat.focused = None;
                }
                if seat
                    .hovered
                    .as_ref()
                    .is_some_and(|h| closed.iter().any(|c| c == h))
                {
                    seat.hovered = None;
                }
            }
        }
        self.windows.retain(|w| !w.closed);

        // 2. Remove disconnected outputs
        self.outputs.retain(|_, out| {
            if out.removed {
                if let Some(ref ls_out) = out.ls_output {
                    ls_out.destroy();
                }
                out.proxy.destroy();
                return false;
            }
            true
        });

        // 3. Arrange windows for each output
        let st = self.state.lock().unwrap();
        let default_area = layout::Rect::new(0, 30, 1280, 770);

        // Get usable area from first output or default (after subtracting Waybar/panels)
        let usable_area = self
            .outputs
            .values()
            .next()
            .map(|out| out.usable_area)
            .unwrap_or(default_area);

        // Filter visible tiled windows on active tags (floating ones keep their own geometry)
        let visible_count = self
            .windows
            .iter()
            .filter(|w| !w.floating && st.tag_state.is_view_visible(w.tags))
            .count();

        let layout_engine = layout::MasterStackLayout;
        let rects = layout_engine.arrange(usable_area, visible_count, &st.layout_config);

        let focused_color = hex_to_river_rgba(&st.border_color_focused);
        let unfocused_color = hex_to_river_rgba(&st.border_color_unfocused);

        let focused_win_proxy = self.seats.values().next().and_then(|s| s.focused.clone());

        let mut rect_idx = 0;
        for w in self.windows.iter_mut() {
            if !st.tag_state.is_view_visible(w.tags) {
                continue;
            }

            if w.floating {
                // Floating windows stay where the user put them.
                w.node.set_position(w.x, w.y);
                w.proxy.propose_dimensions(w.width as i32, w.height as i32);
                w.proxy.set_tiled(Edges::empty());
            } else if let Some(r) = rects.get(rect_idx) {
                w.x = r.x;
                w.y = r.y;
                w.width = r.width;
                w.height = r.height;

                w.node.set_position(r.x, r.y);
                w.proxy.propose_dimensions(r.width as i32, r.height as i32);
                w.proxy.set_tiled(Edges::all());
                rect_idx += 1;
            }

            w.proxy.use_ssd();

            let is_focused = focused_win_proxy.as_ref() == Some(&w.proxy);
            let (cr, cg, cb, ca) = if is_focused {
                focused_color
            } else {
                unfocused_color
            };

            w.proxy
                .set_borders(Edges::all(), st.border_width as i32, cr, cg, cb, ca);
        }

        // 4. Reorder window stack from user interaction (MRU) and process pointer ops
        // 4a. Start interactive operations requested by pointer bindings
        let mut start_move: Vec<(ObjectId, RiverWindowV1)> = Vec::new();
        let mut start_resize: Vec<(ObjectId, RiverWindowV1)> = Vec::new();
        let mut toggle_floating: Vec<RiverWindowV1> = Vec::new();
        for (id, seat) in self.seats.iter_mut() {
            // Clicking a window focuses it.  Focus changes must NOT reorder the
            // tiling stack, otherwise the master/stack layout would shuffle
            // under the user's pointer on every click.
            if let Some(win_proxy) = seat.interacted.take() {
                seat.focused = Some(win_proxy);
            }

            if seat.pending_action != PointerAction::None {
                tracing::debug!(
                    "op: pending_action={:?} hovered={:?}",
                    seat.pending_action,
                    seat.hovered.as_ref().map(|p| p.id())
                );
                if let Some(win_proxy) = seat.hovered.clone() {
                    match seat.pending_action {
                        PointerAction::Move => start_move.push((id.clone(), win_proxy)),
                        PointerAction::Resize => start_resize.push((id.clone(), win_proxy)),
                        PointerAction::ToggleFloating => toggle_floating.push(win_proxy),
                        PointerAction::None => {}
                    }
                    seat.pending_action = PointerAction::None;
                }
            }
        }

        // Toggling is a one-shot gesture: flip the class and let the layout
        // engine take over (or hand the window back to the user) next cycle.
        for win_proxy in toggle_floating {
            if let Some(w) = self.windows.iter_mut().find(|w| w.proxy == win_proxy) {
                w.floating = !w.floating;
                w.node.place_top();
                tracing::debug!(
                    "op: toggle floating on {:?} -> floating={}",
                    w.proxy.id(),
                    w.floating
                );
            }
        }

        for (id, win_proxy) in start_move {
            // Only floating windows are draggable.  A tiled window's position is
            // owned by the layout engine, so dragging it would be a lie.
            let geo = self
                .windows
                .iter()
                .find(|w| w.proxy == win_proxy && w.floating)
                .map(|w| (w.x, w.y));
            if let (Some((x, y)), Some(seat)) = (geo, self.seats.get_mut(&id)) {
                tracing::debug!("op: start move on {:?} at ({x},{y})", win_proxy.id());
                seat.proxy.op_start_pointer();
                seat.op = SeatOp::Move {
                    proxy: win_proxy.clone(),
                    start_x: x,
                    start_y: y,
                };
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        for (id, win_proxy) in start_resize {
            let geo = self
                .windows
                .iter()
                .find(|w| w.proxy == win_proxy && w.floating)
                .map(|w| (w.x, w.y, w.width, w.height));
            if let (Some((x, y, w, h)), Some(seat)) = (geo, self.seats.get_mut(&id)) {
                tracing::debug!("op: start resize on {:?}", win_proxy.id());
                seat.proxy.op_start_pointer();
                win_proxy.inform_resize_start();
                seat.op = SeatOp::Resize {
                    proxy: win_proxy.clone(),
                    start_x: x,
                    start_y: y,
                    start_width: w,
                    start_height: h,
                    edges: Edges::Bottom.union(Edges::Right),
                };
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        // Apply interactive pointer operations (move / resize).
        // `op_dx`/`op_dy` are absolute offsets from the start of the operation,
        // not per-frame deltas, so positions are always `start + delta`.
        let mut released: Vec<ObjectId> = Vec::new();
        #[allow(clippy::type_complexity)]
        let mut resize_updates: Vec<(RiverWindowV1, i32, i32, i32, i32, i32, i32, Edges)> =
            Vec::new();
        let mut move_updates: Vec<(RiverWindowV1, i32, i32, i32, i32)> = Vec::new();

        for (id, seat) in self.seats.iter() {
            match &seat.op {
                SeatOp::None => {}
                SeatOp::Move {
                    proxy,
                    start_x,
                    start_y,
                } => {
                    move_updates.push((proxy.clone(), *start_x, *start_y, seat.op_dx, seat.op_dy));
                }
                SeatOp::Resize {
                    proxy,
                    start_x,
                    start_y,
                    start_width,
                    start_height,
                    edges,
                } => {
                    let mut width = *start_width as i32;
                    let mut height = *start_height as i32;
                    if edges.contains(Edges::Left) {
                        width -= seat.op_dx;
                    }
                    if edges.contains(Edges::Right) {
                        width += seat.op_dx;
                    }
                    if edges.contains(Edges::Top) {
                        height -= seat.op_dy;
                    }
                    if edges.contains(Edges::Bottom) {
                        height += seat.op_dy;
                    }
                    resize_updates.push((
                        proxy.clone(),
                        *start_x,
                        *start_y,
                        *start_width as i32,
                        *start_height as i32,
                        width.max(1),
                        height.max(1),
                        *edges,
                    ));
                }
            }
            if seat.op_release {
                released.push(id.clone());
            }
        }

        // Move: absolute position from the op start plus the accumulated delta.
        for (proxy, start_x, start_y, dx, dy) in move_updates {
            if let Some(w) = self.windows.iter_mut().find(|w| w.proxy == proxy) {
                w.x = (start_x + dx).max(0);
                w.y = (start_y + dy).max(0);
                w.node.set_position(w.x, w.y);
            }
        }

        // Resize: edge-anchored dimensions, shifting the origin for left/top edges.
        #[allow(clippy::type_complexity)]
        for (proxy, start_x, start_y, orig_w, orig_h, new_w, new_h, edges) in resize_updates {
            if let Some(w) = self.windows.iter_mut().find(|w| w.proxy == proxy) {
                let mut x = start_x;
                let mut y = start_y;
                if edges.contains(Edges::Left) {
                    x += orig_w - new_w;
                }
                if edges.contains(Edges::Top) {
                    y += orig_h - new_h;
                }
                w.x = x.max(0);
                w.y = y.max(0);
                w.width = new_w as u32;
                w.height = new_h as u32;
                w.node.set_position(w.x, w.y);
                w.proxy.propose_dimensions(new_w, new_h);
            }
        }

        for id in released {
            if let Some(seat) = self.seats.get_mut(&id) {
                if let SeatOp::Resize { proxy, .. } = &seat.op {
                    proxy.inform_resize_end();
                }
                // Always release the compositor-side pointer grab, otherwise the
                // cursor stays stuck in the operation after the button is let go.
                tracing::debug!("op: end (grab released)");
                seat.proxy.op_end();
                seat.op = SeatOp::None;
                seat.op_release = false;
                seat.op_dx = 0;
                seat.op_dy = 0;
            }
        }

        // 5. Apply focus: the most recently focused window sits at the top of the stack
        let mut focus_updates: Vec<(ObjectId, Option<RiverWindowV1>)> = Vec::new();
        for (id, seat) in self.seats.iter() {
            let target = seat
                .focused
                .clone()
                .filter(|p| self.windows.iter().any(|w| &w.proxy == p))
                .or_else(|| self.windows.last().map(|w| w.proxy.clone()));
            tracing::debug!(
                "manage: seat focused={:?} hovered={:?} target={:?}",
                seat.focused.as_ref().map(|p| p.id()),
                seat.hovered.as_ref().map(|p| p.id()),
                target.as_ref().map(|p| p.id()),
            );
            focus_updates.push((id.clone(), target));
        }

        for (id, target) in focus_updates {
            if let Some(seat) = self.seats.get_mut(&id) {
                match target {
                    Some(win_proxy) => {
                        seat.proxy.focus_window(&win_proxy);
                        if let Some(w) = self.windows.iter().find(|w| w.proxy == win_proxy) {
                            w.node.place_top();
                        }
                        seat.focused = Some(win_proxy);
                    }
                    None => {
                        seat.proxy.clear_focus();
                        seat.focused = None;
                    }
                }
            }
        }

        proxy.manage_finish();
    }

    /// Mirror the live WM window list into the shared `AppState` for `xrwm status`.
    fn sync_shared_state(&mut self) {
        let windows: Vec<state::WindowSnapshot> = self
            .windows
            .iter()
            .map(|w| state::WindowSnapshot {
                id: w.id,
                app_id: w.app_id.clone(),
                title: w.title.clone(),
                tags: w.tags,
                floating: w.floating,
            })
            .collect();
        let focused = self
            .seats
            .values()
            .find_map(|s| s.focused.as_ref())
            .and_then(|p| self.windows.iter().find(|w| &w.proxy == p))
            .map(|w| w.id);
        let hovered = self
            .seats
            .values()
            .find_map(|s| s.hovered.as_ref())
            .and_then(|p| self.windows.iter().find(|w| &w.proxy == p))
            .map(|w| w.id);

        let mut st = self.state.lock().unwrap();
        st.sync_windows(&windows, focused, hovered);
    }

    pub fn handle_render_start(&mut self, proxy: &RiverWindowManagerV1) {
        for w in &mut self.windows {
            w.node.set_position(w.x, w.y);
        }
        proxy.render_finish();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppData {
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

impl Dispatch<RiverWindowManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverWindowManagerV1,
        event: <RiverWindowManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use protocol::river_window_manager_v1::Event;
        match event {
            Event::Unavailable => {
                eprintln!(
                    "Error: Another window manager is already running on this river compositor."
                );
                std::process::exit(1);
            }
            Event::Finished => std::process::exit(0),
            Event::ManageStart => {
                state.handle_manage_start(proxy);
                state.sync_shared_state();
            }
            Event::RenderStart => state.handle_render_start(proxy),
            Event::SessionLocked => {}
            Event::SessionUnlocked => {}
            Event::Window { id } => {
                let node = id.get_node(qh, ());
                id.use_ssd();
                let next_id = {
                    let mut st = state.state.lock().unwrap();
                    let current_tag = st.tag_state.focused;
                    let vid = st.next_view_id;
                    st.next_view_id += 1;
                    (vid, current_tag)
                };
                state.windows.push(WindowItem {
                    id: next_id.0,
                    proxy: id,
                    node,
                    new: true,
                    closed: false,
                    app_id: None,
                    title: None,
                    tags: next_id.1,
                    floating: false,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
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
                        usable_area: layout::Rect::new(0, 30, 1280, 770),
                    },
                );
            }
            Event::Seat { id } => {
                let mut seat = SeatItem {
                    proxy: id.clone(),
                    removed: false,
                    focused: None,
                    hovered: None,
                    interacted: None,
                    pending_action: PointerAction::None,
                    op: SeatOp::None,
                    op_dx: 0,
                    op_dy: 0,
                    op_release: false,
                    pointer_bindings: HashMap::new(),
                };

                // Preset pointer bindings (overridable later via `xrwm map-pointer`):
                //   Super+LMB    drag a floating window
                //   Super+RMB    resize a floating window
                //   Super+MMB    toggle floating/tiling for the hovered window
                const BTN_LEFT: u32 = 0x110;
                const BTN_RIGHT: u32 = 0x111;
                const BTN_MIDDLE: u32 = 0x112;
                for (button, action) in [
                    (BTN_LEFT, PointerAction::Move),
                    (BTN_RIGHT, PointerAction::Resize),
                    (BTN_MIDDLE, PointerAction::ToggleFloating),
                ] {
                    let binding = id.get_pointer_binding(button, Modifiers::Mod4, qh, id.id());
                    binding.enable();
                    tracing::debug!(
                        "seat: registered pointer binding button=0x{button:x} mods={:?} -> {action:?}",
                        Modifiers::Mod4
                    );
                    seat.pointer_bindings.insert(
                        binding.id(),
                        PointerBinding {
                            proxy: binding,
                            action,
                        },
                    );
                }

                state.seats.insert(id.id(), seat);
            }
        }
    }

    wayland_client::event_created_child!(AppData, RiverWindowManagerV1, [
        protocol::river_window_manager_v1::EVT_WINDOW_OPCODE => (RiverWindowV1, ()),
        protocol::river_window_manager_v1::EVT_OUTPUT_OPCODE => (RiverOutputV1, ()),
        protocol::river_window_manager_v1::EVT_SEAT_OPCODE => (RiverSeatV1, ())
    ]);
}

impl Dispatch<RiverWindowV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverWindowV1,
        event: <RiverWindowV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use protocol::river_window_v1::Event;
        match event {
            Event::Closed => {
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.closed = true;
                }
            }
            Event::Dimensions { width, height } => {
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.width = width as u32;
                    w.height = height as u32;
                }
            }
            Event::AppId { app_id } => {
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.app_id = app_id;
                }
            }
            Event::Title { title } => {
                if let Some(w) = state.windows.iter_mut().find(|w| &w.proxy == proxy) {
                    w.title = title;
                }
            }
            Event::PointerMoveRequested { seat } => {
                let geo = state
                    .windows
                    .iter()
                    .find(|w| &w.proxy == proxy)
                    .map(|w| (w.x, w.y));
                if let (Some((x, y)), Some(s)) = (geo, state.seats.get_mut(&seat.id())) {
                    s.interacted = Some(proxy.clone());
                    s.proxy.op_start_pointer();
                    s.op = SeatOp::Move {
                        proxy: proxy.clone(),
                        start_x: x,
                        start_y: y,
                    };
                    s.op_dx = 0;
                    s.op_dy = 0;
                }
            }
            Event::PointerResizeRequested { seat, edges } => {
                let geo = state
                    .windows
                    .iter()
                    .find(|w| &w.proxy == proxy)
                    .map(|w| (w.x, w.y, w.width, w.height));
                if let (Some((x, y, w, h)), Some(s)) = (geo, state.seats.get_mut(&seat.id())) {
                    s.interacted = Some(proxy.clone());
                    s.proxy.op_start_pointer();
                    proxy.inform_resize_start();
                    s.op = SeatOp::Resize {
                        proxy: proxy.clone(),
                        start_x: x,
                        start_y: y,
                        start_width: w,
                        start_height: h,
                        edges: edges
                            .into_result()
                            .unwrap_or(Edges::Bottom.union(Edges::Right)),
                    };
                    s.op_dx = 0;
                    s.op_dy = 0;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<RiverOutputV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverOutputV1,
        event: <RiverOutputV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use protocol::river_output_v1::Event;
        if let Some(out) = state.outputs.get_mut(&proxy.id()) {
            match event {
                Event::Removed => out.removed = true,
                Event::Dimensions { width, height } => {
                    out.usable_area.width = width as u32;
                    out.usable_area.height =
                        (height as u32).saturating_sub(out.usable_area.y as u32);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<RiverLayerShellOutputV1, ObjectId> for AppData {
    fn event(
        state: &mut Self,
        _proxy: &RiverLayerShellOutputV1,
        event: <RiverLayerShellOutputV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use protocol::river_layer_shell_output_v1::Event;
        if let Some(out) = state.outputs.get_mut(data) {
            let Event::NonExclusiveArea {
                x,
                y,
                width,
                height,
            } = event;
            out.usable_area = layout::Rect::new(x, y, width.max(1) as u32, height.max(1) as u32);
        }
    }
}

impl Dispatch<RiverSeatV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverSeatV1,
        event: <RiverSeatV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use protocol::river_seat_v1::Event;

        if let Event::Removed = event {
            if let Some(seat) = state.seats.get_mut(&proxy.id()) {
                seat.removed = true;
                for binding in seat.pointer_bindings.values() {
                    binding.proxy.destroy();
                }
                seat.pointer_bindings.clear();
            }
            return;
        }

        let Some(seat) = state.seats.get_mut(&proxy.id()) else {
            return;
        };

        let mut needs_manage = false;

        match event {
            Event::PointerEnter { window } => {
                tracing::debug!("seat: pointer_enter {:?}", window.id());
                seat.hovered = Some(window.clone());
                // Focus-follows-mouse: hovering a window gives it keyboard focus
                // (and its focused border) without reordering the tiling stack.
                seat.focused = Some(window);
                needs_manage = true;
            }
            Event::PointerLeave => {
                tracing::debug!("seat: pointer_leave");
                seat.hovered = None;
                needs_manage = true;
            }
            Event::WindowInteraction { window } => {
                tracing::debug!("seat: window_interaction {:?}", window.id());
                seat.interacted = Some(window);
                needs_manage = true;
            }
            Event::OpDelta { dx, dy } => {
                tracing::debug!("seat: op_delta {dx},{dy}");
                seat.op_dx = dx;
                seat.op_dy = dy;
            }
            Event::OpRelease => {
                tracing::debug!("seat: op_release");
                seat.op_release = true;
            }
            Event::ShellSurfaceInteraction { .. }
            | Event::PointerPosition { .. }
            | Event::WlSeat { .. } => {}
            Event::Removed => unreachable!(),
        }

        // Only focus changes need us to ask for a manage sequence.  River drives
        // manage/render cycles for pointer operations itself, and the protocol
        // forbids treating a pointer position change as a reason to start one:
        // doing so creates an endless manage_start -> pointer_position -> dirty
        // feedback loop that pegs the CPU.
        if needs_manage {
            if let Some(wm) = state.river_wm.as_ref() {
                wm.manage_dirty();
            }
        }
    }
}

impl Dispatch<RiverXkbBindingV1, ObjectId> for AppData {
    fn event(
        _state: &mut Self,
        _proxy: &RiverXkbBindingV1,
        _event: <RiverXkbBindingV1 as Proxy>::Event,
        _data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<RiverPointerBindingV1, ObjectId> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverPointerBindingV1,
        event: <RiverPointerBindingV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use protocol::river_pointer_binding_v1::Event;
        tracing::debug!("pointer binding event: {event:?} on {proxy:?}");
        if let Event::Pressed = event {
            if let Some(seat) = state.seats.get_mut(data) {
                if let Some(binding) = seat.pointer_bindings.get(&proxy.id()) {
                    seat.pending_action = binding.action;
                }
            }
            if let Some(wm) = state.river_wm.as_ref() {
                wm.manage_dirty();
            }
        }
    }
}

wayland_client::delegate_noop!(AppData: ignore RiverLayerShellV1);
wayland_client::delegate_noop!(AppData: ignore RiverXkbBindingsV1);
wayland_client::delegate_noop!(AppData: ignore RiverNodeV1);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if !args.is_empty() {
        // Run in Client mode
        match ipc::parse_cli_args(&args) {
            Ok(cmd) => match ipc::send_ipc_command(&cmd) {
                Ok(resp) => {
                    if resp.success {
                        if !resp.message.is_empty() {
                            println!("{}", resp.message);
                        }
                    } else {
                        eprintln!("Error: {}", resp.message);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // Default: Run as Window Manager daemon
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("xrwm=info")),
        )
        .with_writer(std::io::stderr)
        .init();
    println!("xrwm - River 0.4+ Wayland Window Manager starting...");

    let state = Arc::new(Mutex::new(AppState::new()));

    // 1. Start IPC server thread
    let server_state = Arc::clone(&state);
    let listener = match ipc::create_ipc_server() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind IPC socket: {e}");
            std::process::exit(1);
        }
    };

    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };

            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() {
                if let Ok(cmd) = serde_json::from_str::<ipc::IpcCommand>(&line) {
                    let mut st = server_state.lock().unwrap();
                    let response = match st.handle_ipc_command(&cmd) {
                        Ok(msg) => ipc::IpcResponse::ok(msg),
                        Err(err) => ipc::IpcResponse::err(err),
                    };

                    if let Ok(resp_json) = serde_json::to_string(&response) {
                        let _ = stream.write_all(resp_json.as_bytes());
                        let _ = stream.write_all(b"\n");
                        let _ = stream.flush();
                    }
                }
            }
        }
    });

    // 2. Connect to Wayland server (River)
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let _registry = display.get_registry(&event_queue.handle(), ());

    let mut app_data = AppData::new(state);

    // Roundtrip to bind globals
    event_queue.roundtrip(&mut app_data)?;

    if app_data.river_wm.is_none() {
        eprintln!("river_window_manager_v1 global not found! Is river running?");
        std::process::exit(1);
    }

    // 3. Spawn ~/.config/xrwm/init once daemon and Wayland protocol are ready
    spawn_init_script();

    // 4. Main Wayland event dispatch loop
    loop {
        event_queue.blocking_dispatch(&mut app_data)?;
    }
}
