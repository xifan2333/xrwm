#![allow(clippy::unwrap_used)]

//! Tests for FramePlan computation and render execution.

use std::os::fd::{OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use wayland_backend::protocol::{Argument, Message};
use wayland_backend::server::{
    Backend, ClientId, GlobalHandler, GlobalId, Handle, ObjectData, ObjectId,
};
use wayland_client::{Connection, EventQueue, Proxy};

use xrwm::protocol::{
    river_output_v1 as output, river_seat_v1 as seat, river_window_manager_v1 as wm,
    river_window_v1 as window,
};
use xrwm::render::compute_frame_plan;
use xrwm::state::AppState;

#[derive(Default)]
struct ServerState {
    manager: Option<ObjectId>,
    requests: Vec<Message<ObjectId, OwnedFd>>,
}

struct Recorder;

impl GlobalHandler<ServerState> for Recorder {
    fn bind(
        self: Arc<Self>,
        _handle: &Handle,
        state: &mut ServerState,
        _client: ClientId,
        _global: GlobalId,
        object: ObjectId,
    ) -> Arc<dyn ObjectData<ServerState>> {
        if state.manager.is_none() {
            state.manager = Some(object);
        }
        self
    }
}

impl ObjectData<ServerState> for Recorder {
    fn request(
        self: Arc<Self>,
        _handle: &Handle,
        state: &mut ServerState,
        _client: ClientId,
        message: Message<ObjectId, OwnedFd>,
    ) -> Option<Arc<dyn ObjectData<ServerState>>> {
        let creates_object = message
            .args
            .iter()
            .any(|arg| matches!(arg, Argument::NewId(_)));
        state.requests.push(message);
        if creates_object { Some(self) } else { None }
    }

    fn destroyed(
        self: Arc<Self>,
        _handle: &Handle,
        _state: &mut ServerState,
        _client: ClientId,
        _object: ObjectId,
    ) {
    }
}

struct TestHarness {
    backend: Backend<ServerState>,
    server: ServerState,
    connection: Connection,
    queue: EventQueue<AppState>,
    state: AppState,
}

impl TestHarness {
    fn new() -> Self {
        let (client, server) = UnixStream::pair().unwrap();
        let backend = Backend::new().unwrap();
        backend
            .handle()
            .insert_client(server, Arc::new(()))
            .unwrap();
        backend.handle().create_global::<ServerState>(
            wm::RiverWindowManagerV1::interface(),
            4,
            Arc::new(Recorder),
        );
        let connection = Connection::from_socket(client).unwrap();
        let queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut harness = Self {
            backend,
            server: ServerState::default(),
            connection,
            queue,
            state: AppState::new(),
        };
        harness.state.anim.enabled = false;
        harness.collect_requests();
        harness.dispatch_events();
        assert!(harness.server.manager.is_some());
        harness
    }

    fn collect_requests(&mut self) {
        self.connection.flush().unwrap();
        self.backend.dispatch_all_clients(&mut self.server).unwrap();
    }

    fn dispatch_events(&mut self) {
        self.backend.flush(None).unwrap();
        self.connection.prepare_read().unwrap().read().unwrap();
        self.queue.dispatch_pending(&mut self.state).unwrap();
        self.collect_requests();
    }

    fn event(&self, sender: &ObjectId, opcode: u16, args: Vec<Argument<ObjectId, RawFd>>) {
        self.backend
            .handle()
            .send_event(Message {
                sender_id: sender.clone(),
                opcode,
                args: args.into(),
            })
            .unwrap();
    }

    fn create<I: Proxy>(&self, opcode: u16) -> ObjectId {
        let manager = self.server.manager.as_ref().unwrap();
        let handle = self.backend.handle();
        let client = handle.get_client(manager.clone()).unwrap();
        let id = handle
            .create_object::<ServerState>(client, I::interface(), 4, Arc::new(Recorder))
            .unwrap();
        self.event(manager, opcode, vec![Argument::NewId(id.clone())]);
        id
    }

    fn add_seat(&mut self) -> ObjectId {
        let id = self.create::<seat::RiverSeatV1>(wm::EVT_SEAT_OPCODE);
        self.dispatch_events();
        id
    }

    fn add_output(&mut self) -> ObjectId {
        let id = self.create::<output::RiverOutputV1>(wm::EVT_OUTPUT_OPCODE);
        self.event(
            &id,
            output::EVT_POSITION_OPCODE,
            vec![Argument::Int(0), Argument::Int(0)],
        );
        self.event(
            &id,
            output::EVT_DIMENSIONS_OPCODE,
            vec![Argument::Int(1920), Argument::Int(1080)],
        );
        self.dispatch_events();
        id
    }

    fn add_window(&mut self) -> ObjectId {
        let id = self.create::<window::RiverWindowV1>(wm::EVT_WINDOW_OPCODE);
        self.event(
            &id,
            window::EVT_APP_ID_OPCODE,
            vec![Argument::Str(Some(Box::new(c"demo".to_owned())))],
        );
        self.dispatch_events();
        id
    }

    fn manage(&mut self) {
        self.server.requests.clear();
        self.event(
            self.server.manager.as_ref().unwrap(),
            wm::EVT_MANAGE_START_OPCODE,
            vec![],
        );
        self.dispatch_events();
    }
}

#[test]
fn test_empty_outputs_produces_empty_plan() {
    let harness = TestHarness::new();
    let plan = compute_frame_plan(&harness.state);
    assert!(plan.window_plans.is_empty());
    assert!(plan.z_order_nodes.is_empty());
}

#[test]
fn test_render_plan_borders_and_colors() {
    let mut harness = TestHarness::new();
    harness.add_output();
    harness.add_seat();
    let w1 = harness.add_window();
    let w2 = harness.add_window();
    harness.manage();

    let plan = compute_frame_plan(&harness.state);
    assert_eq!(plan.window_plans.len(), 2);

    let plan1 = plan
        .window_plans
        .iter()
        .find(|p| p.proxy.id().protocol_id() == w1.protocol_id())
        .unwrap();
    let plan2 = plan
        .window_plans
        .iter()
        .find(|p| p.proxy.id().protocol_id() == w2.protocol_id())
        .unwrap();

    // w2 is focused, w1 is unfocused
    assert_eq!(
        plan2.border.color,
        (122 * 16843009, 162 * 16843009, 247 * 16843009, u32::MAX)
    );
    assert_eq!(
        plan1.border.color,
        (65 * 16843009, 72 * 16843009, 104 * 16843009, u32::MAX)
    );
    assert_eq!(plan1.border.width, 2);
    assert_eq!(plan2.border.width, 2);
}

#[test]
fn test_render_plan_oversized_opengl_anchor_compensation() {
    let mut harness = TestHarness::new();
    harness.add_output();
    let w = harness.add_window();

    // Make floating and give oversized content buffer dimensions
    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|win| win.proxy().id().protocol_id() == w.protocol_id())
    {
        win.toggle_floating(None);
        win.update_floating_geometry(xrwm::layout::Rect::new(200, 300, 400, 300));
        win.apply_client_dimensions(1280, 720, false, true, false, None);
    }
    harness.manage();

    let plan = compute_frame_plan(&harness.state);
    let win_plan = plan
        .window_plans
        .iter()
        .find(|p| p.proxy.id().protocol_id() == w.protocol_id())
        .unwrap();

    // Excess height = 720 - 300 = 420
    assert_eq!(win_plan.surface.clip_box, Some((0, 420, 400, 300)));
    // Y position offset by excess_h = 300 - 420 = -120
    assert_eq!(win_plan.surface.position, (200, -120));
}

