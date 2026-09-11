pub mod animation;
pub mod ipc;
pub mod layout;
pub mod protocol;
pub mod tag;
pub mod wm;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::{AsFd, AsRawFd};

use wayland_client::Connection;
use wm::{AppState, spawn_init_script};

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

    // 1. Create IPC UNIX domain socket server
    let listener = match ipc::create_ipc_server() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind IPC socket: {e}");
            std::process::exit(1);
        }
    };
    listener.set_nonblocking(true)?;

    // 2. Connect to Wayland server (River)
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

    // 3. Spawn ~/.config/xrwm/init once daemon and Wayland protocol are ready
    spawn_init_script();

    let wayland_fd = conn.as_fd().as_raw_fd();
    let ipc_fd = listener.as_raw_fd();

    // 4. Solid single-threaded event loop with libc::poll
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

        // Frame rate animation clock: 16ms when animating (~60Hz tick), -1 when idle
        let timeout = if state.anim.is_animating() { 16 } else { -1 };

        let ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };

        if ret == 0 {
            // Animation frame tick
            drop(guard);
            state.manage_dirty();
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

            // IPC commands
            if fds[1].revents & libc::POLLIN != 0 {
                while let Ok((mut stream, _)) = listener.accept() {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() {
                        if let Ok(cmd) = serde_json::from_str::<ipc::IpcCommand>(&line) {
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
                                let _ = stream.write_all(text.as_bytes());
                                let _ = stream.write_all(b"\n");
                                let _ = stream.flush();
                                state.status_listeners.push((stream, format));
                            } else {
                                let response = match state.handle_ipc_command(&cmd) {
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
                }
            }
        } else {
            drop(guard);
        }
    }

    Ok(())
}
