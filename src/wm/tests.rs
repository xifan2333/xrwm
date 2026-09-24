//! Exercise window management through a socket-backed Wayland protocol peer.

use std::os::fd::{OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wayland_backend::protocol::{Argument, Message};
use wayland_backend::server::{
    Backend, ClientId, GlobalHandler, GlobalId, Handle, ObjectData, ObjectId,
};
use wayland_client::{Connection, EventQueue, Proxy};

use super::AppState;
use crate::ipc::IpcCommand;
use crate::protocol::{
    river_layer_shell_seat_v1 as layer_seat, river_layer_shell_v1 as layer_shell,
    river_node_v1 as node, river_output_v1 as output, river_seat_v1 as seat,
    river_window_manager_v1 as wm, river_window_v1 as window,
};
use crate::wm::LayerShellFocus;

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

struct Harness {
    backend: Backend<ServerState>,
    server: ServerState,
    connection: Connection,
    queue: EventQueue<AppState>,
    state: AppState,
}

impl Harness {
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

    fn enable_layer_shell(&mut self) {
        self.backend.handle().create_global::<ServerState>(
            layer_shell::RiverLayerShellV1::interface(),
            1,
            Arc::new(Recorder),
        );
        self.dispatch_events();
    }

    fn get_layer_shell_seat(&self) -> Option<ObjectId> {
        self.server
            .requests
            .iter()
            .find(|msg| msg.opcode == layer_shell::REQ_GET_SEAT_OPCODE)
            .and_then(|msg| {
                msg.args.iter().find_map(|arg| match arg {
                    Argument::NewId(id) => Some(id.clone()),
                    _ => None,
                })
            })
    }

    fn interact_window(&mut self, seat: &ObjectId, window: &ObjectId) {
        self.event(
            seat,
            seat::EVT_WINDOW_INTERACTION_OPCODE,
            vec![Argument::Object(window.clone())],
        );
        self.dispatch_events();
    }

    fn pointer_enter(&mut self, seat: &ObjectId, window: &ObjectId) {
        self.event(
            seat,
            seat::EVT_POINTER_ENTER_OPCODE,
            vec![Argument::Object(window.clone())],
        );
        self.dispatch_events();
    }

    fn pointer_position(&mut self, seat: &ObjectId, x: i32, y: i32) {
        self.event(
            seat,
            seat::EVT_POINTER_POSITION_OPCODE,
            vec![Argument::Int(x), Argument::Int(y)],
        );
        self.dispatch_events();
    }

    fn has_seat_request(&self, id: &ObjectId, opcode: u16) -> bool {
        self.server
            .requests
            .iter()
            .any(|msg| msg.sender_id == *id && msg.opcode == opcode)
    }

    fn all_node_positions(&self) -> Vec<(i32, i32)> {
        self.server
            .requests
            .iter()
            .filter(|msg| msg.opcode == node::REQ_SET_POSITION_OPCODE)
            .map(|msg| match msg.args.as_slice() {
                [Argument::Int(x), Argument::Int(y)] => (*x, *y),
                args => panic!("unexpected set_position arguments: {args:?}"),
            })
            .collect()
    }

    fn add_output(&mut self) {
        let _ = self.add_output_with_id();
    }

    fn add_output_with_id(&mut self) -> ObjectId {
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

    fn set_output_position(&mut self, output: &ObjectId, x: i32, y: i32) {
        self.event(
            output,
            output::EVT_POSITION_OPCODE,
            vec![Argument::Int(x), Argument::Int(y)],
        );
        self.dispatch_events();
    }

    fn set_output_dimensions(&mut self, output: &ObjectId, width: i32, height: i32) {
        self.event(
            output,
            output::EVT_DIMENSIONS_OPCODE,
            vec![Argument::Int(width), Argument::Int(height)],
        );
        self.dispatch_events();
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

    fn add_window_without_metadata(&mut self) -> ObjectId {
        let id = self.create::<window::RiverWindowV1>(wm::EVT_WINDOW_OPCODE);
        self.dispatch_events();
        id
    }

    fn set_app_id(&mut self, window: &ObjectId, app_id: &str) {
        let c_str = std::ffi::CString::new(app_id).unwrap();
        self.event(
            window,
            window::EVT_APP_ID_OPCODE,
            vec![Argument::Str(Some(Box::new(c_str)))],
        );
        self.dispatch_events();
    }

    fn set_title(&mut self, window: &ObjectId, title: &str) {
        let c_str = std::ffi::CString::new(title).unwrap();
        self.event(
            window,
            window::EVT_TITLE_OPCODE,
            vec![Argument::Str(Some(Box::new(c_str)))],
        );
        self.dispatch_events();
    }

    fn rule(&mut self, action: &[&str]) {
        self.state
            .handle_ipc_command(&IpcCommand::RuleAdd {
                app_id: Some("demo".into()),
                title: None,
                action: action.iter().map(|s| (*s).to_owned()).collect(),
            })
            .unwrap();
    }

    fn rule_with_match(&mut self, app_id: Option<&str>, title: Option<&str>, action: &[&str]) {
        self.state
            .handle_ipc_command(&IpcCommand::RuleAdd {
                app_id: app_id.map(Into::into),
                title: title.map(Into::into),
                action: action.iter().map(|s| (*s).to_owned()).collect(),
            })
            .unwrap();
    }

    fn manage(&mut self) {
        self.server.requests.clear();
        self.event(
            self.server.manager.as_ref().unwrap(),
            wm::EVT_MANAGE_START_OPCODE,
            vec![],
        );
        self.dispatch_events();
        let last = self.server.requests.last().unwrap();
        assert_eq!(Some(&last.sender_id), self.server.manager.as_ref());
        assert_eq!(last.opcode, wm::REQ_MANAGE_FINISH_OPCODE);
    }

    fn render(&mut self) {
        self.server.requests.clear();
        self.event(
            self.server.manager.as_ref().unwrap(),
            wm::EVT_RENDER_START_OPCODE,
            vec![],
        );
        self.dispatch_events();
        let last = self.server.requests.last().unwrap();
        assert_eq!(Some(&last.sender_id), self.server.manager.as_ref());
        assert_eq!(last.opcode, wm::REQ_RENDER_FINISH_OPCODE);
    }

    fn has_wm_request(&self, opcode: u16) -> bool {
        self.server
            .requests
            .iter()
            .any(|msg| Some(&msg.sender_id) == self.server.manager.as_ref() && msg.opcode == opcode)
    }

    fn has_window_request(&self, id: &ObjectId, opcode: u16) -> bool {
        self.server
            .requests
            .iter()
            .any(|msg| msg.sender_id == *id && msg.opcode == opcode)
    }

    fn has_output_request(&self, id: &ObjectId, opcode: u16) -> bool {
        self.server
            .requests
            .iter()
            .any(|msg| msg.sender_id == *id && msg.opcode == opcode)
    }

    fn proposals(&self, id: &ObjectId) -> Vec<(i32, i32)> {
        self.server
            .requests
            .iter()
            .filter(|msg| {
                msg.sender_id == *id && msg.opcode == window::REQ_PROPOSE_DIMENSIONS_OPCODE
            })
            .map(|msg| match msg.args.as_slice() {
                [Argument::Int(width), Argument::Int(height)] => (*width, *height),
                args => panic!("unexpected propose_dimensions arguments: {args:?}"),
            })
            .collect()
    }
}

#[test]
fn floating_without_dimensions_gets_one_initial_zero_proposal() {
    let mut harness = Harness::new();
    harness.add_output();
    harness.rule(&["float"]);
    let window = harness.add_window();

    harness.manage();
    assert!(harness.state.windows[0].floating);
    assert_eq!(harness.proposals(&window), [(0, 0)]);

    // An unrendered window can participate in more than one manage sequence.
    harness.manage();
    assert!(harness.proposals(&window).is_empty());

    harness.state.windows[0].height = 600;
    harness.manage();
    assert_eq!(harness.proposals(&window), [(0, 600)]);
    harness.state.windows[0].width = 800;
    harness.manage();
    assert_eq!(harness.proposals(&window), [(800, 600)]);
    harness.manage();
    assert!(harness.proposals(&window).is_empty());
}

#[test]
fn floating_dimensions_rule_is_used_for_initial_proposal() {
    for (width, height) in [(800, 600), (0, 600), (800, 0), (0, 0)] {
        let mut harness = Harness::new();
        harness.add_output();
        harness.rule(&["float"]);
        harness.rule(&["dimensions", &width.to_string(), &height.to_string()]);
        let window = harness.add_window();
        harness.manage();
        assert_eq!(harness.proposals(&window), [(width, height)]);
        harness.manage();
        assert!(harness.proposals(&window).is_empty());
    }
}

#[test]
fn tiled_window_gets_only_its_layout_proposal() {
    let mut harness = Harness::new();
    harness.add_output();
    let window = harness.add_window();
    harness.manage();
    let proposals = harness.proposals(&window);
    assert_eq!(proposals.len(), 1);
    assert!(proposals[0].0 > 0 && proposals[0].1 > 0);
    harness.manage();
    assert!(harness.proposals(&window).is_empty());
}

#[test]
fn hidden_windows_get_an_initial_proposal() {
    for floating in [false, true] {
        let mut harness = Harness::new();
        harness.add_output();
        harness.rule(&["tags", "2"]);
        if floating {
            harness.rule(&["float"]);
        }
        let window = harness.add_window();
        harness.manage();
        assert_eq!(harness.proposals(&window), [(0, 0)]);
        harness.manage();
        assert!(harness.proposals(&window).is_empty());
    }
}

#[test]
fn windows_without_an_output_get_an_initial_proposal() {
    for floating in [false, true] {
        let mut harness = Harness::new();
        if floating {
            harness.rule(&["float"]);
        }
        let window = harness.add_window();
        harness.manage();
        assert_eq!(harness.proposals(&window), [(0, 0)]);
        harness.manage();
        assert!(harness.proposals(&window).is_empty());
    }
}

#[test]
fn fullscreen_window_uses_fullscreen_instead_of_a_dimension_proposal() {
    let mut harness = Harness::new();
    harness.add_output();
    harness.rule(&["float"]);
    harness.rule(&["fullscreen"]);
    let window = harness.add_window();
    harness.manage();
    assert!(harness.proposals(&window).is_empty());
    assert_eq!(
        harness
            .server
            .requests
            .iter()
            .filter(|msg| msg.sender_id == window && msg.opcode == window::REQ_FULLSCREEN_OPCODE)
            .count(),
        1
    );
}

#[test]
fn closed_window_does_not_get_an_initial_proposal() {
    let mut harness = Harness::new();
    harness.add_output();
    harness.rule(&["float"]);
    let window = harness.add_window();
    harness.event(&window, window::EVT_CLOSED_OPCODE, vec![]);
    harness.dispatch_events();
    harness.manage();
    assert!(harness.proposals(&window).is_empty());
    assert!(harness.state.windows.is_empty());
}

#[test]
fn hidden_floating_window_does_not_loop_manage_dirty_or_restart_animation() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = true;
    harness.state.anim.duration = Duration::from_millis(150);
    harness.add_output();

    harness.rule(&["tags", "2"]);
    harness.rule(&["float"]);
    harness.rule(&["dimensions", "800", "600"]);

    let window = harness.add_window();
    assert!(!harness.state.windows[0].initial_managed);
    assert!(!harness.state.windows[0].initial_rendered);

    // Initial manage sequence
    harness.manage();
    assert!(harness.state.windows[0].initial_managed);
    assert!(!harness.state.windows[0].initial_rendered);
    assert!(!harness.state.anim.is_animating());
    assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
    assert_eq!(harness.proposals(&window), [(800, 600)]);
    assert!(harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));

    // Render sequence while window is on hidden tag
    harness.render();
    assert!(!harness.state.windows[0].initial_rendered);

    // Subsequent manage cycles should remain completely idle
    for _ in 0..3 {
        harness.manage();
        assert!(!harness.state.anim.is_animating());
        assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
        assert!(harness.proposals(&window).is_empty());
        assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
        harness.render();
        assert!(!harness.state.windows[0].initial_rendered);
    }
}

#[test]
fn hidden_floating_window_recovers_to_idle_when_existing_animation_expires() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = true;
    harness.state.anim.duration = Duration::from_millis(150);
    harness.add_output();

    // Window 1 on visible tag (tag 1)
    harness.rule(&["float"]);
    let _win1 = harness.add_window();
    harness.manage();
    assert!(harness.state.anim.is_animating());
    assert!(harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));

    // Window 2 on hidden tag (tag 2)
    harness.rule(&["tags", "2"]);
    let _win2 = harness.add_window();

    // Simulate animation duration elapsed
    harness.state.anim.start_time = Some(Instant::now() - Duration::from_millis(200));

    // Manage pass: window 2 completes initial manage without restarting animation;
    // expired animation allows event loop to recover to idle.
    harness.manage();
    assert!(!harness.state.anim.is_animating());
    assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
}

