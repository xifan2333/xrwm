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
    river_output_v1 as output, river_window_manager_v1 as wm, river_window_v1 as window,
};

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
        state.manager = Some(object);
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

    fn add_output(&mut self) {
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
