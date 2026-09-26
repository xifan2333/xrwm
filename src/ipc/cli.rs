use super::server::get_socket_path;
use super::{IpcCommand, IpcResponse};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

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

fn validate_ratio_arg(arg: &str) -> Result<(), String> {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return Err("Ratio value cannot be empty".to_string());
    }
    let val = trimmed
        .parse::<f32>()
        .map_err(|_| "Ratio must be a valid float (e.g. 0.55, +0.05, -0.05)")?;
    if !val.is_finite() {
        return Err("Ratio must be a finite number".to_string());
    }
    Ok(())
}

struct ParsedRuleArgs {
    app_id: Option<String>,
    title: Option<String>,
    action: Vec<String>,
}

fn parse_rule_args(args: &[String], is_add: bool) -> Result<ParsedRuleArgs, String> {
    let cmd_name = if is_add { "rule-add" } else { "rule-del" };
    let mut app_id = None;
    let mut title = None;
    let mut action_tokens = Vec::new();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-app-id" => {
                i += 1;
                if i < args.len() {
                    let val = &args[i];
                    if val == "-title" || val == "-app-id" {
                        return Err(format!("Missing value for -app-id in {cmd_name}"));
                    }
                    app_id = Some(val.clone());
                } else {
                    return Err(format!("Missing value for -app-id in {cmd_name}"));
                }
            }
            "-title" => {
                i += 1;
                if i < args.len() {
                    let val = &args[i];
                    if val == "-app-id" || val == "-title" {
                        return Err(format!("Missing value for -title in {cmd_name}"));
                    }
                    title = Some(val.clone());
                } else {
                    return Err(format!("Missing value for -title in {cmd_name}"));
                }
            }
            val => {
                action_tokens.push(val.to_string());
            }
        }
        i += 1;
    }

    if action_tokens.is_empty() {
        let usage = if is_add {
            "Usage: xrwm rule-add [-app-id <id>] [-title <title>] <action> [args...]"
        } else {
            "Usage: xrwm rule-del [-app-id <id>] [-title <title>] <action>"
        };
        return Err(usage.to_string());
    }

    Ok(ParsedRuleArgs {
        app_id,
        title,
        action: action_tokens,
    })
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
        "toggle-monocle" => Ok(IpcCommand::ToggleMonocle),
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
            if gaps > i32::MAX as u32 {
                return Err("view-padding exceeds maximum allowed value".to_string());
            }
            Ok(IpcCommand::ViewPadding(gaps))
        }
        "outer-padding" => {
            let padding = args
                .get(1)
                .ok_or("Missing outer-padding value")?
                .parse::<u32>()
                .map_err(|_| "outer-padding must be a positive integer")?;
            if padding > i32::MAX as u32 {
                return Err("outer-padding exceeds maximum allowed value".to_string());
            }
            Ok(IpcCommand::OuterPadding(padding))
        }
        "border-width" => {
            let width = args
                .get(1)
                .ok_or("Missing width value")?
                .parse::<u32>()
                .map_err(|_| "Width must be a positive integer")?;
            if width > i32::MAX as u32 {
                return Err("border-width exceeds maximum allowed value".to_string());
            }
            Ok(IpcCommand::BorderWidth(width))
        }
        "border-color-focused" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            crate::wm::state::parse_hex_color(&color)?;
            Ok(IpcCommand::BorderColorFocused(color))
        }
        "border-color-unfocused" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            crate::wm::state::parse_hex_color(&color)?;
            Ok(IpcCommand::BorderColorUnfocused(color))
        }
        "border-color-urgent" => {
            let color = args.get(1).ok_or("Missing color value")?.clone();
            crate::wm::state::parse_hex_color(&color)?;
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
            validate_ratio_arg(&ratio)?;
            Ok(IpcCommand::MainRatio(ratio))
        }
        "stack-ratio" => {
            let ratio = args.get(1).ok_or("Missing ratio value")?.clone();
            validate_ratio_arg(&ratio)?;
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
        "spawn" => {
            if args.len() < 2 {
                return Err("Usage: xrwm spawn <command> [args...]".to_string());
            }
            Ok(IpcCommand::Spawn(args[1..].to_vec()))
        }
        "map" => {
            if args.len() < 5 {
                return Err("Usage: xrwm map <mode> <modifiers> <key> <action...>".to_string());
            }
            crate::wm::binds::parse_modifiers(&args[2])?;
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
            crate::wm::binds::parse_modifiers(&args[2])?;
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
            crate::wm::binds::parse_modifiers(&args[2])?;
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
            crate::wm::binds::parse_modifiers(&args[2])?;
            Ok(IpcCommand::UnmapPointer {
                mode: args[1].clone(),
                modifiers: args[2].clone(),
                button: args[3].clone(),
            })
        }
        "rule-add" => {
            let parsed = parse_rule_args(args, true)?;
            Ok(IpcCommand::RuleAdd {
                app_id: parsed.app_id,
                title: parsed.title,
                action: parsed.action,
            })
        }
        "rule-del" => {
            let parsed = parse_rule_args(args, false)?;
            Ok(IpcCommand::RuleDel {
                app_id: parsed.app_id,
                title: parsed.title,
                action: parsed.action,
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
