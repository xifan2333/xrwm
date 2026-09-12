//! IPC protocol and UNIX domain socket client/server for xrwm.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcCommand {
    Close,
    ToggleFloat,
    ToggleFullscreen,
    Zoom,
    FocusView(String),
    FocusOutput(String),
    SendToOutput(String),
    Swap(String),
    SetFocusedTags(u32),
    ToggleFocusedTags(u32),
    SetViewTags(u32),
    ToggleViewTags(u32),
    FocusTag(u8),
    MoveToTag(u8),
    FocusPreviousTags,
    SendToPreviousTags,
    ViewPadding(u32),
    SetBorderWidth(u32),
    SetBorderColorFocused(String),
    SetBorderColorUnfocused(String),
    SetBorderColorUrgent(String),
    MainRatio(String),
    StackRatio(String),
    MainCount(String),
    MainLocation(crate::layout::MainLocation),
    SetAttachMode(crate::wm::AttachMode),
    SetCursorWarp(crate::wm::CursorWarp),
    SetFocusFollowsCursor(crate::wm::FocusFollowsCursor),
    DeclareMode(String),
    EnterMode(String),
    ResizeWindow {
        horizontal: bool,
        delta: i32,
    },
    SetAnimation(bool),
    SetAnimationDuration(u64),
    Map {
        mode: String,
        modifiers: String,
        key: String,
        action: Vec<String>,
    },
    MapPointer {
        mode: String,
        modifiers: String,
        button: String,
        action: Vec<String>,
    },
    Bind {
        combo: String,
        action: Vec<String>,
    },
    RuleAdd {
        app_id: Option<String>,
        title: Option<String>,
        action: Vec<String>,
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

    if let IpcCommand::Status { stream: true, .. } = cmd {
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            print!("{line}");
            let _ = std::io::stdout().flush();
            line.clear();
        }
        return Ok(IpcResponse::ok(""));
    }

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
        "toggle-fullscreen" | "fullscreen" => Ok(IpcCommand::ToggleFullscreen),
        "zoom" => Ok(IpcCommand::Zoom),
        "focus-view" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::FocusView(dir))
        }
        "focus-output" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::FocusOutput(dir))
        }
        "send-to-output" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::SendToOutput(dir))
        }
        "swap" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::Swap(dir))
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
        "focus-previous-tags" => Ok(IpcCommand::FocusPreviousTags),
        "send-to-previous-tags" => Ok(IpcCommand::SendToPreviousTags),
        "main-location" => {
            let loc_str = args
                .get(1)
                .ok_or("Missing location: top|bottom|left|right")?;
            let loc = match loc_str.to_ascii_lowercase().as_str() {
                "top" => crate::layout::MainLocation::Top,
                "bottom" => crate::layout::MainLocation::Bottom,
                "left" => crate::layout::MainLocation::Left,
                "right" => crate::layout::MainLocation::Right,
                _ => {
                    return Err("Invalid main location, use top|bottom|left|right".to_string());
                }
            };
            Ok(IpcCommand::MainLocation(loc))
        }
        "default-attach-mode" | "attach-mode" => {
            if args.len() < 2 {
                return Err("Missing attach mode: top|bottom|above|below|after <N>".to_string());
            }
            let raw_arg = args[1..].join(" ");
            let mode = crate::wm::AttachMode::parse(&raw_arg)?;
            Ok(IpcCommand::SetAttachMode(mode))
        }
        "set-cursor-warp" | "cursor-warp" => {
            let raw = args
                .get(1)
                .ok_or("Missing cursor warp mode: disabled|on-output-change|on-focus-change")?;
            let mode = crate::wm::CursorWarp::parse(raw)?;
            Ok(IpcCommand::SetCursorWarp(mode))
        }
        "focus-follows-cursor" => {
            let raw = args
                .get(1)
                .ok_or("Missing focus-follows-cursor mode: disabled|normal|always")?;
            let mode = crate::wm::FocusFollowsCursor::parse(raw)?;
            Ok(IpcCommand::SetFocusFollowsCursor(mode))
        }
        "view-padding" => {
            let gaps = args
                .get(1)
                .ok_or("Missing view-padding value")?
                .parse::<u32>()
                .map_err(|_| "view-padding must be a positive integer")?;
            Ok(IpcCommand::ViewPadding(gaps))
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
        "declare-mode" => {
            let mode = args.get(1).ok_or("Missing mode name")?.clone();
            Ok(IpcCommand::DeclareMode(mode))
        }
        "enter-mode" => {
            let mode = args.get(1).ok_or("Missing mode name")?.clone();
            Ok(IpcCommand::EnterMode(mode))
        }
        "resize" => {
            let orientation = args
                .get(1)
                .ok_or("Missing orientation: horizontal|vertical")?;
            let horizontal = match orientation.to_ascii_lowercase().as_str() {
                "horizontal" | "h" | "width" => true,
                "vertical" | "v" | "height" => false,
                _ => return Err("Invalid orientation, use horizontal|vertical".to_string()),
            };
            let delta = args
                .get(2)
                .ok_or("Missing delta pixels (e.g. +20, -20, 20)")?
                .parse::<i32>()
                .map_err(|_| "Delta must be an integer (e.g. 20, -20)")?;
            Ok(IpcCommand::ResizeWindow { horizontal, delta })
        }
        "main-ratio" => {
            let ratio = args.get(1).ok_or("Missing ratio value")?.clone();
            Ok(IpcCommand::MainRatio(ratio))
        }
        "stack-ratio" => {
            let ratio = args.get(1).ok_or("Missing ratio value")?.clone();
            Ok(IpcCommand::StackRatio(ratio))
        }
        "main-count" => {
            let count = args.get(1).ok_or("Missing count value")?.clone();
            Ok(IpcCommand::MainCount(count))
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
            let mut action_tokens = Vec::new();
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
                        action_tokens.push(val.to_string());
                    }
                }
                i += 1;
            }

            if action_tokens.is_empty() {
                return Err(
                    "Usage: xrwm rule-add [-app-id <id>] [-title <title>] <action> [args...]"
                        .to_string(),
                );
            }
            Ok(IpcCommand::RuleAdd {
                app_id,
                title,
                action: action_tokens,
            })
        }
        "map-pointer" => {
            if args.len() < 5 {
                return Err(
                    "Usage: xrwm map-pointer <mode> <modifiers> <button> <action...>".to_string(),
                );
            }
            Ok(IpcCommand::MapPointer {
                mode: args[1].clone(),
                modifiers: args[2].clone(),
                button: args[3].clone(),
                action: args[4..].to_vec(),
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
                action: vec!["float".into()],
            }
        );

        let dim_rule_args = vec![
            "rule-add".into(),
            "-app-id".into(),
            "mpv".into(),
            "dimensions".into(),
            "960".into(),
            "540".into(),
        ];
        assert_eq!(
            parse_cli_args(&dim_rule_args).unwrap(),
            IpcCommand::RuleAdd {
                app_id: Some("mpv".into()),
                title: None,
                action: vec!["dimensions".into(), "960".into(), "540".into()],
            }
        );

        let gaps_args = vec!["view-padding".into(), "8".into()];
        assert_eq!(
            parse_cli_args(&gaps_args).unwrap(),
            IpcCommand::ViewPadding(8)
        );

        assert_eq!(
            parse_cli_args(&["toggle-float".into()]).unwrap(),
            IpcCommand::ToggleFloat
        );
        assert_eq!(
            parse_cli_args(&[
                "map-pointer".into(),
                "normal".into(),
                "Super".into(),
                "BTN_LEFT".into(),
                "move-view".into(),
            ])
            .unwrap(),
            IpcCommand::MapPointer {
                mode: "normal".into(),
                modifiers: "Super".into(),
                button: "BTN_LEFT".into(),
                action: vec!["move-view".into()],
            }
        );
        assert_eq!(parse_cli_args(&["zoom".into()]).unwrap(), IpcCommand::Zoom);
        assert_eq!(
            parse_cli_args(&["set-focused-tags".into(), "3".into()]).unwrap(),
            IpcCommand::SetFocusedTags(3)
        );
        assert_eq!(
            parse_cli_args(&["move-to-tag".into(), "4".into()]).unwrap(),
            IpcCommand::MoveToTag(4)
        );
        assert_eq!(
            parse_cli_args(&["focus-view".into(), "next".into()]).unwrap(),
            IpcCommand::FocusView("next".into())
        );
        assert_eq!(
            parse_cli_args(&["focus-output".into(), "right".into()]).unwrap(),
            IpcCommand::FocusOutput("right".into())
        );
        assert_eq!(
            parse_cli_args(&["send-to-output".into(), "left".into()]).unwrap(),
            IpcCommand::SendToOutput("left".into())
        );
        assert_eq!(
            parse_cli_args(&["declare-mode".into(), "resize".into()]).unwrap(),
            IpcCommand::DeclareMode("resize".into())
        );
        assert_eq!(
            parse_cli_args(&["enter-mode".into(), "resize".into()]).unwrap(),
            IpcCommand::EnterMode("resize".into())
        );
        assert_eq!(
            parse_cli_args(&["resize".into(), "horizontal".into(), "20".into()]).unwrap(),
            IpcCommand::ResizeWindow {
                horizontal: true,
                delta: 20,
            }
        );
        assert_eq!(
            parse_cli_args(&["main-ratio".into(), "+0.05".into()]).unwrap(),
            IpcCommand::MainRatio("+0.05".into())
        );
        assert_eq!(
            parse_cli_args(&["stack-ratio".into(), "0.60".into()]).unwrap(),
            IpcCommand::StackRatio("0.60".into())
        );
        assert_eq!(
            parse_cli_args(&["stack-ratio".into(), "+0.05".into()]).unwrap(),
            IpcCommand::StackRatio("+0.05".into())
        );
        assert_eq!(
            parse_cli_args(&["stack-ratio".into(), "-0.05".into()]).unwrap(),
            IpcCommand::StackRatio("-0.05".into())
        );
        assert_eq!(
            parse_cli_args(&["main-count".into(), "+1".into()]).unwrap(),
            IpcCommand::MainCount("+1".into())
        );
        assert_eq!(
            parse_cli_args(&["main-ratio".into(), "0.60".into()]).unwrap(),
            IpcCommand::MainRatio("0.60".into())
        );
        assert_eq!(
            parse_cli_args(&["main-count".into(), "2".into()]).unwrap(),
            IpcCommand::MainCount("2".into())
        );
        assert_eq!(
            parse_cli_args(&["set-animation".into(), "true".into()]).unwrap(),
            IpcCommand::SetAnimation(true)
        );
        assert_eq!(
            parse_cli_args(&["animation-duration".into(), "180".into()]).unwrap(),
            IpcCommand::SetAnimationDuration(180)
        );
        assert_eq!(
            parse_cli_args(&["status".into(), "--format".into(), "waybar".into()]).unwrap(),
            IpcCommand::Status {
                stream: false,
                format: Some("waybar".into()),
            }
        );
        assert_eq!(
            parse_cli_args(&["toggle-fullscreen".into()]).unwrap(),
            IpcCommand::ToggleFullscreen
        );
        assert_eq!(
            parse_cli_args(&["focus-previous-tags".into()]).unwrap(),
            IpcCommand::FocusPreviousTags
        );
        assert_eq!(
            parse_cli_args(&["send-to-previous-tags".into()]).unwrap(),
            IpcCommand::SendToPreviousTags
        );
        assert_eq!(
            parse_cli_args(&["default-attach-mode".into(), "bottom".into()]).unwrap(),
            IpcCommand::SetAttachMode(crate::wm::AttachMode::Bottom)
        );
        assert_eq!(
            parse_cli_args(&["attach-mode".into(), "after".into(), "2".into()]).unwrap(),
            IpcCommand::SetAttachMode(crate::wm::AttachMode::After(2))
        );
        assert_eq!(
            parse_cli_args(&["set-cursor-warp".into(), "on-output-change".into()]).unwrap(),
            IpcCommand::SetCursorWarp(crate::wm::CursorWarp::OnOutputChange)
        );
        assert_eq!(
            parse_cli_args(&["cursor-warp".into(), "disabled".into()]).unwrap(),
            IpcCommand::SetCursorWarp(crate::wm::CursorWarp::Disabled)
        );
        assert_eq!(
            parse_cli_args(&["focus-follows-cursor".into(), "always".into()]).unwrap(),
            IpcCommand::SetFocusFollowsCursor(crate::wm::FocusFollowsCursor::Always)
        );
        assert_eq!(
            parse_cli_args(&["main-location".into(), "top".into()]).unwrap(),
            IpcCommand::MainLocation(crate::layout::MainLocation::Top)
        );
        assert_eq!(
            parse_cli_args(&["swap".into(), "next".into()]).unwrap(),
            IpcCommand::Swap("next".into())
        );
        assert_eq!(
            parse_cli_args(&["swap".into(), "left".into()]).unwrap(),
            IpcCommand::Swap("left".into())
        );
        assert_eq!(parse_cli_args(&["ping".into()]).unwrap(), IpcCommand::Ping);
        assert_eq!(
            parse_cli_args(&["close".into()]).unwrap(),
            IpcCommand::Close
        );
        assert_eq!(parse_cli_args(&["exit".into()]).unwrap(), IpcCommand::Exit);
        assert_eq!(
            parse_cli_args(&["reload".into()]).unwrap(),
            IpcCommand::Reload
        );
    }
}