#[test]
fn hidden_floating_window_rendered_lifecycle_on_tag_switch() {
    let mut harness = Harness::new();
    harness.add_output();

    harness.rule(&["tags", "2"]);
    harness.rule(&["float"]);
    let _win = harness.add_window();

    harness.manage();
    harness.render();
    assert!(harness.state.windows[0].initial_managed);
    assert!(!harness.state.windows[0].initial_rendered);

    // Switch focus to tag 2
    harness.state.tag_state.focused = 2;
    harness.manage();
    harness.render();
    assert!(harness.state.windows[0].initial_managed);
    assert!(harness.state.windows[0].initial_rendered);
}

#[test]
fn hidden_tiled_window_lifecycle_and_tag_switch() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = true;
    harness.state.anim.duration = Duration::from_millis(150);
    harness.add_output();

    harness.rule(&["tags", "2"]);
    let window = harness.add_window();

    assert!(!harness.state.windows[0].initial_managed);
    assert!(!harness.state.windows[0].initial_rendered);

    // Initial manage sequence while tiled window is on hidden tag
    harness.manage();
    assert!(harness.state.windows[0].initial_managed);
    assert!(!harness.state.windows[0].initial_rendered);
    assert!(!harness.state.anim.is_animating());
    assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
    assert_eq!(harness.proposals(&window), [(0, 0)]);
    assert!(harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));

    harness.render();
    assert!(!harness.state.windows[0].initial_rendered);

    // Subsequent manage cycles while hidden should remain idle
    for _ in 0..2 {
        harness.manage();
        assert!(!harness.state.anim.is_animating());
        assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
        assert!(harness.proposals(&window).is_empty());
        assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    }

    // Switch focus to tag 2
    harness.state.tag_state.focused = 2;
    harness.manage();
    let proposals = harness.proposals(&window);
    assert_eq!(proposals.len(), 1);
    assert!(proposals[0].0 > 0 && proposals[0].1 > 0);

    harness.render();
    assert!(harness.state.windows[0].initial_managed);
    assert!(harness.state.windows[0].initial_rendered);
}

