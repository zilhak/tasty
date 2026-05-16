pub mod dynamic;
mod format;
mod plugin;
mod request;
mod transport;

use plugin::run_plugin_logs;

use std::net::TcpStream;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::ipc::server::IpcServer;

use format::format_output;
use request::command_to_request;
use transport::IpcConnection;

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
}

#[derive(Subcommand)]
pub enum PluginCommands {
    /// List installed plugins (id, version, enabled, running).
    List,
    /// Show full manifest, permissions, commands, and runtime state for a plugin.
    Show {
        /// Plugin id.
        id: String,
    },
    /// Install a plugin from a directory containing tasty-plugin.toml.
    Install {
        /// Path to plugin directory (must contain tasty-plugin.toml).
        path: String,
    },
    /// Remove an installed plugin by id.
    Remove {
        /// Plugin id (e.g. com.example.explorer).
        id: String,
    },
    /// Enable a disabled plugin and start it.
    Enable {
        id: String,
    },
    /// Disable a plugin (graceful shutdown if running).
    Disable {
        id: String,
    },
    /// Print the contents of a plugin's log file.
    Logs {
        /// Plugin id.
        id: String,
        /// Tail and follow new output (Ctrl-C to stop).
        #[arg(long)]
        follow: bool,
    },
    /// Show a plugin's manifest permissions and currently granted set.
    Permissions {
        /// Plugin id.
        id: String,
    },
    /// Grant a permission to a plugin (must be declared in its manifest).
    Grant {
        /// Plugin id.
        id: String,
        /// Permission token (e.g. fs.read, surface.write).
        permission: String,
    },
    /// Revoke a previously-granted permission from a plugin.
    Revoke {
        /// Plugin id.
        id: String,
        /// Permission token.
        permission: String,
    },
    /// Inspect plugin extensions (extends-blocks).
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },
}

#[derive(Subcommand)]
pub enum ExtensionCommands {
    /// List all extensions and their current state.
    List,
}

#[derive(Subcommand)]
pub enum ToolCommands {
    /// Clipboard history operations (list, get, paste, remove, clear).
    Clipboard {
        #[command(subcommand)]
        command: ClipboardCommands,
    },
}

