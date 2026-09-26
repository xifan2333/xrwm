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
    ToggleMonocle,
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
    Spawn(Vec<String>),
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
    let display_str = if display.trim().is_empty() {
        "wayland-0"
    } else {
        display
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
        let uid = rustix::process::getuid().as_raw();
        let xdg_hash = fnv1a_64(xdg_runtime_dir.as_os_str().as_encoded_bytes());
        let fallback_dir = PathBuf::from(format!("/tmp/xrwm-{uid}"));
        let fallback_socket_name = format!("{xdg_hash:08x}-{socket_name}");
        fallback_dir.join(fallback_socket_name)
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
    if let Some(parent) = socket_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent)?;
            }
        } else if parent.exists() {
            #[cfg(unix)]
            if parent.starts_with("/tmp/xrwm-") {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::metadata(parent)?;
                let current_uid = rustix::process::getuid().as_raw();
                if meta.uid() != current_uid {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "Fallback directory {parent:?} is owned by UID {}, expected UID {current_uid}",
                            meta.uid()
                        ),
                    ));
                }
                if (meta.mode() & 0o077) != 0 {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = meta.permissions();
                    perms.set_mode(0o700);
                    let _ = std::fs::set_permissions(parent, perms);
                }
            }
        }
    }

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

#[cfg(test)]
mod tests;
