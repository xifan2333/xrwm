pub mod animation;
pub mod ipc;
pub mod layout;
pub mod protocol;
pub mod sys;
pub mod tag;
pub mod wm;

use std::os::unix::io::{AsFd, AsRawFd};

use wayland_client::Connection;
use wm::AppState;
use wm::spawn_init_script;

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
    tracing::info!("xrwm - River 0.4+ Wayland Window Manager starting...");

    let mut state = AppState::new();

    // 1. Connect to Wayland server (River)
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let _registry = display.get_registry(&event_queue.handle(), ());

    // Roundtrip to bind globals
    event_queue.roundtrip(&mut state)?;

    if state.river_wm.is_none() {
        eprintln!("river_window_manager_v1 global not found! Is river running?");
        std::process::exit(1);
    }

    // 2. Create IPC UNIX domain socket server after confirming Wayland connection and WM ownership
    let socket_path = ipc::get_socket_path();
    let listener = match ipc::create_ipc_server_at(&socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind IPC socket: {e}");
            std::process::exit(1);
        }
    };
    let _ipc_guard = ipc::IpcServerGuard::for_path(socket_path).ok();
    listener.set_nonblocking(true)?;

    // 3. Spawn ~/.config/xrwm/init once daemon and Wayland protocol are ready
    spawn_init_script();

    let wayland_fd = conn.as_fd().as_raw_fd();
    let ipc_fd = listener.as_raw_fd();

    // 4. Solid single-threaded event loop with poll(2)
    while !state.should_exit {
        // Dispatch pending events in the queue
        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            tracing::error!("Dispatch error: {:?}", e);
            break;
        }

        // Flush outgoing requests to Wayland compositor
        let _ = event_queue.flush();

        // Prepare Wayland read guard
        let guard = match event_queue.prepare_read() {
            Some(g) => g,
            None => continue,
        };

        let mut fds = [
            libc::pollfd {
                fd: wayland_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: ipc_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        // Animation and cursor hide timeout clock
        let timeout = if state.anim.is_animating() {
            16
        } else if state.cursor_hide_timeout > 0 && !state.cursor_hidden {
            let elapsed = state.last_pointer_activity.elapsed().as_millis() as u64;
            if elapsed >= state.cursor_hide_timeout {
                0
            } else {
                (state.cursor_hide_timeout - elapsed).min(100) as i32
            }
        } else {
            -1
        };

        let ret = match sys::poll(&mut fds, timeout) {
            Ok(ready) => ready as i32,
            // A failed poll (including EINTR) is a no-op tick.
            Err(_) => -1,
        };

        if ret == 0 {
            drop(guard);
            if state.anim.is_animating() {
                state.manage_dirty();
            }
            if state.cursor_hide_timeout > 0
                && !state.cursor_hidden
                && state.last_pointer_activity.elapsed().as_millis() as u64
                    >= state.cursor_hide_timeout
            {
                state.hide_cursor();
            }
            continue;
        } else if ret > 0 {
            if fds[0].revents & libc::POLLIN != 0 {
                if let Err(e) = guard.read() {
                    tracing::error!("Read events error: {:?}", e);
                    break;
                }
            } else {
                drop(guard);
            }

            // IPC commands with bounded aggregate processing budget
            if fds[1].revents & libc::POLLIN != 0 {
                let ipc_deadline = std::time::Instant::now()
                    + std::time::Duration::from_millis(ipc::IPC_TOTAL_BUDGET_MS);
                let mut processed = 0;
                while processed < 8 && std::time::Instant::now() < ipc_deadline {
                    let Ok((mut stream, _)) = listener.accept() else {
                        break;
                    };
                    processed += 1;
                    if stream.set_nonblocking(true).is_err() {
                        continue;
                    }
                    let now = std::time::Instant::now();
                    let req_deadline = (now
                        + std::time::Duration::from_millis(ipc::IPC_PER_REQUEST_TIMEOUT_MS))
                    .min(ipc_deadline);

                    let line = match ipc::read_ipc_request(
                        &mut stream,
                        req_deadline,
                        ipc::MAX_IPC_REQUEST_BYTES,
                    ) {
                        Ok(l) => l,
                        Err(e) => {
                            tracing::debug!("Failed to read IPC request: {:?}", e);
                            continue;
                        }
                    };

                    let Ok(cmd) = serde_json::from_str::<ipc::IpcCommand>(&line) else {
                        let resp = ipc::IpcResponse::err("Malformed JSON request");
                        if let Ok(mut resp_json) = serde_json::to_string(&resp) {
                            resp_json.push('\n');
                            let write_deadline = (std::time::Instant::now()
                                + std::time::Duration::from_millis(
                                    ipc::IPC_PER_REQUEST_TIMEOUT_MS,
                                ))
                            .min(ipc_deadline);
                            let _ = ipc::write_ipc_response(
                                &mut stream,
                                resp_json.as_bytes(),
                                write_deadline,
                            );
                        }
                        continue;
                    };

                    if let ipc::IpcCommand::Status {
                        stream: true,
                        format,
                    } = cmd
                    {
                        let text = if format.as_deref() == Some("waybar") {
                            state.format_waybar_status()
                        } else {
                            state.format_json_status()
                        };
                        let mut msg = text.into_bytes();
                        msg.push(b'\n');
                        let write_deadline = (std::time::Instant::now()
                            + std::time::Duration::from_millis(ipc::IPC_PER_REQUEST_TIMEOUT_MS))
                        .min(ipc_deadline);
                        if ipc::write_ipc_response(&mut stream, &msg, write_deadline).is_ok() {
                            state.status_listeners.push((stream, format));
                        }
                    } else {
                        let response = match state.handle_ipc_command(&cmd) {
                            Ok(msg) => ipc::IpcResponse::ok(msg),
                            Err(err) => ipc::IpcResponse::err(err),
                        };
                        if let Ok(mut resp_json) = serde_json::to_string(&response) {
                            resp_json.push('\n');
                            let write_deadline = (std::time::Instant::now()
                                + std::time::Duration::from_millis(
                                    ipc::IPC_PER_REQUEST_TIMEOUT_MS,
                                ))
                            .min(ipc_deadline);
                            let _ = ipc::write_ipc_response(
                                &mut stream,
                                resp_json.as_bytes(),
                                write_deadline,
                            );
                        }
                    }
                }
            }
        } else {
            drop(guard);
        }
    }

    Ok(())
}