#[test]
fn test_render_plan_z_ordering_hierarchy() {
    let mut harness = TestHarness::new();
    harness.add_output();
    let _seat = harness.add_seat();
    let w_tiled = harness.add_window();
    let w_float = harness.add_window();

    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|w| w.proxy().id().protocol_id() == w_float.protocol_id())
    {
        win.toggle_floating(None);
    }
    harness.manage();

    let plan = compute_frame_plan(&harness.state);
    assert_eq!(plan.z_order_nodes.len(), 2);

    // River classic invariant: floating window node is placed on top of tiled window node
    let node_tiled = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_tiled.protocol_id())
        .unwrap()
        .node()
        .id();
    let node_float = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_float.protocol_id())
        .unwrap()
        .node()
        .id();

    assert_eq!(plan.z_order_nodes[0].id(), node_tiled);
    assert_eq!(plan.z_order_nodes[1].id(), node_float);
}

#[test]
fn test_render_plan_captured_window_urgent_border() {
    let mut harness = TestHarness::new();
    harness.add_output();
    let _seat = harness.add_seat();
    let w1 = harness.add_window();
    let _w2 = harness.add_window();

    // Mark w1 as actively captured (e.g. river screen recording)
    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|w| w.proxy().id().protocol_id() == w1.protocol_id())
    {
        win.set_capture_sessions(1);
    }
    harness.manage();

    let plan = compute_frame_plan(&harness.state);
    let plan1 = plan
        .window_plans
        .iter()
        .find(|p| p.proxy.id().protocol_id() == w1.protocol_id())
        .unwrap();

    // w1 has BorderRole::Captured, which takes top priority with border_color_urgent (#f7768e)
    assert_eq!(
        plan1.border.color,
        (247 * 16843009, 118 * 16843009, 142 * 16843009, u32::MAX)
    );

    // Now focus w1 on the seat: BorderRole::Captured MUST still take priority over BorderRole::Focused!
    let seat = harness.state.seats.values_mut().next().unwrap();
    seat.set_focused_window(Some(harness.state.windows[0].proxy.clone()));
    harness.manage();

    let plan_focused = compute_frame_plan(&harness.state);
    let plan1_focused = plan_focused
        .window_plans
        .iter()
        .find(|p| p.proxy.id().protocol_id() == w1.protocol_id())
        .unwrap();
    assert_eq!(
        plan1_focused.border.color,
        (247 * 16843009, 118 * 16843009, 142 * 16843009, u32::MAX)
    );
}

