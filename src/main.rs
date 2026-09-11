pub mod ipc;
pub mod layout;
pub mod protocol;
pub mod state;
pub mod tag;

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use state::{AppState, spawn_init_script};

fn main() {
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
        return;
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

    // 2. Spawn ~/.config/xrwm/init once daemon IPC server is ready
    spawn_init_script();

    // 3. Connect to Wayland server (River) if WAYLAND_DISPLAY is active
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        match wayland_client::Connection::connect_to_env() {
            Ok(conn) => {
                println!("Connected to Wayland compositor.");
                let mut event_queue = conn.new_event_queue();
                let _display = conn.display();

                loop {
                    if event_queue.blocking_dispatch(&mut ()).is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Wayland connection error: {e}");
            }
        }
    } else {
        println!("No WAYLAND_DISPLAY detected. Running IPC server (press Ctrl+C to stop).");
        loop {
            thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}