#[test]
fn initial_rule_csd_applies_csd_and_stays_idle() {
    let mut harness = Harness::new();
    harness.add_output();
    harness.rule(&["csd"]);
    let window = harness.add_window();

    assert_eq!(harness.state.windows[0].last_applied_ssd, None);
    assert!(!harness.state.windows[0].ssd);
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));

    harness.manage();
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(false));
    assert!(harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));

    harness.manage();
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
}

#[test]
fn async_metadata_triggers_decoration_mode_switch_and_stays_idle() {
    let mut harness = Harness::new();
    harness.add_output();

    // Configure rules: app-id "terminal" uses csd, title "*floating-dialog*" switches back to ssd
    harness.rule_with_match(Some("terminal"), None, &["csd"]);
    harness.rule_with_match(None, Some("*floating-dialog*"), &["ssd"]);

    // Window arrives without metadata initially
    let window = harness.add_window_without_metadata();
    assert_eq!(harness.state.windows[0].last_applied_ssd, None);
    assert!(harness.state.windows[0].ssd);
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));

    // Initial manage sequence applies default SSD
    harness.manage();
    assert!(harness.state.windows[0].initial_managed);
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(true));
    assert!(harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));

    // Idle manage cycles emit no redundant decoration requests
    harness.manage();
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));

    // Async app_id arrives matching the CSD rule
    harness.server.requests.clear();
    harness.set_app_id(&window, "terminal");
    assert!(harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
    assert!(!harness.state.windows[0].ssd);
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(true));

    // Manage pass applies CSD and synchronizes last_applied_ssd
    harness.manage();
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(false));
    assert!(harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));

    // Subsequent manage cycles stay idle
    for _ in 0..2 {
        harness.manage();
        assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
        assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));
    }

    // Async title arrives matching the SSD rule
    harness.server.requests.clear();
    harness.set_title(&window, "preferences floating-dialog");
    assert!(harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
    assert!(harness.state.windows[0].ssd);
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(false));

    // Manage pass applies SSD and synchronizes last_applied_ssd
    harness.manage();
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(true));
    assert!(harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));

    // Subsequent manage cycles stay idle
    for _ in 0..2 {
        harness.manage();
        assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
        assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));
    }

    // Metadata update that does not change decoration mode does not re-apply
    harness.server.requests.clear();
    harness.set_title(&window, "other title floating-dialog");
    harness.manage();
    assert_eq!(harness.state.windows[0].last_applied_ssd, Some(true));
    assert!(!harness.has_window_request(&window, window::REQ_USE_SSD_OPCODE));
    assert!(!harness.has_window_request(&window, window::REQ_USE_CSD_OPCODE));
}