/// Agent memory CLI. Scope formats:
/// `global`, `account:<userid>`, `window:<id>`, `workspace:<id>`, `surface:<id>`.
/// `--surface <id>` 같은 alias가 대응 scope로 정규화된다.
#[derive(Subcommand)]
pub enum MemoryCommands {
    /// Store a value at scope/key. Default content type inferred from value
    /// (string → text/plain, JSON literal → application/json).
    Put {
        /// Scope token (`global`, `surface:3`, `workspace:7`, ...).
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        /// Alias: `--surface 3` → `surface:3`.
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        /// Alias: `--workspace 7` → `workspace:7`.
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        /// Alias: `--window 42` → `window:42`.
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        /// Alias: `--account zilhak` → `account:zilhak`.
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        /// Alias: `--global` → `global`.
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        /// Key (1..256 `[a-z0-9._-]+`).
        #[arg(long)]
        key: String,
        /// Value. Treated as JSON if it parses, otherwise as a plain text string.
        /// Prefix with `@path` to read from a file (UTF-8 only; binary needs --value-b64).
        #[arg(long)]
        value: Option<String>,
        /// Base64-encoded binary payload. Overrides --value.
        #[arg(long)]
        value_b64: Option<String>,
        /// Force content type. Defaults: text/plain (string), application/json (JSON literal),
        /// application/octet-stream (with --value-b64).
        #[arg(long)]
        content_type: Option<String>,
        /// Relative TTL in seconds (entry expires `now + ttl` ms). Conflicts with --expires-at.
        #[arg(long, conflicts_with = "expires_at")]
        ttl: Option<u64>,
        /// Absolute expiry timestamp (unix ms). No-op if omitted.
        #[arg(long)]
        expires_at: Option<i64>,
        /// CAS version (must match current entry, otherwise cas_conflict).
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Read a single entry.
    Get {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
    },
    /// Delete a key.
    Delete {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
        /// CAS version; if specified and mismatched, returns cas_conflict.
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Check existence.
    Exists {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
    },
    /// List entries in a scope (prefix + since/until/limit/offset).
    List {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        /// Only entries with `updated_at >= since` (unix ms).
        #[arg(long)]
        since: Option<i64>,
        /// Only entries with `updated_at < until` (unix ms).
        #[arg(long)]
        until: Option<i64>,
        /// Skip the first N matching entries (use with --limit for pagination).
        #[arg(long)]
        offset: Option<usize>,
    },
    /// Filter JSON entries by a dot-path equality (`--path a.b --equals <json>`).
    Query {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        /// Dot path, e.g. `"task.status"`. Only `application/json` entries are inspected.
        #[arg(long)]
        path: String,
        /// JSON literal (or quoted string) to compare for equality.
        #[arg(long)]
        equals: String,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        since: Option<i64>,
        #[arg(long)]
        until: Option<i64>,
        #[arg(long)]
        offset: Option<usize>,
    },
    /// Export regular entries to JSON (optional `--scope` filter). Secret area is
    /// never exported.
    Export {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
    },
    /// Import regular entries from a JSON file (output of `memory export`).
    /// `--replace` overwrites existing keys; default skips conflicts.
    Import {
        /// Path to JSON file (entries array, or `{ "entries": [...] }`).
        #[arg(long)]
        file: String,
        /// Overwrite existing keys (default: skip).
        #[arg(long)]
        replace: bool,
    },
    /// Count entries in a scope (prefix optional).
    Count {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        prefix: Option<String>,
    },
    /// List scopes currently in use.
    Scopes,
    /// Stats: total entries + bytes (per scope or aggregate).
    Stats {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
    },
    /// Garbage-collect expired entries (regular + secret). Reads already filter
    /// expired rows; this only reclaims disk + quota. Local-only.
    Gc,
    /// Secret memory store. CLI acts as `_host` owner; no --owner flag exists.
    /// Plugin secret areas are inaccessible from the CLI by design.
    Secret {
        #[command(subcommand)]
        command: MemorySecretCommands,
    },
}

/// `tasty memory secret ...` 서브커맨드. CLI 는 항상 `_host` owner 로 동작하며,
/// plugin 영역 접근 경로는 IPC 표면에 존재하지 않는다 (다른 plugin secret 의
/// 존재 자체가 개념적으로 보이지 않는 모델).
#[derive(Subcommand)]
pub enum MemorySecretCommands {
    /// Store a secret value at scope/key.
    Put {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        value_b64: Option<String>,
        #[arg(long)]
        content_type: Option<String>,
        /// Relative TTL in seconds. Conflicts with --expires-at.
        #[arg(long, conflicts_with = "expires_at")]
        ttl: Option<u64>,
        #[arg(long)]
        expires_at: Option<i64>,
        #[arg(long)]
        cas: Option<u64>,
    },
    Get {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
    },
    Delete {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
        #[arg(long)]
        cas: Option<u64>,
    },
    Exists {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        key: String,
    },
    List {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Count {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
        #[arg(long)]
        prefix: Option<String>,
    },
    Scopes,
    Stats {
        #[arg(long, conflicts_with_all = ["surface", "workspace", "window", "account", "global"])]
        scope: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "workspace", "window", "account", "global"])]
        surface: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "window", "account", "global"])]
        workspace: Option<u32>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "account", "global"])]
        window: Option<u64>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "global"])]
        account: Option<String>,
        #[arg(long, conflicts_with_all = ["scope", "surface", "workspace", "window", "account"])]
        global: bool,
    },
}

