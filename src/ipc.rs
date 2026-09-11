//! IPC protocol and UNIX domain socket client/server for xrwm.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcCommand {
    Close,
    ToggleFloat,
    Zoom,
    FocusView(String),
    SetFocusedTags(u32),
    ToggleFocusedTags(u32),
    SetViewTags(u32),
    ToggleViewTags(u32),
    FocusTag(u8),
    MoveToTag(u8),
    SetWindowGaps(u32),
    SetBorderWidth(u32),
    SetBorderColorFocused(String),
    SetBorderColorUnfocused(String),
    SetBorderColorUrgent(String),
    SetSmartBorders(bool),
    SetMainRatio(f32),
    SetMainCount(u32),
    SetAnimation(bool),
    SetAnimationDuration(u64),
    Map {
        mode: String,
        modifiers: String,
        key: String,
        action: Vec<String>,
    },
    Bind {
        combo: String,
        action: Vec<String>,
    },
    RuleAdd {
        app_id: Option<String>,
        title: Option<String>,
        action: String,
    },
    Status {
        stream: bool,
        format: Option<String>,
    },
    Exit,
    Reload,
    Ping,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IpcResponse {
    pub success: bool,
    pub message: String,
}

impl IpcResponse {
    pub fn ok(msg: impl Into<String>) -> Self {
        Self {
            success: true,
            message: msg.into(),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            message: msg.into(),
        }
    }
}

pub fn get_socket_path() -> PathBuf {
    let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    PathBuf::from(xdg).join(format!("xrwm-{display}.sock"))
}

pub fn create_ipc_server() -> std::io::Result<UnixListener> {
    let socket_path = get_socket_path();
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    UnixListener::bind(&socket_path)
}

pub fn send_ipc_command(cmd: &IpcCommand) -> Result<IpcResponse, String> {
    let socket_path = get_socket_path();
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| format!("Failed to connect to xrwm at {socket_path:?}: {e}"))?;

    let json_line = serde_json::to_string(cmd).map_err(|e| e.to_string())?;
    stream
        .write_all(json_line.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(b"\n").map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|e| e.to_string())?;

    serde_json::from_str::<IpcResponse>(&response_line)
        .map_err(|e| format!("Invalid response from xrwm: {e}"))
}