#[test]
fn output_usable_area_follows_geometry_changes_without_layer_shell() {
    let mut harness = Harness::new();
    let out = harness.add_output_with_id();
    let client_out_id = harness.state.outputs.keys().next().unwrap().clone();

    let output = &harness.state.outputs[&client_out_id];
    assert_eq!(output.x, 0);
    assert_eq!(output.y, 0);
    assert_eq!(output.width, 1920);
    assert_eq!(output.height, 1080);
    assert_eq!(
        output.usable_area,
        crate::layout::Rect::new(0, 0, 1920, 1080)
    );
    assert!(!output.has_custom_usable_area);

    // 1. Moving the output updates both output.x/y and usable_area.x/y without redundant manage_dirty
    harness.server.requests.clear();
    harness.set_output_position(&out, 1920, 100);
    assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));

    let output = &harness.state.outputs[&client_out_id];
    assert_eq!(output.x, 1920);
    assert_eq!(output.y, 100);
    assert_eq!(
        output.usable_area,
        crate::layout::Rect::new(1920, 100, 1920, 1080)
    );
    assert!(!output.has_custom_usable_area);

    // 2. Resizing the output updates both output.width/height and usable_area.width/height without redundant manage_dirty
    harness.server.requests.clear();
    harness.set_output_dimensions(&out, 1280, 720);
    assert!(!harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));

    let output = &harness.state.outputs[&client_out_id];
    assert_eq!(output.width, 1280);
    assert_eq!(output.height, 720);
    assert_eq!(
        output.usable_area,
        crate::layout::Rect::new(1920, 100, 1280, 720)
    );
    assert!(!output.has_custom_usable_area);

    // 3. If custom usable area was set (e.g. by layer-shell exclusive area), geometry updates do not overwrite it
    let output_mut = harness.state.outputs.get_mut(&client_out_id).unwrap();
    output_mut.usable_area = crate::layout::Rect::new(1920, 132, 1280, 688);
    output_mut.has_custom_usable_area = true;

    harness.set_output_position(&out, 0, 0);
    let output = &harness.state.outputs[&client_out_id];
    assert_eq!(output.x, 0);
    assert_eq!(output.y, 0);
    assert_eq!(
        output.usable_area,
        crate::layout::Rect::new(1920, 132, 1280, 688)
    );
    assert!(output.has_custom_usable_area);
}

