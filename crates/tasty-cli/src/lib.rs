//! Tasty CLI — clap subcommand surface + transport/request/run dispatch.
//!
//! Phase F.B.13-3 에서 본 바이너리 src/adapters/cli/ 의 전 내용을 흡수.
#![allow(dead_code)]

pub mod commands;
pub mod cwd_resolve;
pub mod dynamic;
pub mod format;
pub mod help;
pub mod plugin;
pub mod request;
pub mod run;
pub mod stream;
pub mod transport;

use clap::{Parser, Subcommand};

pub use commands::*;
pub use help::{format_parse_error, print_augmented_help, print_command_tree};
pub use run::{run_client, try_run_plugin_cli};

#[derive(Parser)]
#[command(
    name = "tasty",
    about = "GPU-accelerated terminal emulator for AI coding agents",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Custom port file path (for test isolation)
    #[arg(long)]
    pub port_file: Option<String>,

    /// Force GUI launch even inside a tasty terminal
    #[arg(long)]
    pub launch: bool,

    /// Run as headless terminal emulator (no GUI, IPC + PTY + plugin only).
    /// Default-features 빌드에서는 GUI 부팅을 skip. no-default-features 빌드는 항상 headless.
    #[arg(long, default_value_t = false)]
    pub headless: bool,

    /// Show all commands in a tree (use with -h)
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Enable input simulation IPC (debug builds only).
    /// Required for debug.inject_mouse, debug.inject_key, etc.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub enable_input_simulation: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new resource (window, workspace, tab, split)
    New {
        #[command(subcommand)]
        command: NewCommands,
    },
    /// Close a resource (tab, pane, surface)
    Close {
        #[command(subcommand)]
        command: CloseCommands,
    },
    /// List/query resources (workspaces, windows, tree, surfaces, panes, info, hooks, etc.)
    List {
        #[command(subcommand)]
        command: ListCommands,
    },
    /// Set/update resources (hook, mark, workspace, global-hook)
    Set {
        #[command(subcommand)]
        command: SetCommands,
    },
    /// Move/reorder a resource (tab, workspace)
    Move {
        #[command(subcommand)]
        command: MoveCommands,
    },
    /// Split a pane group or surface
    Split {
        /// Split level: pane (upper layout) or surface (lower layout)
        #[arg(long)]
        level: String,
        /// Target surface: numeric surface ID, "this" (TASTY_SURFACE_ID), or nickname
        #[arg(long)]
        target_surface: Option<String>,
        /// Target pane: numeric pane ID (only for --level pane)
        #[arg(long)]
        target_pane: Option<u32>,
        /// Split direction: vertical (default) or horizontal
        #[arg(long, default_value = "vertical")]
        direction: String,
        /// Surface type: terminal (default), markdown, explorer, html, image
        #[arg(long, default_value = "terminal")]
        r#type: String,
        /// Metadata JSON to set on the new surface (e.g. '{"nickname":"build"}')
        #[arg(long)]
        meta: Option<String>,
        /// Working directory (for terminal type)
        #[arg(long)]
        cwd: Option<String>,
        /// File path (for markdown/image type)
        #[arg(long)]
        file: Option<String>,
        /// Directory path (for explorer type)
        #[arg(long)]
        path: Option<String>,
        /// URL (for html type)
        #[arg(long)]
        url: Option<String>,
    },
    /// Attach to a terminal surface and mirror it (surface 단위, 로컬 loopback)
    Attach {
        /// 대상 surface_id (포커스 비의존 — ID 직접 지정)
        surface: u32,
        /// mirror-dump: attach 후 N ms 동안 출력 수집 → mirror 화면을 stdout 출력 후 종료
        /// (GUI 없이 자동 검증용)
        #[arg(long)]
        dump_after: Option<u64>,
        /// attach 직후 1 회 전송할 입력 (escape 디코딩: \n \r \t \xNN). 비대화형 검증용
        #[arg(long)]
        send: Option<String>,
        /// raw 브리지 모드: stdin/stdout passthrough (detach = Ctrl+\)
        #[arg(long)]
        raw: bool,
        /// 점유된 surface 를 강제로 끊는다 (서버 권한, attach 하지 않음)
        #[arg(long)]
        force_detach: bool,
    },
    /// Send text, key, or queue message
    Send {
        #[command(subcommand)]
        command: SendCommands,
    },
    /// Read from surface or queue
    Read {
        #[command(subcommand)]
        command: ReadCommands,
    },
    /// Create a notification
    Notify {
        /// Notification body
        body: String,
        /// Optional notification title
        #[arg(long, default_value = "Notification")]
        title: String,
    },
    /// Remove resources (hook, global-hook)
    Unset {
        #[command(subcommand)]
        command: UnsetCommands,
    },
    /// Manage per-surface metadata
    SurfaceMeta {
        #[command(subcommand)]
        command: SurfaceMetaCommands,
    },
    /// Check if a surface is currently typing (received key input within 5 seconds)
    IsTyping {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Force-spawn the PTY of a deferred surface (restored from layout in an
    /// inactive workspace). No-op if the surface's PTY is already running.
    /// Send commands (`send text`, `send key`, ...) auto-wake the target, so
    /// this is only useful when you want the PTY running without sending any
    /// input yet.
    Wake {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Debug and diagnostic commands (IME simulation, raw key input, etc.) — debug builds only
    #[cfg(debug_assertions)]
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
    /// Internal tools (clipboard history, etc.)
    Tool {
        #[command(subcommand)]
        command: ToolCommands,
    },
    /// Manage plugins (list, install, remove, enable, disable, logs)
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
    /// Agent memory store (~/.tasty/memory.db) — persistent key-value
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
    /// Structured output observer (parsers + sink fan-out)
    Output {
        #[command(subcommand)]
        command: OutputCommands,
    },
    /// Human-handoff approval gates (request/respond/await/cancel/list/get)
    Approval {
        #[command(subcommand)]
        command: ApprovalCommands,
    },
    /// Agent telemetry (record metrics, summary, timeseries, top-N)
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommands,
    },
    /// Agent collaboration primitives (task DAG; barrier/semaphore/lease/reducer/rate-limit follow)
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
    /// File handler — 사용자 설정 (`~/.tasty/file-handlers.toml`) 재로드.
    FileHandler {
        #[command(subcommand)]
        command: FileHandlerCommands,
    },
    /// User script — `~/.tasty/init.lua` 재로드.
    Script {
        #[command(subcommand)]
        command: ScriptCommands,
    },
    /// Manage layout presets (workspace / tab / pane).
    Preset {
        #[command(subcommand)]
        command: PresetCommands,
    },
    /// Check for and install a new tasty version (standalone — no host needed)
    Update(UpdateOpts),
}