pub fn parse_cli_args(args: &[String]) -> Result<IpcCommand, String> {
    if args.is_empty() {
        return Err("No arguments provided".to_string());
    }

    match args[0].as_str() {
        "ping" => Ok(IpcCommand::Ping),
        "close" => Ok(IpcCommand::Close),
        "toggle-float" | "toggle-floating" => Ok(IpcCommand::ToggleFloat),
        "zoom" => Ok(IpcCommand::Zoom),
        "focus-view" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::FocusView(dir))
        }
        "set-focused-tags" => {
            let mask = args
                .get(1)
                .ok_or("Missing tagmask value")?
                .parse::<u32>()
                .map_err(|_| "Tagmask must be an unsigned integer")?;
            Ok(IpcCommand::SetFocusedTags(mask))
        }
        "toggle-focused-tags" => {
            let mask = args
                .get(1)
                .ok_or("Missing tagmask value")?
                .parse::<u32>()
                .map_err(|_| "Tagmask must be an unsigned integer")?;
            Ok(IpcCommand::ToggleFocusedTags(mask))
        }
        "set-view-tags" => {
            let mask = args
                .get(1)
                .ok_or("Missing tagmask value")?
                .parse::<u32>()
                .map_err(|_| "Tagmask must be an unsigned integer")?;
            Ok(IpcCommand::SetViewTags(mask))
        }
        "toggle-view-tags" => {
            let mask = args
                .get(1)
                .ok_or("Missing tagmask value")?
                .parse::<u32>()
                .map_err(|_| "Tagmask must be an unsigned integer")?;
            Ok(IpcCommand::ToggleViewTags(mask))
        }
        "focus-tag" => {
            let tag = args
                .get(1)
                .ok_or("Missing tag number (1..32)")?
                .parse::<u8>()
                .map_err(|_| "Tag must be an integer between 1 and 32")?;
            if !(1..=32).contains(&tag) {
                return Err("Tag must be between 1 and 32".to_string());
            }
            Ok(IpcCommand::FocusTag(tag))
        }
        "move-to-tag" => {
            let tag = args
                .get(1)
                .ok_or("Missing tag number (1..32)")?
                .parse::<u8>()
                .map_err(|_| "Tag must be an integer between 1 and 32")?;
            if !(1..=32).contains(&tag) {
                return Err("Tag must be between 1 and 32".to_string());
            }
            Ok(IpcCommand::MoveToTag(tag))
        }
        "set-window-gaps" | "window-gaps" | "view-padding" => {
            let gaps = args
                .get(1)
                .ok_or("Missing gaps value")?
                .parse::<u32>()
                .map_err(|_| "Gaps must be a positive integer")?;
            Ok(IpcCommand::SetWindowGaps(gaps))
        }
        "set-border-width" | "border-width" => {
            let width = args
                .get(1)
                .ok_or("Missing width value")?
                .parse::<u32>()
                .map_err(|_| "Width must be a positive integer")?;
            Ok(IpcCommand::SetBorderWidth(width))
        }
        "set-border-color-focused" | "border-color-focused" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            Ok(IpcCommand::SetBorderColorFocused(color))
        }
        "set-border-color-unfocused" | "border-color-unfocused" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            Ok(IpcCommand::SetBorderColorUnfocused(color))
        }
        "set-border-color-urgent" | "border-color-urgent" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            Ok(IpcCommand::SetBorderColorUrgent(color))
        }
        "set-smart-borders" | "smart-borders" => {
            let val = args.get(1).ok_or("Missing boolean value")?;
            let enabled = match val.as_str() {
                "true" | "1" | "on" => true,
                "false" | "0" | "off" => false,
                _ => return Err("Invalid boolean, use true|false".to_string()),
            };
            Ok(IpcCommand::SetSmartBorders(enabled))
        }
        "set-main-ratio" | "main-ratio" => {
            let ratio = args
                .get(1)
                .ok_or("Missing ratio value")?
                .parse::<f32>()
                .map_err(|_| "Ratio must be a float (e.g. 0.55)")?;
            Ok(IpcCommand::SetMainRatio(ratio))
        }
        "set-main-count" | "main-count" => {
            let count = args
                .get(1)
                .ok_or("Missing count value")?
                .parse::<u32>()
                .map_err(|_| "Count must be a positive integer")?;
            Ok(IpcCommand::SetMainCount(count))
        }
        "set-animation" | "animation" => {
            let val = args.get(1).ok_or("Missing boolean value")?;
            let enabled = match val.as_str() {
                "true" | "1" | "on" => true,
                "false" | "0" | "off" => false,
                _ => return Err("Invalid boolean, use true|false".to_string()),
            };
            Ok(IpcCommand::SetAnimation(enabled))
        }
        "set-animation-duration" | "animation-duration" => {
            let duration = args
                .get(1)
                .ok_or("Missing duration in ms")?
                .parse::<u64>()
                .map_err(|_| "Duration must be an integer in milliseconds")?;
            Ok(IpcCommand::SetAnimationDuration(duration))
        }
        "exit" => Ok(IpcCommand::Exit),
        "reload" => Ok(IpcCommand::Reload),
        "map" => {
            if args.len() < 5 {
                return Err("Usage: xrwm map <mode> <modifiers> <key> <action...>".to_string());
            }
            Ok(IpcCommand::Map {
                mode: args[1].clone(),
                modifiers: args[2].clone(),
                key: args[3].clone(),
                action: args[4..].to_vec(),
            })
        }
        "bind" => {
            if args.len() < 3 {
                return Err("Usage: xrwm bind <mod+key> <action...>".to_string());
            }
            Ok(IpcCommand::Bind {
                combo: args[1].clone(),
                action: args[2..].to_vec(),
            })
        }
        "rule-add" => {
            let mut app_id = None;
            let mut title = None;
            let mut action = None;
            let mut i = 1;

            while i < args.len() {
                match args[i].as_str() {
                    "-app-id" => {
                        i += 1;
                        if i < args.len() {
                            app_id = Some(args[i].clone());
                        }
                    }
                    "-title" => {
                        i += 1;
                        if i < args.len() {
                            title = Some(args[i].clone());
                        }
                    }
                    val => {
                        action = Some(val.to_string());
                    }
                }
                i += 1;
            }

            let action = action.ok_or_else(|| {
                "Usage: xrwm rule-add [-app-id <id>] [-title <title>] <action>".to_string()
            })?;
            Ok(IpcCommand::RuleAdd {
                app_id,
                title,
                action,
            })
        }
        "status" => {
            let stream = args.iter().any(|a| a == "--stream");
            let format = args
                .iter()
                .position(|a| a == "--format")
                .and_then(|idx| args.get(idx + 1).cloned());
            Ok(IpcCommand::Status { stream, format })
        }
        other => Err(format!("Unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cli_args() {
        let args = vec![
            "map".into(),
            "normal".into(),
            "Super".into(),
            "Return".into(),
            "spawn".into(),
            "foot".into(),
        ];
        let cmd = parse_cli_args(&args).unwrap();
        assert_eq!(
            cmd,
            IpcCommand::Map {
                mode: "normal".into(),
                modifiers: "Super".into(),
                key: "Return".into(),
                action: vec!["spawn".into(), "foot".into()],
            }
        );

        let bind_args = vec!["bind".into(), "Super+Q".into(), "close".into()];
        let bind_cmd = parse_cli_args(&bind_args).unwrap();
        assert_eq!(
            bind_cmd,
            IpcCommand::Bind {
                combo: "Super+Q".into(),
                action: vec!["close".into()],
            }
        );

        let rule_args = vec![
            "rule-add".into(),
            "-app-id".into(),
            "mpv".into(),
            "float".into(),
        ];
        let rule_cmd = parse_cli_args(&rule_args).unwrap();
        assert_eq!(
            rule_cmd,
            IpcCommand::RuleAdd {
                app_id: Some("mpv".into()),
                title: None,
                action: "float".into(),
            }
        );

        let gaps_args = vec!["set-window-gaps".into(), "8".into()];
        assert_eq!(
            parse_cli_args(&gaps_args).unwrap(),
            IpcCommand::SetWindowGaps(8)
        );

        assert_eq!(
            parse_cli_args(&["toggle-float".into()]).unwrap(),
            IpcCommand::ToggleFloat
        );
        assert_eq!(parse_cli_args(&["zoom".into()]).unwrap(), IpcCommand::Zoom);
        assert_eq!(
            parse_cli_args(&["set-focused-tags".into(), "3".into()]).unwrap(),
            IpcCommand::SetFocusedTags(3)
        );
        assert_eq!(
            parse_cli_args(&["focus-tag".into(), "2".into()]).unwrap(),
            IpcCommand::FocusTag(2)
        );
    }
}