#[test]
fn layer_shell_seat_focus_lifecycle_and_window_refocus() {
    let mut harness = Harness::new();
    harness.enable_layer_shell();
    harness.add_output();
    let seat = harness.add_seat();
    let layer_seat_id = harness
        .get_layer_shell_seat()
        .expect("layer shell seat should be bound");

    let window1 = harness.add_window();
    let win1_id = harness.state.windows[0].id;
    let window2 = harness.add_window();
    let win2_id = harness.state.windows[0].id;

    // 1. Initial interaction focuses Window 1 on the seat
    harness.interact_window(&seat, &window1);
    harness.manage();
    assert!(harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
    assert_eq!(harness.state.focused_window_id(), Some(win1_id));

    // 2. Subsequent idle manage does not spam redundant focus_window
    harness.manage();
    assert!(!harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));

    // 3. Layer surface requests non-exclusive focus (e.g. wofi/fuzzel)
    harness.server.requests.clear();
    harness.event(
        &layer_seat_id,
        layer_seat::EVT_FOCUS_NON_EXCLUSIVE_OPCODE,
        vec![],
    );
    harness.dispatch_events();
    assert_eq!(
        harness.state.seats.values().next().unwrap().layer_focus,
        LayerShellFocus::NonExclusive
    );

    // During manage, WM must NOT call focus_window, respecting the layer surface's focus
    harness.manage();
    assert!(!harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
    assert_eq!(harness.state.focused_window_id(), None);

    // Subsequent manage cycles while non-exclusive layer surface is open remain idle
    for _ in 0..2 {
        harness.manage();
        assert!(!harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
        assert_eq!(harness.state.focused_window_id(), None);
    }

    // 4. Layer surface closes (focus_none arrives)
    harness.server.requests.clear();
    harness.event(&layer_seat_id, layer_seat::EVT_FOCUS_NONE_OPCODE, vec![]);
    harness.dispatch_events();
    assert_eq!(
        harness.state.seats.values().next().unwrap().layer_focus,
        LayerShellFocus::None
    );

    // WM restores focus to Window 1
    harness.manage();
    assert!(harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
    assert_eq!(harness.state.focused_window_id(), Some(win1_id));

    // 5. Subsequent manage is idle again
    harness.manage();
    assert!(!harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));

    // 6. User switches focus to Window 2 while layer surface opens
    harness.event(
        &layer_seat_id,
        layer_seat::EVT_FOCUS_NON_EXCLUSIVE_OPCODE,
        vec![],
    );
    harness.dispatch_events();
    harness.manage();
    assert!(!harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));

    // User interacts with Window 2
    harness.server.requests.clear();
    harness.interact_window(&seat, &window2);
    assert_eq!(
        harness.state.seats.values().next().unwrap().layer_focus,
        LayerShellFocus::None
    );
    harness.manage();
    assert!(harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));

    // 7. Interacting with the already-cached window reclaims focus from non-exclusive layer surface
    harness.event(
        &layer_seat_id,
        layer_seat::EVT_FOCUS_NON_EXCLUSIVE_OPCODE,
        vec![],
    );
    harness.dispatch_events();
    assert_eq!(
        harness.state.seats.values().next().unwrap().layer_focus,
        LayerShellFocus::NonExclusive
    );

    // User interacts with Window 2 (the same window that was cached)
    harness.server.requests.clear();
    harness.interact_window(&seat, &window2);
    assert_eq!(
        harness.state.seats.values().next().unwrap().layer_focus,
        LayerShellFocus::None
    );
    harness.manage();
    assert!(harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));

    // 8. Closing a window while layer surface has non-exclusive focus preserves layer focus
    harness.event(
        &layer_seat_id,
        layer_seat::EVT_FOCUS_NON_EXCLUSIVE_OPCODE,
        vec![],
    );
    harness.dispatch_events();
    harness.event(&window2, window::EVT_CLOSED_OPCODE, vec![]);
    harness.dispatch_events();
    assert_eq!(
        harness.state.seats.values().next().unwrap().layer_focus,
        LayerShellFocus::NonExclusive
    );
    assert_eq!(harness.state.focused_window_id(), None);

    // 9. Layer surface receives exclusive focus (e.g. lockscreen)
    harness.server.requests.clear();
    harness.event(
        &layer_seat_id,
        layer_seat::EVT_FOCUS_EXCLUSIVE_OPCODE,
        vec![],
    );
    harness.dispatch_events();
    harness.manage();
    assert!(!harness.has_seat_request(&seat, seat::REQ_FOCUS_WINDOW_OPCODE));
    assert_eq!(harness.state.focused_window_id(), None);
}

#[test]
fn late_bound_layer_shell_backfills_existing_seats_and_destroys_on_removal() {
    let mut harness = Harness::new();
    let seat = harness.add_seat();
    assert!(harness.get_layer_shell_seat().is_none());

    // Layer shell global is announced after the seat is created
    harness.enable_layer_shell();
    let layer_seat_id = harness
        .get_layer_shell_seat()
        .expect("existing seat should be backfilled with layer shell seat");

    // When the seat is removed, the layer-shell seat proxy and seat proxy must be destroyed
    harness.server.requests.clear();
    harness.event(&seat, seat::EVT_REMOVED_OPCODE, vec![]);
    harness.dispatch_events();
    assert!(harness.has_seat_request(&layer_seat_id, layer_seat::REQ_DESTROY_OPCODE));
    assert!(harness.has_seat_request(&seat, seat::REQ_DESTROY_OPCODE));
    assert!(harness.state.seats.is_empty());
}

#[test]
fn window_completely_offscreen_during_slide_animation_is_hidden_not_leaked() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = true;
    harness.state.anim.duration = Duration::from_secs(10);
    harness.add_output();

    // Window 1 on Tag 1
    let _win1 = harness.add_window();
    harness.manage();
    harness.render();

    // Simulate tag animation where window 1 is on old tag (1), focused tag is now 2
    harness.state.tag_state.focused = 2;
    harness.state.tag_anim_old_mask = 1;
    harness.state.tag_slide_dir = Some(crate::animation::SlideDirection::Right);
    // At progress ~0.95, the window has slid completely outside the screen (x = 0 - 1920 = -1920) while animation is actively running
    harness.state.anim.start_time = Some(Instant::now() - Duration::from_millis(9500));

    harness.render();

    let (hide_x, hide_y) = harness.state.offscreen_hiding_position();
    let positions = harness.all_node_positions();
    assert_eq!(positions.last(), Some(&(hide_x, hide_y)));
}