#[derive(Subcommand)]
pub enum ClipboardCommands {
    /// List clipboard history (newest first).
    List {
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Print the text at a specific index to stdout (0 = newest).
    Get {
        #[arg(long)]
        index: usize,
    },
    /// Copy the entry at a specific index back to the system clipboard.
    Paste {
        #[arg(long)]
        index: usize,
    },
    /// Remove a specific entry.
    Remove {
        #[arg(long)]
        index: usize,
    },
    /// Clear all clipboard history.
    Clear,
}

#[derive(Subcommand)]
pub enum ListCommands {
    /// List workspaces
    Workspaces,
    /// List all windows
    Windows,
    /// Show tree view of workspaces, panes, and tabs
    Tree,
    /// List surfaces (terminals) across all workspaces
    Surfaces,
    /// List panes across all workspaces
    Panes,
    /// List tabs in a pane
    Tabs {
        /// Pane ID (required)
        #[arg(long)]
        pane: u32,
    },
    /// Show system info
    Info,
    /// List notifications
    Notifications,
    /// List hooks
    Hooks {
        /// Filter by surface ID
        #[arg(long)]
        surface: Option<u32>,
    },
    /// List all global hooks
    GlobalHooks,
    /// Show queue status (count + preview of pending messages)
    Queue {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum SetCommands {
    /// Set a hook on a surface
    Hook {
        /// Surface ID to hook (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Event type: process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS
        #[arg(long)]
        event: String,
        /// Shell command to execute when the event fires
        #[arg(long)]
        command: String,
        /// Remove the hook after it fires once
        #[arg(long)]
        once: bool,
    },
    /// Set a read mark on a surface
    Mark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Update workspace name, subtitle, or description
    Workspace {
        /// Workspace ID (required)
        #[arg(long)]
        id: u32,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New subtitle
        #[arg(long)]
        subtitle: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Set a global hook (timer or file-watching)
    GlobalHook {
        /// Condition: interval:SECS, once:SECS, file:/path/to/watch
        #[arg(long)]
        condition: String,
        /// Shell command to execute when the condition fires
        #[arg(long)]
        command: String,
        /// Optional human-readable label
        #[arg(long)]
        label: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MoveCommands {
    /// Move a tab to a different position within the same pane
    Tab {
        /// Pane ID
        #[arg(long)]
        pane: u32,
        /// Source tab index (0-based)
        #[arg(long)]
        from: u64,
        /// Destination tab index (0-based)
        #[arg(long)]
        to: u64,
    },
    /// Move a workspace to a different position
    Workspace {
        /// Source workspace index (0-based)
        #[arg(long)]
        from: u64,
        /// Destination workspace index (0-based)
        #[arg(long)]
        to: u64,
    },
}

#[derive(Subcommand)]
pub enum SendCommands {
    /// Send text to a terminal surface
    Text {
        /// Text to send
        text: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Send a key to a terminal surface (enter, tab, escape, up, down, etc.)
    Key {
        /// Key name
        key: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Send a message to a surface's queue
    Queue {
        /// Target surface ID
        #[arg(long)]
        to: u32,
        /// Message content
        #[arg()]
        content: String,
        /// Sender surface ID (default: focused)
        #[arg(long)]
        from: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum ReadCommands {
    /// Read output since last mark
    #[command(name = "since-mark")]
    SinceMark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Strip ANSI escape sequences from output
        #[arg(long)]
        strip_ansi: bool,
    },
    /// Parse output since last mark with builtin parsers (path/url/prompt_boundary/exit_code)
    #[command(name = "parse-since-mark")]
    ParseSinceMark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Comma-separated parser ids. Default = all builtins.
        #[arg(long, value_delimiter = ',')]
        parsers: Option<Vec<String>>,
    },
    /// Read from a surface's message queue (consumes oldest message)
    Queue {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Filter by sender surface ID
        #[arg(long)]
        from: Option<u32>,
        /// Peek without consuming
        #[arg(long)]
        peek: bool,
        /// Clear all messages instead of reading
        #[arg(long)]
        clear: bool,
    },
    /// Read current screen text of a surface
    Screen {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Number of lines to read from the bottom (dips into scrollback if needed)
        #[arg(long)]
        lines: Option<usize>,
    },
    /// List recorded shell commands (OSC 133) for a surface
    Commands {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Limit number of returned records
        #[arg(long)]
        limit: Option<usize>,
        /// Only include records ended at or after this unix-ms timestamp
        #[arg(long)]
        since: Option<i64>,
    },
    /// Most recent recorded command for a surface
    #[command(name = "last-command")]
    LastCommand {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Recorded command at index (negative = from end)
    #[command(name = "command-at")]
    CommandAt {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// 0-based index; negatives count from the end
        #[arg(long, allow_hyphen_values = true)]
        index: i64,
    },
}

#[derive(Subcommand)]
pub enum UnsetCommands {
    /// Remove a hook
    Hook {
        /// Hook ID to remove
        #[arg(long)]
        hook: u64,
    },
    /// Remove a global hook by ID
    GlobalHook {
        /// Hook ID to remove
        #[arg(long)]
        hook: u32,
    },
}

#[derive(Subcommand)]
pub enum NewCommands {
    /// Create a new window
    Window,
    /// Create a new workspace
    Workspace {
        /// Name for the new workspace
        #[arg(long)]
        name: Option<String>,
        /// Working directory for the new workspace
        #[arg(long)]
        cwd: Option<String>,
        /// Surface type: terminal (default), markdown, explorer, html, image
        #[arg(long, default_value = "terminal")]
        r#type: String,
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
    /// Create a new tab in the specified pane
    Tab {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
        /// Surface type: terminal (default), markdown, explorer, html, image
        #[arg(long, default_value = "terminal")]
        r#type: String,
        /// Working directory (for terminal type)
        #[arg(long)]
        cwd: Option<String>,
        /// File path (for markdown type)
        #[arg(long)]
        file: Option<String>,
        /// Directory path (for explorer type)
        #[arg(long)]
        path: Option<String>,
        /// URL (for html type)
        #[arg(long)]
        url: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CloseCommands {
    /// Close a specific tab by its ID
    Tab {
        /// Target tab ID (required)
        #[arg(long)]
        tab: u32,
    },
    /// Close the specified pane (unsplit)
    Pane {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
    },
    /// Close the specified surface within a tab
    Surface {
        /// Target surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Close the calling surface itself (uses TASTY_SURFACE_ID)
    #[command(name = "self")]
    CloseSelf,
}

#[derive(Subcommand)]
pub enum SurfaceMetaCommands {
    /// Set a metadata key-value pair on a surface
    Set {
        /// Key name
        #[arg(long)]
        key: String,
        /// Value
        #[arg(long)]
        value: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Get a metadata value by key
    Get {
        /// Key name
        #[arg(long)]
        key: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Remove a metadata key
    Unset {
        /// Key name
        #[arg(long)]
        key: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// List all metadata for a surface
    List {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum DebugCommands {
    /// Show debug info from the running tasty instance
    Info,
    /// Enable IME composition mode
    ImeEnable,
    /// Disable IME composition mode and clear preedit
    ImeDisable,
    /// Send IME preedit (composition) text
    ImePreedit {
        /// Composition text (e.g. "ㅎ", "하", "한")
        #[arg()]
        text: String,
        /// Cursor position within composition
        #[arg(long)]
        cursor: Option<u64>,
    },
    /// Commit IME composition text (finalize and send to terminal)
    ImeCommit {
        /// Finalized text to commit (e.g. "한")
        #[arg()]
        text: String,
    },
    /// Show current IME status
    ImeStatus,
    /// Get cell info at a specific position
    CellInfo {
        /// Row (0-indexed)
        #[arg(long)]
        row: u64,
        /// Column (0-indexed)
        #[arg(long)]
        col: u64,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Get all cell attributes for a specific row
    ScreenAttrs {
        /// Row (0-indexed)
        #[arg(long)]
        row: u64,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Get the resolved RGBA bg/fg the renderer would push to the GPU for a cell
    GlyphColor {
        /// Row (0-indexed)
        #[arg(long)]
        row: u64,
        /// Column (0-indexed)
        #[arg(long)]
        col: u64,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Background mode: "focused" or "unfocused" (default: focused)
        #[arg(long, default_value = "focused")]
        bg_mode: String,
    },
    /// Switch macOS input source (e.g. Korean IME)
    SwitchInputSource {
        /// Input source ID (e.g. "com.apple.inputmethod.Korean.2SetKorean")
        #[arg()]
        source_id: String,
    },
    /// Send a raw physical key code via CGEvent (goes through IME pipeline)
    RawKey {
        /// macOS virtual key code (e.g. 7=KeyX, 35=KeyP, 49=Space)
        #[arg()]
        keycode: u16,
    },
    /// Event Bus inspection and injection (debug builds only)
    #[command(subcommand)]
    EventBus(EventBusCommands),
    /// Extension hook inspection and manual invocation (debug builds only)
    #[command(subcommand)]
    Extension(ExtensionDebugCommands),
    /// Tool menu inspection and invocation (debug builds only)
    #[command(subcommand)]
    Tool(ToolDebugCommands),
    /// Plugin popup inspection and open/close (debug builds only)
    #[command(subcommand)]
    Popup(PopupDebugCommands),
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum ToolDebugCommands {
    /// List all tool menu items in display order
    List,
    /// Invoke a tool menu item by key
    Invoke {
        /// Tool item key (e.g. "builtin:clipboard_history" or "<plugin_id>/<tool_id>")
        #[arg(long)]
        key: String,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum PopupDebugCommands {
    /// List all popup contributes + currently open instances
    List,
    /// Open a plugin popup instance
    Open {
        /// Plugin id (e.g. "com.example.popper")
        #[arg(long)]
        plugin_id: String,
        /// Popup id within the plugin
        #[arg(long)]
        popup_id: String,
        /// Optional context JSON to send as the popup.open payload
        #[arg(long)]
        context: Option<String>,
    },
    /// Close a popup instance by id
    Close {
        /// Popup instance id returned by `popup open`
        #[arg(long)]
        instance_id: u64,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum ExtensionDebugCommands {
    /// Fire an extension hook manually (sends extension.invoke_hook to the
    /// specified extension and returns the response).
    InvokeHook {
        /// Extension plugin id (must be installed and running).
        #[arg(long)]
        extension_id: String,
        /// Hook kind: "event" or "ipc".
        #[arg(long)]
        kind: String,
        /// Hook phase: "pre" or "post".
        #[arg(long)]
        phase: String,
        /// Hook mode: "transform", "filter", or "observe".
        #[arg(long)]
        mode: String,
        /// Target: event key (e.g. "foo.bar") or IPC method (e.g. "codex.spawn").
        #[arg(long)]
        target: String,
        /// JSON payload to pass as the hook input (default: `{}`).
        #[arg(long, default_value = "{}")]
        payload: String,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum EventBusCommands {
    /// List plugins subscribing to the given event key
    ListSubscribers {
        /// Event key (e.g. "surface.closed")
        #[arg()]
        key: String,
    },
    /// Publish an arbitrary event from the host side
    Publish {
        /// Event key
        #[arg()]
        key: String,
        /// JSON payload (default: `{}`)
        #[arg(long, default_value = "{}")]
        payload: String,
        /// Event scope: "system" (default) or "surface"
        #[arg(long, default_value = "system")]
        scope: String,
    },
    /// Print recent envelopes with the given trace_id
    Trace {
        /// trace_id (e.g. "h2a")
        #[arg()]
        trace_id: String,
    },
}

// ── Shared argument introspection ──

struct ArgInfo {
    name: String,
    flag: Option<String>,
    help: String,
    required: bool,
}

impl ArgInfo {
    /// Compact form: `<NAME>`, `--flag <NAME>`, `[--flag <NAME>]`
    fn compact(&self) -> String {
        match &self.flag {
            None => {
                if self.required {
                    format!("<{}>", self.name)
                } else {
                    format!("[{}]", self.name)
                }
            }
            Some(f) => {
                if self.required {
                    format!("{} <{}>", f, self.name)
                } else {
                    format!("[{} <{}>]", f, self.name)
                }
            }
        }
    }

    /// Detail form for error messages: `  --flag <NAME>   Help text`
    fn detail(&self) -> String {
        match &self.flag {
            None => format!("  <{}>          {}", self.name, self.help),
            Some(f) => {
                if self.required {
                    format!("  {} <{}>   {}", f, self.name, self.help)
                } else {
                    format!("  [{} <{}>] {}", f, self.name, self.help)
                }
            }
        }
    }
}

/// Extract visible arguments from a clap Command (filtering out help/version).
fn visible_args(cmd: &clap::Command) -> Vec<ArgInfo> {
    cmd.get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(|a| ArgInfo {
            name: a.get_id().to_string().to_uppercase(),
            flag: a
                .get_long()
                .map(|l| format!("--{}", l))
                .or_else(|| a.get_short().map(|s| format!("-{}", s))),
            help: a.get_help().map(|s| s.to_string()).unwrap_or_default(),
            required: a.is_required_set(),
        })
        .collect()
}

/// Extract visible subcommands (filtering out "help").
fn visible_subcommands(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands()
        .filter(|s| s.get_name() != "help")
        .collect()
}

/// Compact usage string: `<TEXT> [--surface <SURFACE>]`
fn format_args(cmd: &clap::Command) -> String {
    visible_args(cmd)
        .iter()
        .map(|a| a.compact())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve the deepest matched command from raw CLI args.
fn resolve_command_path() -> (clap::Command, String) {
    use clap::CommandFactory;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Cli::command();
    let mut current = root.clone();
    let mut matched_path: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') {
            break;
        }
        let found = current
            .get_subcommands()
            .find(|s| s.get_name() == arg.as_str());
        if let Some(sub) = found {
            matched_path.push(arg.clone());
            current = sub.clone();
        } else {
            break;
        }
    }

    let path = if matched_path.is_empty() {
        "tasty".to_string()
    } else {
        format!("tasty {}", matched_path.join(" "))
    };
    (current, path)
}

// ── Public entry points ──

/// Print all commands in a tree structure (2 levels deep) with usage details.
pub fn print_command_tree() {
    use clap::CommandFactory;

    let cmd = Cli::command();
    println!("{} {}", cmd.get_name(), cmd.get_version().unwrap_or(""));
    println!(
        "{}",
        cmd.get_about().map(|s| s.to_string()).unwrap_or_default()
    );
    println!();

    // `_connector`는 leaf print_node에서는 쓰지 않지만, 시그니처를 재귀 호출 측과
    // 동일하게 유지하기 위해 받기만 한다 (caller가 자식 노드 prefix 조립에 사용).
    fn print_node(cmd: &clap::Command, prefix: &str, _connector: &str) {
        let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
        let args = format_args(cmd);
        if args.is_empty() {
            println!("{}{} — {}", prefix, cmd.get_name(), about);
        } else {
            println!("{}{} {} — {}", prefix, cmd.get_name(), args, about);
        }
    }

    let subs: Vec<_> = visible_subcommands(&cmd);
    let count = subs.len();
    for (i, sub) in subs.iter().enumerate() {
        let is_last = i == count - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        let connector = if is_last { "    " } else { "│   " };

        let children = visible_subcommands(sub);
        if children.is_empty() {
            print_node(sub, prefix, connector);
        } else {
            let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
            println!("{}{} — {}", prefix, sub.get_name(), about);
            let child_count = children.len();
            for (j, child) in children.iter().enumerate() {
                let child_is_last = j == child_count - 1;
                let child_prefix = if child_is_last {
                    "└── "
                } else {
                    "├── "
                };
                print_node(child, &format!("{}{}", connector, child_prefix), connector);
            }
        }
    }
}

/// Format a contextual error message for a failed parse.
pub fn format_parse_error(err: clap::Error) {
    use clap::error::ErrorKind;

    match err.kind() {
        ErrorKind::MissingRequiredArgument
        | ErrorKind::InvalidValue
        | ErrorKind::UnknownArgument
        | ErrorKind::InvalidSubcommand => {
            let (current, cmd_path) = resolve_command_path();
            let children = visible_subcommands(&current);

            eprintln!("{}", err);

            if !children.is_empty() {
                eprintln!("Available subcommands for '{}':", cmd_path);
                for sub in &children {
                    let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
                    let args = format_args(sub);
                    if args.is_empty() {
                        eprintln!("  {} {:16} {}", cmd_path, sub.get_name(), about);
                    } else {
                        eprintln!("  {} {} {} — {}", cmd_path, sub.get_name(), args, about);
                    }
                }
            } else {
                let args = visible_args(&current);
                let required: Vec<_> = args.iter().filter(|a| a.required).collect();
                let optional: Vec<_> = args.iter().filter(|a| !a.required).collect();

                if !required.is_empty() {
                    eprintln!("Required arguments for '{}':", cmd_path);
                    for arg in &required {
                        eprintln!("{}", arg.detail());
                    }
                }
                if !optional.is_empty() {
                    eprintln!("Optional:");
                    for arg in &optional {
                        eprintln!("{}", arg.detail());
                    }
                }
            }
            eprintln!();
            eprintln!("Run '{} --help' for full details.", cmd_path);
        }
        _ => {
            err.exit();
        }
    }
    std::process::exit(2);
}

/// plugin contributes.cli가 합쳐진 도움말 출력. plugin 디스커버리에 실패해도
/// 정적 CLI 도움말은 항상 보장한다.
pub fn print_augmented_help() -> Result<()> {
    use clap::CommandFactory;
    let entries = match crate::plugin::plugin_root() {
        Some(root) => dynamic::discover_plugin_clis(&root),
        None => Vec::new(),
    };
    let mut cmd = if entries.is_empty() {
        Cli::command()
    } else {
        dynamic::build_augmented_cli(&entries)
    };
    cmd.print_help()?;
    println!();
    Ok(())
}

/// 정적 `Cli` 파싱이 `InvalidSubcommand`로 실패했을 때 마지막 시도로 plugin
/// CLI에서 매칭한다. 매칭되면 IPC로 전송, 매칭 실패면 None을 반환해 호출자가
/// 원래 에러를 출력하도록 한다.
///
/// `tasty <plugin>` 단독, `tasty <plugin> --help`, `tasty <plugin> --version` 처럼
/// 사용자가 plugin command를 입력했지만 augmented 파싱이 도움말/버전/서브커맨드
/// 누락 등으로 실패한 경우는 clap의 표준 출력을 그대로 보여준다.
pub fn try_run_plugin_cli() -> Option<Result<()>> {
    let plugins_root = match crate::plugin::plugin_root() {
        Some(p) => p,
        None => return None,
    };
    let entries = dynamic::discover_plugin_clis(&plugins_root);
    if entries.is_empty() {
        return None;
    }
    // 사용자가 입력한 첫 인자가 plugin command 이름인지 확인. plugin 명령이 맞다면
    // clap 에러도 자체 출력으로 처리한다 (정적 CLI의 "unrecognized subcommand"가
    // 대신 뜨면 안 됨).
    let first_arg = std::env::args().nth(1);
    let is_plugin_cmd = first_arg
        .as_deref()
        .map(|name| entries.iter().any(|e| e.cli.name == name))
        .unwrap_or(false);
    let augmented = dynamic::build_augmented_cli(&entries);
    let matches = match augmented.try_get_matches() {
        Ok(m) => m,
        Err(err) => {
            if is_plugin_cmd {
                err.exit();
            }
            return None;
        }
    };
    let (top_name, _) = matches.subcommand()?;
    if !entries.iter().any(|e| e.cli.name == top_name) {
        return None;
    }
    let request = match dynamic::matches_to_request(&entries, &matches) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };
    Some(run_dynamic_client(request))
}

fn run_dynamic_client(request: crate::ipc::protocol::JsonRpcRequest) -> Result<()> {
    let port = IpcServer::read_port_file()?;
    let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;
    let mut conn = IpcConnection::new(stream)?;
    match conn.send(&request) {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            if let Some(rest) = msg.strip_prefix("Error (") {
                eprintln!("Error ({}", rest);
            } else {
                eprintln!("{}", msg);
            }
            std::process::exit(1);
        }
    }
}

/// Run the CLI client: connect to a running tasty instance and execute the command.
pub fn run_client(command: Commands) -> Result<()> {
    // plugin logs is local-only — read the log file directly.
    if let Commands::Plugin {
        command: PluginCommands::Logs { id, follow },
    } = &command
    {
        return run_plugin_logs(id, *follow);
    }

    let port = IpcServer::read_port_file()?;
    let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;

    let mut conn = IpcConnection::new(stream)?;

    let request = command_to_request(&command);
    let result = conn.send(&request);

    match result {
        Ok(value) => format_output(&command, &value),
        Err(e) => {
            let msg = e.to_string();
            if let Some(rest) = msg.strip_prefix("Error (") {
                eprintln!("Error ({}", rest);
            } else {
                eprintln!("{}", msg);
            }
            std::process::exit(1);
        }
    }

    Ok(())
}
