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
    FocusView {
        direction: String,
        skip_floating: bool,
    },
    FocusOutput(String),
    SendToOutput {
        direction: String,
        current_tags: bool,
    },
    Swap(String),
    Snap(String),
    SetFocusedTags(u32),
    ToggleFocusedTags(u32),
    SetViewTags(u32),
    ToggleViewTags(u32),
    FocusPreviousTags,
    SendToPreviousTags,
    SpawnTagmask(u32),
    ViewPadding(u32),
    OuterPadding(u32),
    BorderWidth(u32),
    BorderColorFocused(String),
    BorderColorUnfocused(String),
    BorderColorUrgent(String),
    MainRatio(String),
    StackRatio(String),
    MainCount(String),
    MainLocation(crate::layout::MainLocation),
    DefaultAttachMode(crate::wm::AttachMode),
    SetCursorWarp(crate::wm::CursorWarp),
    FocusFollowsCursor(crate::wm::FocusFollowsCursor),
    HideCursorTimeout(u64),
    HideCursorWhenTyping(bool),
    DeclareMode(String),
    EnterMode(String),
    MoveWindow {
        direction: String,
        delta: i32,
    },
    ResizeWindow {
        horizontal: bool,
        delta: i32,
    },
    Animation(bool),
    AnimationDuration(u64),
    Map {
        mode: String,
        modifiers: String,
        key: String,
        action: Vec<String>,
    },
    Unmap {
        mode: String,
        modifiers: String,
        key: String,
    },
    MapPointer {
        mode: String,
        modifiers: String,
        button: String,
        action: Vec<String>,
    },
    UnmapPointer {
        mode: String,
        modifiers: String,
        button: String,
    },
    RuleAdd {
        app_id: Option<String>,
        title: Option<String>,
        action: Vec<String>,
    },
    RuleDel {
        app_id: Option<String>,
        title: Option<String>,
        action: Vec<String>,
    },
    ListRules {
        action: Option<String>,
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
        "toggle-float" => Ok(IpcCommand::ToggleFloat),
        "toggle-fullscreen" => Ok(IpcCommand::ToggleFullscreen),
        "zoom" => Ok(IpcCommand::Zoom),
        "focus-view" => {
            let mut skip_floating = false;
            let mut direction = "next".to_string();
            for arg in &args[1..] {
                if arg == "-skip-floating" {
                    skip_floating = true;
                } else {
                    direction = arg.clone();
                }
            }
            Ok(IpcCommand::FocusView {
                direction,
                skip_floating,
            })
        }
        "focus-output" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::FocusOutput(dir))
        }
        "send-to-output" => {
            let mut current_tags = false;
            let mut direction = "next".to_string();
            for arg in &args[1..] {
                if arg == "-current-tags" {
                    current_tags = true;
                } else {
                    direction = arg.clone();
                }
            }
            Ok(IpcCommand::SendToOutput {
                direction,
                current_tags,
            })
        }
        "swap" => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "next".to_string());
            Ok(IpcCommand::Swap(dir))
        }
        "snap" => {
            let edge = args
                .get(1)
                .ok_or("Missing snap edge: left|right|up|down")?
                .clone();
            Ok(IpcCommand::Snap(edge))
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
        "focus-previous-tags" => Ok(IpcCommand::FocusPreviousTags),
        "send-to-previous-tags" => Ok(IpcCommand::SendToPreviousTags),
        "spawn-tagmask" => {
            let mask = args
                .get(1)
                .ok_or("Missing tagmask value")?
                .parse::<u32>()
                .map_err(|_| "Tagmask must be an unsigned integer")?;
            Ok(IpcCommand::SpawnTagmask(mask))
        }
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
        "default-attach-mode" => {
            if args.len() < 2 {
                return Err("Missing attach mode: top|bottom|above|below|after <N>".to_string());
            }
            let raw_arg = args[1..].join(" ");
            let mode = crate::wm::AttachMode::parse(&raw_arg)?;
            Ok(IpcCommand::DefaultAttachMode(mode))
        }
        "set-cursor-warp" => {
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
            Ok(IpcCommand::FocusFollowsCursor(mode))
        }
        "hide-cursor" => {
            let sub = args
                .get(1)
                .ok_or("Missing subcommand: timeout|when-typing")?;
            match sub.as_str() {
                "timeout" => {
                    let ms = args
                        .get(2)
                        .ok_or("Missing timeout in ms")?
                        .parse::<u64>()
                        .map_err(|_| "Timeout must be an integer in milliseconds")?;
                    Ok(IpcCommand::HideCursorTimeout(ms))
                }
                "when-typing" => {
                    let val = args.get(2).ok_or("Missing value: enabled|disabled")?;
                    let enabled = match val.to_ascii_lowercase().as_str() {
                        "enabled" | "true" | "on" | "1" => true,
                        "disabled" | "false" | "off" | "0" => false,
                        _ => return Err("Invalid value, use enabled|disabled".to_string()),
                    };
                    Ok(IpcCommand::HideCursorWhenTyping(enabled))
                }
                other => Err(format!(
                    "Unknown hide-cursor subcommand: {other}, expected timeout|when-typing"
                )),
            }
        }
        "view-padding" => {
            let gaps = args
                .get(1)
                .ok_or("Missing view-padding value")?
                .parse::<u32>()
                .map_err(|_| "view-padding must be a positive integer")?;
            Ok(IpcCommand::ViewPadding(gaps))
        }
        "outer-padding" => {
            let padding = args
                .get(1)
                .ok_or("Missing outer-padding value")?
                .parse::<u32>()
                .map_err(|_| "outer-padding must be a positive integer")?;
            Ok(IpcCommand::OuterPadding(padding))
        }
        "border-width" => {
            let width = args
                .get(1)
                .ok_or("Missing width value")?
                .parse::<u32>()
                .map_err(|_| "Width must be a positive integer")?;
            Ok(IpcCommand::BorderWidth(width))
        }
        "border-color-focused" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            Ok(IpcCommand::BorderColorFocused(color))
        }
        "border-color-unfocused" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            Ok(IpcCommand::BorderColorUnfocused(color))
        }
        "border-color-urgent" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            Ok(IpcCommand::BorderColorUrgent(color))
        }
        "declare-mode" => {
            let mode = args.get(1).ok_or("Missing mode name")?.clone();
            Ok(IpcCommand::DeclareMode(mode))
        }
        "enter-mode" => {
            let mode = args.get(1).ok_or("Missing mode name")?.clone();
            Ok(IpcCommand::EnterMode(mode))
        }
        "move" => {
            let direction = args
                .get(1)
                .ok_or("Missing direction: left|right|up|down")?
                .clone();
            let delta = args
                .get(2)
                .ok_or("Missing delta pixels (e.g. 50, -50)")?
                .parse::<i32>()
                .map_err(|_| "Delta must be an integer (e.g. 50, -50)")?;
            Ok(IpcCommand::MoveWindow { direction, delta })
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
        "animation" => {
            let val = args.get(1).ok_or("Missing boolean value")?;
            let enabled = match val.as_str() {
                "true" | "1" | "on" => true,
                "false" | "0" | "off" => false,
                _ => return Err("Invalid boolean, use true|false".to_string()),
            };
            Ok(IpcCommand::Animation(enabled))
        }
        "animation-duration" => {
            let duration = args
                .get(1)
                .ok_or("Missing duration in ms")?
                .parse::<u64>()
                .map_err(|_| "Duration must be an integer in milliseconds")?;
            Ok(IpcCommand::AnimationDuration(duration))
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
        "unmap" => {
            if args.len() < 4 {
                return Err("Usage: xrwm unmap <mode> <modifiers> <key>".to_string());
            }
            Ok(IpcCommand::Unmap {
                mode: args[1].clone(),
                modifiers: args[2].clone(),
                key: args[3].clone(),
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
        "unmap-pointer" => {
            if args.len() < 4 {
                return Err("Usage: xrwm unmap-pointer <mode> <modifiers> <button>".to_string());
            }
            Ok(IpcCommand::UnmapPointer {
                mode: args[1].clone(),
                modifiers: args[2].clone(),
                button: args[3].clone(),
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
        "rule-del" => {
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
                    "Usage: xrwm rule-del [-app-id <id>] [-title <title>] <action>".to_string(),
                );
            }
            Ok(IpcCommand::RuleDel {
                app_id,
                title,
                action: action_tokens,
            })
        }
        "list-rules" => {
            let action = args.get(1).cloned();
            Ok(IpcCommand::ListRules { action })
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

        let del_rule_args = vec![
            "rule-del".into(),
            "-app-id".into(),
            "mpv".into(),
            "float".into(),
        ];
        assert_eq!(
            parse_cli_args(&del_rule_args).unwrap(),
            IpcCommand::RuleDel {
                app_id: Some("mpv".into()),
                title: None,
                action: vec!["float".into()],
            }
        );

        assert_eq!(
            parse_cli_args(&["list-rules".into()]).unwrap(),
            IpcCommand::ListRules { action: None }
        );
        assert_eq!(
            parse_cli_args(&["list-rules".into(), "float".into()]).unwrap(),
            IpcCommand::ListRules {
                action: Some("float".into())
            }
        );

        assert_eq!(
            parse_cli_args(&["hide-cursor".into(), "timeout".into(), "3000".into()]).unwrap(),
            IpcCommand::HideCursorTimeout(3000)
        );
        assert_eq!(
            parse_cli_args(&["hide-cursor".into(), "when-typing".into(), "enabled".into()])
                .unwrap(),
            IpcCommand::HideCursorWhenTyping(true)
        );
        assert_eq!(
            parse_cli_args(&[
                "hide-cursor".into(),
                "when-typing".into(),
                "disabled".into()
            ])
            .unwrap(),
            IpcCommand::HideCursorWhenTyping(false)
        );
        assert_eq!(
            parse_cli_args(&["spawn-tagmask".into(), "511".into()]).unwrap(),
            IpcCommand::SpawnTagmask(511)
        );

        let gaps_args = vec!["view-padding".into(), "8".into()];
        assert_eq!(
            parse_cli_args(&gaps_args).unwrap(),
            IpcCommand::ViewPadding(8)
        );
        let op_args = vec!["outer-padding".into(), "12".into()];
        assert_eq!(
            parse_cli_args(&op_args).unwrap(),
            IpcCommand::OuterPadding(12)
        );

        assert_eq!(
            parse_cli_args(&["toggle-float".into()]).unwrap(),
            IpcCommand::ToggleFloat
        );
        assert_eq!(
            parse_cli_args(&["unmap".into(), "normal".into(), "Super".into(), "Q".into(),])
                .unwrap(),
            IpcCommand::Unmap {
                mode: "normal".into(),
                modifiers: "Super".into(),
                key: "Q".into(),
            }
        );
        assert_eq!(
            parse_cli_args(&[
                "unmap-pointer".into(),
                "normal".into(),
                "Super".into(),
                "BTN_LEFT".into(),
            ])
            .unwrap(),
            IpcCommand::UnmapPointer {
                mode: "normal".into(),
                modifiers: "Super".into(),
                button: "BTN_LEFT".into(),
            }
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
            parse_cli_args(&["snap".into(), "left".into()]).unwrap(),
            IpcCommand::Snap("left".into())
        );
        assert_eq!(
            parse_cli_args(&["snap".into(), "right".into()]).unwrap(),
            IpcCommand::Snap("right".into())
        );
        assert_eq!(
            parse_cli_args(&["set-focused-tags".into(), "3".into()]).unwrap(),
            IpcCommand::SetFocusedTags(3)
        );
        assert_eq!(
            parse_cli_args(&["focus-view".into(), "next".into()]).unwrap(),
            IpcCommand::FocusView {
                direction: "next".into(),
                skip_floating: false,
            }
        );
        assert_eq!(
            parse_cli_args(&["focus-view".into(), "-skip-floating".into(), "left".into()]).unwrap(),
            IpcCommand::FocusView {
                direction: "left".into(),
                skip_floating: true,
            }
        );
        assert_eq!(
            parse_cli_args(&["focus-output".into(), "right".into()]).unwrap(),
            IpcCommand::FocusOutput("right".into())
        );
        assert_eq!(
            parse_cli_args(&["send-to-output".into(), "left".into()]).unwrap(),
            IpcCommand::SendToOutput {
                direction: "left".into(),
                current_tags: false,
            }
        );
        assert_eq!(
            parse_cli_args(&[
                "send-to-output".into(),
                "-current-tags".into(),
                "right".into()
            ])
            .unwrap(),
            IpcCommand::SendToOutput {
                direction: "right".into(),
                current_tags: true,
            }
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
            parse_cli_args(&["move".into(), "left".into(), "50".into()]).unwrap(),
            IpcCommand::MoveWindow {
                direction: "left".into(),
                delta: 50,
            }
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
            parse_cli_args(&["animation".into(), "true".into()]).unwrap(),
            IpcCommand::Animation(true)
        );
        assert_eq!(
            parse_cli_args(&["animation-duration".into(), "180".into()]).unwrap(),
            IpcCommand::AnimationDuration(180)
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
            IpcCommand::DefaultAttachMode(crate::wm::AttachMode::Bottom)
        );
        assert_eq!(
            parse_cli_args(&["default-attach-mode".into(), "after".into(), "2".into()]).unwrap(),
            IpcCommand::DefaultAttachMode(crate::wm::AttachMode::After(2))
        );
        assert_eq!(
            parse_cli_args(&["set-cursor-warp".into(), "on-output-change".into()]).unwrap(),
            IpcCommand::SetCursorWarp(crate::wm::CursorWarp::OnOutputChange)
        );
        assert_eq!(
            parse_cli_args(&["set-cursor-warp".into(), "disabled".into()]).unwrap(),
            IpcCommand::SetCursorWarp(crate::wm::CursorWarp::Disabled)
        );
        assert_eq!(
            parse_cli_args(&["focus-follows-cursor".into(), "always".into()]).unwrap(),
            IpcCommand::FocusFollowsCursor(crate::wm::FocusFollowsCursor::Always)
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