#[test]
fn stationary_floating_window_stays_visible_during_unrelated_animation() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = true;
    harness.state.anim.duration = Duration::from_millis(150);
    harness.add_output();

    // Floating window
    harness.rule(&["float"]);
    harness.rule(&["dimensions", "400", "300"]);
    let _win = harness.add_window();
    harness.manage();
    harness.render();

    // Simulate window dragged onto coordinates outside output bounds (e.g. x = 2000)
    harness.state.windows[0].x = 2000;
    harness.state.windows[0].y = 100;
    harness.state.windows[0].anim_start_geo = Some(crate::layout::Rect::new(2000, 100, 400, 300));
    harness.state.windows[0].anim_target_geo = Some(crate::layout::Rect::new(2000, 100, 400, 300));

    // An unrelated animation is active globally
    harness.state.anim.start_time = Some(Instant::now());

    harness.render();

    // Because this window is stationary, it should NOT be hidden
    let positions = harness.all_node_positions();
    assert_eq!(positions.last(), Some(&(2000, 100)));
}

#[test]
fn tag_toggle_and_previous_tags_history_navigation() {
    let mut harness = Harness::new();
    harness.add_output();
    let seat = harness.add_seat();
    let window = harness.add_window();
    harness.interact_window(&seat, &window);
    harness.manage();

    assert_eq!(harness.state.tag_state.focused, 1);
    assert_eq!(harness.state.previous_focused_tags, 1);
    assert_eq!(harness.state.windows[0].tags, 1);

    // 1. set-focused-tags 2
    harness
        .state
        .handle_ipc_command(&IpcCommand::SetFocusedTags(2))
        .unwrap();
    assert_eq!(harness.state.tag_state.focused, 2);
    assert_eq!(harness.state.previous_focused_tags, 1);

    // 2. toggle-focused-tags 4 -> focused becomes 6 (2 | 4)
    harness
        .state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(4))
        .unwrap();
    assert_eq!(harness.state.tag_state.focused, 6);
    assert_eq!(harness.state.previous_focused_tags, 2);

    // 3. send-to-previous-tags sends window to previous_focused_tags (2)
    harness
        .state
        .handle_ipc_command(&IpcCommand::SendToPreviousTags)
        .unwrap();
    assert_eq!(harness.state.windows[0].tags, 2);

    // 4. focus-previous-tags returns to 2
    harness
        .state
        .handle_ipc_command(&IpcCommand::FocusPreviousTags)
        .unwrap();
    assert_eq!(harness.state.tag_state.focused, 2);
    assert_eq!(harness.state.previous_focused_tags, 6);

    // 5. focus-previous-tags returns back to 6
    harness
        .state
        .handle_ipc_command(&IpcCommand::FocusPreviousTags)
        .unwrap();
    assert_eq!(harness.state.tag_state.focused, 6);
    assert_eq!(harness.state.previous_focused_tags, 2);

    // 6. No-op toggle (0) does not pollute history
    harness
        .state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(0))
        .unwrap();
    assert_eq!(harness.state.tag_state.focused, 6);
    assert_eq!(harness.state.previous_focused_tags, 2);

    // 7. Invalid toggle (would zero all tags) does not change focused tags and does not pollute history
    harness
        .state
        .handle_ipc_command(&IpcCommand::ToggleFocusedTags(6))
        .unwrap();
    assert_eq!(harness.state.tag_state.focused, 6);
    assert_eq!(harness.state.previous_focused_tags, 2);
}

#[test]
fn focus_follows_cursor_normal_vs_always_pointer_movement_in_same_window() {
    let mut harness = Harness::new();
    let seat = harness.add_seat();
    let window1 = harness.add_window();
    let win1_id = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy.id().protocol_id() == window1.protocol_id())
        .unwrap()
        .id;
    let window2 = harness.add_window();
    let win2_id = harness
        .state
        .windows
        .iter()
        .find(|w| w.proxy.id().protocol_id() == window2.protocol_id())
        .unwrap()
        .id;
    harness.manage();

    // Focus window1 via pointer enter in Normal mode
    harness.state.focus_follows_cursor = crate::wm::FocusFollowsCursor::Normal;
    harness.pointer_enter(&seat, &window1);
    assert_eq!(harness.state.focused_window_id(), Some(win1_id));

    // Move keyboard focus to window2
    harness
        .state
        .execute_action_tokens(&["focus-view".into(), "next".into()]);
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));
    // Pointer is still hovering window1
    let seat_item = harness.state.seats.values().next().unwrap();
    assert_eq!(
        seat_item.hovered.as_ref().map(|p| p.id().protocol_id()),
        Some(window1.protocol_id())
    );

    // In Normal mode: moving pointer within window1 does NOT refocus window1
    harness.pointer_position(&seat, 10, 10);
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));

    // Switch to Always mode: moving pointer within window1 DOES refocus window1
    harness.state.focus_follows_cursor = crate::wm::FocusFollowsCursor::Always;
    harness.pointer_position(&seat, 12, 12);
    assert_eq!(harness.state.focused_window_id(), Some(win1_id));

    // Move keyboard focus back to window2
    harness
        .state
        .execute_action_tokens(&["focus-view".into(), "next".into()]);
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));

    // In Disabled mode: moving pointer within window1 does NOT refocus window1
    harness.state.focus_follows_cursor = crate::wm::FocusFollowsCursor::Disabled;
    harness.pointer_position(&seat, 15, 15);
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));
}

