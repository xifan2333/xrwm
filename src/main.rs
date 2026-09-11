pub mod ipc;
pub mod layout;
pub mod protocol;
pub mod tag;

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
    println!("xrwm daemon starting...");
}