#[test]
fn test_render_plan_five_layer_z_ordering() {
    let mut harness = TestHarness::new();
    harness.add_output();
    let _seat = harness.add_seat();

    let w_tiled = harness.add_window();
    let w_float = harness.add_window();
    let w_scratch = harness.add_window();
    let w_pinned = harness.add_window();
    let w_fullscreen = harness.add_window();

    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|w| w.proxy().id().protocol_id() == w_float.protocol_id())
    {
        win.toggle_floating(None);
    }

    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|w| w.proxy().id().protocol_id() == w_scratch.protocol_id())
    {
        win.toggle_floating(None);
        win.set_tags(xrwm::tag::TAG_SCRATCHPAD);
    }

    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|w| w.proxy().id().protocol_id() == w_pinned.protocol_id())
    {
        win.toggle_floating(None);
        win.set_tags(xrwm::tag::TAG_ALL);
    }

    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|w| w.proxy().id().protocol_id() == w_fullscreen.protocol_id())
    {
        win.toggle_fullscreen();
        win.set_tags(1);
    }

    // Tag 1 is active; also focus Scratchpad tag so scratchpad window is visible
    harness.state.tag_state.focused = 1 | xrwm::tag::TAG_SCRATCHPAD;
    harness.manage();

    let plan = compute_frame_plan(&harness.state);
    assert_eq!(plan.z_order_nodes.len(), 5);

    let id_tiled = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_tiled.protocol_id())
        .unwrap()
        .node()
        .id();
    let id_float = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_float.protocol_id())
        .unwrap()
        .node()
        .id();
    let id_scratch = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_scratch.protocol_id())
        .unwrap()
        .node()
        .id();
    let id_pinned = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_pinned.protocol_id())
        .unwrap()
        .node()
        .id();
    let id_fullscreen = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy().id().protocol_id() == w_fullscreen.protocol_id())
        .unwrap()
        .node()
        .id();

    // Hierarchy: Tiled < WorkspaceFloating < Scratchpad < Pinned < Fullscreen
    assert_eq!(plan.z_order_nodes[0].id(), id_tiled);
    assert_eq!(plan.z_order_nodes[1].id(), id_float);
    assert_eq!(plan.z_order_nodes[2].id(), id_scratch);
    assert_eq!(plan.z_order_nodes[3].id(), id_pinned);
    assert_eq!(plan.z_order_nodes[4].id(), id_fullscreen);
}

#[test]
fn test_window_visibility_resolver_multi_tag() {
    let mut harness = TestHarness::new();
    harness.add_output();
    let w = harness.add_window();

    if let Some(win) = harness
        .state
        .windows
        .iter_mut()
        .find(|win| win.proxy().id().protocol_id() == w.protocol_id())
    {
        win.toggle_floating(None);
        win.set_tags(1 | 2);
    }

    let tag_state = xrwm::tag::TagState {
        focused: 1 | 2,
        occupied: 1 | 2,
        urgent: 0,
    };

    let win_item = harness.state.windows.first().unwrap();

    // When Tag 1 is collapsed, but Tag 2 is uncollapsed:
    // Window is visible on Tag 2, so visibility MUST be Visible!
    let vis = AppState::resolve_window_visibility(tag_state, 1, win_item);
    assert_eq!(vis, xrwm::state::WindowVisibility::Visible);

    // When both Tag 1 and Tag 2 are collapsed:
    // Window is collapsed on all active tags, so visibility MUST be HiddenByCollapse!
    let vis_both = AppState::resolve_window_visibility(tag_state, 1 | 2, win_item);
    assert_eq!(vis_both, xrwm::state::WindowVisibility::HiddenByCollapse);

    // When active tags do not intersect window tags:
    let tag_state_inactive = xrwm::tag::TagState {
        focused: 4,
        occupied: 4,
        urgent: 0,
    };
    let vis_inactive = AppState::resolve_window_visibility(tag_state_inactive, 0, win_item);
    assert_eq!(vis_inactive, xrwm::state::WindowVisibility::HiddenByTags);
}