#[test]
fn window_closed_and_output_removed_sends_protocol_destroy() {
    let mut harness = Harness::new();
    let output = harness.add_output_with_id();
    let window = harness.add_window();
    harness.manage();
    harness.render();

    // 1. Close window and verify window.destroy request is sent on manage
    harness.server.requests.clear();
    harness.event(&window, window::EVT_CLOSED_OPCODE, vec![]);
    harness.dispatch_events();
    assert_eq!(harness.state.windows.len(), 1);
    assert!(harness.state.windows[0].closed);
    harness.manage();
    assert!(harness.has_window_request(&window, window::REQ_DESTROY_OPCODE));
    assert!(harness.state.windows.is_empty());

    // 2. Remove output and verify output.destroy request is sent
    harness.server.requests.clear();
    harness.event(&output, output::EVT_REMOVED_OPCODE, vec![]);
    harness.dispatch_events();
    assert!(harness.has_output_request(&output, output::REQ_DESTROY_OPCODE));
    assert!(harness.state.outputs.is_empty());
}

#[test]
fn tiled_drag_resize_and_swap_applies_immediately_on_release_without_extra_events() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = false;
    let _out = harness.add_output_with_id();
    let _seat = harness.add_seat();

    let _win1 = harness.add_window();
    let _win2 = harness.add_window();
    harness.manage();
    harness.render();

    let win1_id = harness.state.windows[0].id;
    let win2_id = harness.state.windows[1].id;
    let initial_x1 = harness.state.windows[0].x;
    let initial_w1 = harness.state.windows[0].width;
    assert_eq!(harness.state.layout_config.split_ratio, 0.55);

    // 1. Tiled resize drag release: immediately applies final ratio & geometry, and schedules follow-up manage
    let seat_item = harness.state.seats.values_mut().next().unwrap();
    seat_item.op = crate::wm::SeatOp::TiledResize { start_ratio: 0.55 };
    seat_item.op_dx = 200;
    seat_item.op_release = true;

    harness.server.requests.clear();
    harness.manage();

    assert!(harness.state.layout_config.split_ratio > 0.60);
    assert!(harness.state.windows[0].width > initial_w1);
    assert_eq!(
        harness.state.windows[0].visual_geo.unwrap().width,
        harness.state.windows[0].width
    );
    assert!(harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));

    // 2. Tiled swap drag release: immediately swaps master order & geometry, and schedules follow-up manage
    let current_x2 = harness.state.windows[1].x;
    harness.state.pointer = (50, 50); // Inside win1 (master slot)
    let seat_item = harness.state.seats.values_mut().next().unwrap();
    seat_item.op = crate::wm::SeatOp::TiledMove {
        proxy: harness.state.windows[1].proxy.clone(),
        start_win_id: win2_id,
    };
    seat_item.op_release = true;

    harness.server.requests.clear();
    harness.manage();

    assert_eq!(harness.state.windows[0].id, win2_id);
    assert_eq!(harness.state.windows[0].x, initial_x1);
    assert_eq!(harness.state.windows[1].id, win1_id);
    assert_eq!(harness.state.windows[1].x, current_x2);
    assert_eq!(harness.state.windows[0].visual_geo.unwrap().x, initial_x1);
    assert!(harness.has_wm_request(wm::REQ_MANAGE_DIRTY_OPCODE));
}

#[test]
fn pointer_commands_execute_on_empty_desktop_and_focus_hovered_window() {
    let mut harness = Harness::new();
    let _out = harness.add_output_with_id();
    let _seat = harness.add_seat();
    harness.manage();
    harness.render();

    // 1. Empty desktop: pointer command executes without any hovered window
    assert_eq!(harness.state.tag_state.focused, 1);
    let seat_item = harness.state.seats.values_mut().next().unwrap();
    assert!(seat_item.hovered.is_none());
    seat_item.pending_action =
        crate::wm::seat::PointerAction::Command(vec!["set-focused-tags".into(), "2".into()]);

    harness.manage();
    assert_eq!(harness.state.tag_state.focused, 2);

    // Switch tags back to 1
    harness
        .state
        .execute_action_tokens(&["set-focused-tags".into(), "1".into()]);
    assert_eq!(harness.state.tag_state.focused, 1);

    // 2. Over window: pointer command focuses the hovered window and applies window command
    let win1 = harness.add_window();
    let win2 = harness.add_window();
    harness.manage();
    harness.render();

    assert_eq!(harness.state.windows.len(), 2);
    let win1_id = harness.state.windows[1].id;
    let win2_id = harness.state.windows[0].id;
    let win1_proxy = harness
        .state
        .windows
        .iter()
        .find(|w| w.id == win1_id)
        .unwrap()
        .proxy
        .clone();
    let win2_proxy = harness
        .state
        .windows
        .iter()
        .find(|w| w.id == win2_id)
        .unwrap()
        .proxy
        .clone();

    // Focus is initially win1
    let seat_item = harness.state.seats.values_mut().next().unwrap();
    seat_item.set_focused_window(Some(win1_proxy));

    // Pointer hovers win2 and triggers "close"
    let seat_item = harness.state.seats.values_mut().next().unwrap();
    seat_item.hovered = Some(win2_proxy);
    seat_item.pending_action = crate::wm::seat::PointerAction::Command(vec!["close".into()]);

    harness.manage();
    assert_eq!(harness.state.focused_window_id(), Some(win2_id));

    harness.manage();
    assert!(harness.has_window_request(&win2, window::REQ_CLOSE_OPCODE));
    assert!(!harness.has_window_request(&win1, window::REQ_CLOSE_OPCODE));

    // When win2 confirms closure, it is removed and win1 remains
    harness.event(&win2, window::EVT_CLOSED_OPCODE, vec![]);
    harness.dispatch_events();
    harness.manage();

    assert_eq!(harness.state.windows.len(), 1);
    assert_eq!(harness.state.windows[0].id, win1_id);
}

