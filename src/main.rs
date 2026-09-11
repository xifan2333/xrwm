pub mod animation;
pub mod ipc;
pub mod layout;
pub mod protocol;
pub mod tag;
pub mod wm;

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;

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

    let mut app_state = state.lock().unwrap();

    // Roundtrip to bind globals
    event_queue.roundtrip(&mut *app_state)?;

    if app_state.river_wm.is_none() {
        eprintln!("river_window_manager_v1 global not found! Is river running?");
        std::process::exit(1);
    }

    // 3. Spawn ~/.config/xrwm/init once daemon and Wayland protocol are ready
    spawn_init_script();

    drop(app_state);

    // 4. Main Wayland event dispatch loop
    loop {
        let mut st = state.lock().unwrap();
        if st.should_exit {
            break;
        }
        event_queue.blocking_dispatch(&mut *st)?;
    }

    Ok(())
}
