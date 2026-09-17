//! IPC protocol and UNIX domain socket client/server for xrwm.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use rustix::event::{PollFd, PollFlags, Timespec};

pub const MAX_IPC_REQUEST_BYTES: usize = 64 * 1024;
pub const IPC_TOTAL_BUDGET_MS: u64 = 15;
pub const IPC_PER_REQUEST_TIMEOUT_MS: u64 = 5;

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

const MAX_SUN_LEN: usize = 107; // 108 bytes minus null terminator

fn fnv1a_64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn is_safe_relative_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 32 {
        return false;
    }
    if name.starts_with('.') || name.starts_with('-') {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn format_socket_name(display: &str) -> String {
    let trimmed = display.trim();
    let display_str = if trimmed.is_empty() {
        "wayland-0"
    } else {
        trimmed
    };

    if is_safe_relative_name(display_str) {
        format!("xrwm-{display_str}.sock")
    } else {
        let hash = fnv1a_64(display_str.as_bytes());
        let raw_basename = Path::new(display_str)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let sanitized: String = raw_basename
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(16)
            .collect();

        let prefix = if sanitized.is_empty() {
            "display"
        } else {
            &sanitized
        };

        format!("xrwm-{prefix}-{hash:016x}.sock")
    }
}

pub fn socket_path_for_display(xdg_runtime_dir: &Path, display: &str) -> PathBuf {
    let socket_name = format_socket_name(display);
    let path = xdg_runtime_dir.join(&socket_name);
    if path.as_os_str().len() > MAX_SUN_LEN {
        PathBuf::from("/tmp").join(socket_name)
    } else {
        path
    }
}

pub fn get_socket_path() -> PathBuf {
    let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    socket_path_for_display(Path::new(&xdg), &display)
}

pub fn create_ipc_server_at(socket_path: &std::path::Path) -> std::io::Result<UnixListener> {
    if socket_path.exists() {
        match UnixStream::connect(socket_path) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("Another xrwm instance is already listening on {socket_path:?}"),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                let _ = std::fs::remove_file(socket_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e);
            }
        }
    }
    UnixListener::bind(socket_path)
}

pub fn create_ipc_server() -> std::io::Result<UnixListener> {
    let socket_path = get_socket_path();
    create_ipc_server_at(&socket_path)
}

pub struct IpcServerGuard {
    path: PathBuf,
    ino: u64,
    dev: u64,
}

impl IpcServerGuard {
    pub fn for_path(path: PathBuf) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(&path)?;
        Ok(Self {
            path,
            ino: meta.ino(),
            dev: meta.dev(),
        })
    }
}

impl Drop for IpcServerGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(&self.path)
            && meta.ino() == self.ino
            && meta.dev() == self.dev
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Converts a millisecond timeout into the [`Timespec`] expected by `poll(2)`.
fn ipc_timeout_spec(timeout_ms: i32) -> Timespec {
    let ms = timeout_ms.max(1);
    Timespec {
        tv_sec: i64::from(ms / 1000),
        tv_nsec: (i64::from(ms % 1000) * 1_000_000) as _,
    }
}

pub fn read_ipc_request(
    stream: &mut UnixStream,
    deadline: std::time::Instant,
    max_bytes: usize,
) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC request timed out",
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;

        let mut poll_fd = PollFd::from_borrowed_fd(stream.as_fd(), PollFlags::IN);
        let timeout_spec = ipc_timeout_spec(timeout_ms);

        let ready =
            match rustix::event::poll(std::slice::from_mut(&mut poll_fd), Some(&timeout_spec)) {
                Ok(ready) => ready,
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => return Err(std::io::Error::from(err)),
            };
        if ready == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC request timed out",
            ));
        }

        match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Connection closed before newline",
                ));
            }
            Ok(n) => {
                if buffer.len() + n > max_bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "IPC request exceeded maximum allowed size",
                    ));
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line = std::str::from_utf8(&buffer[..pos])
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                        .to_string();
                    return Ok(line);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