#[test]
fn dimensions_rule_combined_with_output_rule_centers_on_target_output() {
    let mut harness = Harness::new();
    harness.state.anim.enabled = false;
    let out1 = harness.add_output_with_id(); // Left screen: x=0, y=0, width=1920, height=1080
    let out2 = harness.add_output_with_id(); // Right screen
    harness.set_output_position(&out2, 1920, 0); // Position right screen at x=1920
    harness.manage();

    let out1_id = harness
        .state
        .outputs
        .iter()
        .find(|(id, _)| id.protocol_id() == out1.protocol_id())
        .unwrap()
        .0
        .clone();
    let out2_id = harness
        .state
        .outputs
        .iter()
        .find(|(id, _)| id.protocol_id() == out2.protocol_id())
        .unwrap()
        .0
        .clone();

    // Verify out2 is at x=1920
    assert_eq!(harness.state.outputs[&out2_id].x, 1920);
    assert_eq!(harness.state.outputs[&out2_id].usable_area.x, 1920);

    // Focus left screen
    harness.state.focused_output = Some(out1_id);

    // Test 1: output rule followed by dimensions rule
    harness.state.rules.clear();
    harness
        .state
        .handle_ipc_command(&IpcCommand::RuleAdd {
            app_id: Some("target_right".into()),
            title: None,
            action: vec!["output".into(), out2_id.to_string()],
        })
        .unwrap();
    harness
        .state
        .handle_ipc_command(&IpcCommand::RuleAdd {
            app_id: Some("target_right".into()),
            title: None,
            action: vec!["dimensions".into(), "800".into(), "600".into()],
        })
        .unwrap();

    let win = harness.add_window();
    harness.set_app_id(&win, "target_right");
    let win_item = harness
        .state
        .windows
        .iter()
        .find(|w| w.app_id.as_deref() == Some("target_right"))
        .unwrap();
    assert_eq!(win_item.output, Some(out2_id.clone()));
    assert_eq!(win_item.width, 800);
    assert_eq!(win_item.height, 600);
    assert_eq!(win_item.x, 2480);
    assert_eq!(win_item.y, 240);

    // Test 2: dimensions rule defined BEFORE output rule (reversed order)
    harness.state.rules.clear();
    harness
        .state
        .handle_ipc_command(&IpcCommand::RuleAdd {
            app_id: Some("rev_order".into()),
            title: None,
            action: vec!["dimensions".into(), "800".into(), "600".into()],
        })
        .unwrap();
    harness
        .state
        .handle_ipc_command(&IpcCommand::RuleAdd {
            app_id: Some("rev_order".into()),
            title: None,
            action: vec!["output".into(), out2_id.to_string()],
        })
        .unwrap();

    let win_rev = harness.add_window();
    harness.set_app_id(&win_rev, "rev_order");
    let win_rev_item = harness
        .state
        .windows
        .iter()
        .find(|w| w.app_id.as_deref() == Some("rev_order"))
        .unwrap();
    assert_eq!(win_rev_item.output, Some(out2_id.clone()));
    assert_eq!(win_rev_item.x, 2480);
    assert_eq!(win_rev_item.y, 240);

    // Test 3: target output with exclusive layer-shell margins
    if let Some(out2_item) = harness.state.outputs.get_mut(&out2_id) {
        out2_item.has_custom_usable_area = true;
        out2_item.usable_area = crate::layout::Rect::new(1920, 30, 1920, 1050);
    }

    let win_margins = harness.add_window();
    harness.set_app_id(&win_margins, "target_margins");
    harness
        .state
        .handle_ipc_command(&IpcCommand::RuleAdd {
            app_id: Some("target_margins".into()),
            title: None,
            action: vec!["output".into(), out2_id.to_string()],
        })
        .unwrap();
    harness
        .state
        .handle_ipc_command(&IpcCommand::RuleAdd {
            app_id: Some("target_margins".into()),
            title: None,
            action: vec!["dimensions".into(), "800".into(), "600".into()],
        })
        .unwrap();
    // Re-apply rules via set_title
    harness.set_title(&win_margins, "update");
    let win_margins_item = harness
        .state
        .windows
        .iter()
        .find(|w| w.app_id.as_deref() == Some("target_margins"))
        .unwrap();
    assert_eq!(win_margins_item.x, 2480);
    assert_eq!(win_margins_item.y, 255); // 30 + (1050 - 600) / 2 = 255
}
