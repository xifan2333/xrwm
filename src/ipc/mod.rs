//! IPC protocol and UNIX domain socket client/server for xrwm.

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

pub mod cli;
pub mod server;

pub use cli::{parse_cli_args, send_ipc_command};
pub use server::*;

#[cfg(test)]
mod tests;