pub fn write_ipc_response(
    stream: &mut UnixStream,
    data: &[u8],
    deadline: std::time::Instant,
) -> std::io::Result<()> {
    let mut written = 0;
    while written < data.len() {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC response write timed out",
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;

        let mut poll_fd = PollFd::from_borrowed_fd(stream.as_fd(), PollFlags::OUT);
        let timeout_spec = ipc_timeout_spec(timeout_ms);

        let ready =
            match rustix::event::poll(std::slice::from_mut(&mut poll_fd), Some(&timeout_spec)) {
                Ok(ready) => ready,
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => return Err(std::io::Error::from(err)),
            };
        if ready == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC response write timed out",
            ));
        }

        match stream.write(&data[written..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "Failed to write to IPC socket",
                ));
            }
            Ok(n) => {
                written += n;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    let _ = stream.flush();
    Ok(())
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

    #[test]
    fn test_read_ipc_request_success() {
        let (mut server, mut client) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        client.write_all(b"{\"type\":\"Ping\"}\n").unwrap();
        let line = read_ipc_request(
            &mut server,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
            MAX_IPC_REQUEST_BYTES,
        )
        .unwrap();
        assert_eq!(line, "{\"type\":\"Ping\"}");
    }

    #[test]
    fn test_read_ipc_request_exceeds_max_size() {
        let (mut server, mut client) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        client.write_all(&[b'x'; 200]).unwrap();
        let res = read_ipc_request(
            &mut server,
            std::time::Instant::now() + std::time::Duration::from_millis(50),
            100,
        );
        assert!(res.is_err());
    }

    #[test]
    fn test_read_ipc_request_slow_fragmented_input_deadline() {
        let (mut server, mut client) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();
        let sender_thread = std::thread::spawn(move || {
            while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                if client.write_all(b" ").is_err() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });

        let start = std::time::Instant::now();
        let deadline = start + std::time::Duration::from_millis(20);
        let res = read_ipc_request(&mut server, deadline, MAX_IPC_REQUEST_BYTES);

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = sender_thread.join();

        let elapsed = start.elapsed();
        assert!(
            res.is_err(),
            "Expected timeout error on fragmented slow input without newline"
        );
        assert!(
            elapsed >= std::time::Duration::from_millis(15)
                && elapsed < std::time::Duration::from_millis(80),
            "Expected elapsed around 20ms, got {elapsed:?}"
        );
    }

    #[test]
    fn test_write_ipc_response_deadline_on_blocked_receiver() {
        let (mut server, _client) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();

        // Fill OS socket buffer with large buffer without reading on client side
        let big_chunk = vec![0u8; 1024 * 1024];
        let start = std::time::Instant::now();
        let deadline = start + std::time::Duration::from_millis(20);

        let res = write_ipc_response(&mut server, &big_chunk, deadline);
        let elapsed = start.elapsed();
        assert!(res.is_err(), "Expected write to timeout when buffer fills");
        assert!(
            elapsed >= std::time::Duration::from_millis(15)
                && elapsed < std::time::Duration::from_millis(80),
            "Expected elapsed around 20ms, got {elapsed:?}"
        );
    }

    #[test]
    fn test_create_ipc_server_protects_active_socket() {
        let temp_dir = std::env::temp_dir();
        let socket_path = temp_dir.join(format!("xrwm-test-active-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);

        let listener = create_ipc_server_at(&socket_path).unwrap();

        // Attempting to bind another server on the active socket must fail with AddrInUse
        let second_res = create_ipc_server_at(&socket_path);
        assert!(second_res.is_err());
        assert_eq!(
            second_res.unwrap_err().kind(),
            std::io::ErrorKind::AddrInUse
        );

        // The probe connection from second_res was queued in listener's backlog
        let (probe_conn, _) = listener.accept().unwrap();
        drop(probe_conn);

        // Verify the original listener is still active and can accept connections
        let mut client = UnixStream::connect(&socket_path).unwrap();
        client.write_all(b"test").unwrap();
        let (mut accepted, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4];
        accepted.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"test");

        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_create_ipc_server_cleans_stale_socket() {
        let temp_dir = std::env::temp_dir();
        let socket_path = temp_dir.join(format!("xrwm-test-stale-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);

        // Create a listener and immediately drop it, leaving a stale socket file
        {
            let _l = UnixListener::bind(&socket_path).unwrap();
        }
        assert!(socket_path.exists());

        // create_ipc_server_at should recognize it is stale, remove it, and successfully bind
        let listener = create_ipc_server_at(&socket_path).unwrap();
        assert!(socket_path.exists());

        let _ = std::fs::remove_file(&socket_path);
        drop(listener);
    }

    #[test]
    fn test_ipc_server_guard_drop_cleans_socket() {
        let temp_dir = std::env::temp_dir();
        let socket_path = temp_dir.join(format!("xrwm-test-guard-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).unwrap();
        assert!(socket_path.exists());

        {
            let _guard = IpcServerGuard::for_path(socket_path.clone()).unwrap();
        }
        // Guard was dropped and inode matched, file should be unlinked
        assert!(!socket_path.exists());
        drop(listener);
    }

    #[test]
    fn test_ipc_server_guard_does_not_unlink_replaced_socket() {
        let temp_dir = std::env::temp_dir();
        let socket_path = temp_dir.join(format!(
            "xrwm-test-guard-replace-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).unwrap();
        let guard = IpcServerGuard::for_path(socket_path.clone()).unwrap();

        // Simulate replacement: original file unlinked and replaced with a new inode
        let _ = std::fs::remove_file(&socket_path);
        let replacement_listener = UnixListener::bind(&socket_path).unwrap();

        // When original guard drops, it must NOT delete the replacement socket
        drop(guard);
        assert!(
            socket_path.exists(),
            "Replacement socket was incorrectly unlinked by old guard"
        );

        let _ = std::fs::remove_file(&socket_path);
        drop(listener);
        drop(replacement_listener);
    }

    #[test]
    fn test_parse_cli_args_border_color_validation() {
        assert!(parse_cli_args(&["border-color-focused".into(), "你好".into()]).is_err());
        assert!(parse_cli_args(&["border-color-focused".into(), "#123".into()]).is_err());
        assert!(parse_cli_args(&["border-color-focused".into(), "#61afef".into()]).is_ok());
    }

    #[test]
    fn test_parse_cli_args_i32_bounds_validation() {
        // Values > i32::MAX should be rejected
        assert!(parse_cli_args(&["border-width".into(), "4294967295".into()]).is_err());
        assert!(parse_cli_args(&["view-padding".into(), "4294967295".into()]).is_err());
        assert!(parse_cli_args(&["outer-padding".into(), "4294967295".into()]).is_err());

        // Valid values should succeed
        assert!(parse_cli_args(&["border-width".into(), "4".into()]).is_ok());
        assert!(parse_cli_args(&["view-padding".into(), "8".into()]).is_ok());
        assert!(parse_cli_args(&["outer-padding".into(), "12".into()]).is_ok());
    }

    #[test]
    fn test_format_socket_name_relative_display() {
        assert_eq!(format_socket_name("wayland-0"), "xrwm-wayland-0.sock");
        assert_eq!(format_socket_name("wayland-1"), "xrwm-wayland-1.sock");
        assert_eq!(
            format_socket_name("custom_display.1"),
            "xrwm-custom_display.1.sock"
        );
        // Empty or whitespace falls back to default wayland-0
        assert_eq!(format_socket_name(""), "xrwm-wayland-0.sock");
        assert_eq!(format_socket_name("   "), "xrwm-wayland-0.sock");
    }

    #[test]
    fn test_format_socket_name_absolute_paths_isolation() {
        let path1 = "/run/user/1000/wayland-0";
        let path2 = "/tmp/nested/test/wayland-0";

        let name1 = format_socket_name(path1);
        let name2 = format_socket_name(path2);

        // Neither contains path separators
        assert!(!name1.contains('/'));
        assert!(!name2.contains('/'));
        assert!(name1.starts_with("xrwm-wayland-0-"));
        assert!(name2.starts_with("xrwm-wayland-0-"));
        assert!(name1.ends_with(".sock"));
        assert!(name2.ends_with(".sock"));

        // Different absolute paths with identical basenames MUST have distinct socket names
        assert_ne!(name1, name2);

        // Deterministic: formatting the same path twice yields identical results
        assert_eq!(format_socket_name(path1), name1);
    }

    #[test]
    fn test_format_socket_name_special_characters_and_edge_cases() {
        // Path with trailing slash or no valid basename
        let root_name = format_socket_name("/");
        assert!(!root_name.contains('/'));
        assert!(root_name.starts_with("xrwm-display-"));
        assert!(root_name.ends_with(".sock"));

        // Name with special characters
        let special_name = format_socket_name("display:with!special@chars");
        assert!(!special_name.contains(':'));
        assert!(!special_name.contains('!'));
        assert!(!special_name.contains('@'));
        assert!(special_name.ends_with(".sock"));
    }

    #[test]
    fn test_socket_path_length_bounds_and_server_binding() {
        let temp_dir = std::env::temp_dir();
        // Extremely long compositor socket path (exceeding standard SUN_LEN if directly appended)
        let long_display = "/var/run/user/1000/very/deeply/nested/compositor/instance/with/a/super/long/path/hierarchy/that/would/exceed/sun_len/wayland-99.sock";

        let socket_path = socket_path_for_display(&temp_dir, long_display);

        // The overall path must be strictly within SUN_LEN limit (107 bytes)
        assert!(
            socket_path.as_os_str().len() <= MAX_SUN_LEN,
            "Socket path length {} exceeds MAX_SUN_LEN {MAX_SUN_LEN}",
            socket_path.as_os_str().len()
        );

        // Verify the generated path can actually be bound and connected
        let _ = std::fs::remove_file(&socket_path);
        let listener = create_ipc_server_at(&socket_path).unwrap();
        assert!(socket_path.exists());

        let client = UnixStream::connect(&socket_path).unwrap();
        drop(client);
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_socket_path_excessive_xdg_dir_falls_back_to_tmp() {
        // Create an excessively long XDG_RUNTIME_DIR path (> 90 chars)
        let long_xdg = Path::new(
            "/var/run/user/100000/extremely/long/runtime/dir/that/leaves/no/room/for/any/reasonable/socket/name",
        );
        let display = "/run/user/1000/wayland-0";

        let socket_path = socket_path_for_display(long_xdg, display);

        // It must safely fall back to /tmp so the total path is within bounds
        assert!(socket_path.starts_with("/tmp"));
        assert!(socket_path.as_os_str().len() <= MAX_SUN_LEN);
    }
}
