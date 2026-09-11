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
    river_seat_v1::RiverSeatV1,
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
    pub proxy: RiverWindowV1,
    pub node: RiverNodeV1,
    pub new: bool,
    pub closed: bool,
    pub tags: u32,
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
        // 1. Remove closed windows
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

        // Filter visible windows on active tags
        let visible_count = self
            .windows
            .iter()
            .filter(|w| st.tag_state.is_view_visible(w.tags))
            .count();

        let layout_engine = layout::MasterStackLayout;
        let rects = layout_engine.arrange(usable_area, visible_count, &st.layout_config);

        let focused_color = hex_to_river_rgba(&st.border_color_focused);
        let unfocused_color = hex_to_river_rgba(&st.border_color_unfocused);

        let focused_win_proxy = self.seats.values().next().and_then(|s| s.focused.clone());

        let mut rect_idx = 0;
        for w in self.windows.iter_mut() {
            if st.tag_state.is_view_visible(w.tags) {
                if let Some(r) = rects.get(rect_idx) {
                    w.x = r.x;
                    w.y = r.y;
                    w.width = r.width;
                    w.height = r.height;

                    w.node.set_position(r.x, r.y);
                    w.proxy.propose_dimensions(r.width as i32, r.height as i32);
                    w.proxy.set_tiled(Edges::all());
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
                rect_idx += 1;
            }
        }

        // 4. Update focus on seats
        if let Some(seat) = self.seats.values_mut().next() {
            if let Some(top_win) = self.windows.last() {
                seat.proxy.focus_window(&top_win.proxy);
                top_win.node.place_top();
                seat.focused = Some(top_win.proxy.clone());
            } else {
                seat.proxy.clear_focus();
                seat.focused = None;
            }
        }

        proxy.manage_finish();
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
            Event::ManageStart => state.handle_manage_start(proxy),
            Event::RenderStart => state.handle_render_start(proxy),
            Event::SessionLocked => {}
            Event::SessionUnlocked => {}
            Event::Window { id } => {
                let node = id.get_node(qh, ());
                id.use_ssd();
                let current_tag = state.state.lock().unwrap().tag_state.focused;
                state.windows.push(WindowItem {
                    proxy: id,
                    node,
                    new: true,
                    closed: false,
                    tags: current_tag,
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
                state.seats.insert(
                    id.id(),
                    SeatItem {
                        proxy: id,
                        removed: false,
                        focused: None,
                    },
                );
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
        let window = match state.windows.iter_mut().find(|w| &w.proxy == proxy) {
            Some(w) => w,
            None => return,
        };
        match event {
            Event::Closed => window.closed = true,
            Event::Dimensions { width, height } => {
                window.width = width as u32;
                window.height = height as u32;
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
        _state: &mut Self,
        _proxy: &RiverSeatV1,
        _event: <RiverSeatV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
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
        _state: &mut Self,
        _proxy: &RiverPointerBindingV1,
        _event: <RiverPointerBindingV1 as Proxy>::Event,
        _data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
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
