mod claude;
mod format;
mod request;
mod transport;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::IpcServer;

use claude::{run_claude_hook, run_claude_wait};
use format::format_output;
use request::command_to_request;

#[derive(Parser)]
#[command(name = "tasty", about = "GPU-accelerated terminal emulator for AI coding agents", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Custom port file path (for test isolation)
    #[arg(long)]
    pub port_file: Option<String>,

    /// Force GUI launch even inside a tasty terminal
    #[arg(long)]
    pub launch: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new resource (window, workspace, tab, split, markdown, explorer)
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
    /// Claude Code integration (launch, spawn, kill, wait, etc.)
    Claude {
        #[command(subcommand)]
        command: ClaudeCommands,
    },
    /// Send text to the focused terminal
    Send {
        /// Text to send
        text: String,
    },
    /// Send a key to the focused terminal (enter, tab, escape, up, down, etc.)
    SendKey {
        /// Key name
        key: String,
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
    /// Read output since last mark
    ReadSinceMark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Strip ANSI escape sequences from output
        #[arg(long)]
        strip_ansi: bool,
    },
    /// Send a message to a surface's message queue
    MessageSend {
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
    /// Read messages from a surface's message queue (consumes by default)
    MessageRead {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Filter by sender surface ID
        #[arg(long)]
        from: Option<u32>,
        /// Peek without consuming
        #[arg(long)]
        peek: bool,
    },
    /// Count messages in a surface's message queue
    MessageCount {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Clear all messages in a surface's message queue
    MessageClear {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Manage per-surface metadata (set, get, unset, list)
    SurfaceMeta {
        /// Action: set, get, unset, list
        #[arg()]
        action: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Key name
        #[arg(long)]
        key: Option<String>,
        /// Value (for set action)
        #[arg(long)]
        value: Option<String>,
    },
    /// Check if a surface is currently typing (received key input within 5 seconds)
    IsTyping {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Debug and diagnostic commands (IME simulation, raw key input, etc.)
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
}

#[derive(Subcommand)]
pub enum ListCommands {
    /// List workspaces
    Workspaces,
    /// List all windows
    Windows,
    /// Show tree view of workspaces, panes, and tabs
    Tree,
    /// List surfaces (terminals) in the active workspace
    Surfaces,
    /// List panes in the active workspace
    Panes,
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
        /// Workspace ID (default: active workspace)
        #[arg(long)]
        id: Option<u32>,
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
    /// Set focus target (workspace, direction)
    Focus {
        #[command(subcommand)]
        command: SetFocusCommands,
    },
}

#[derive(Subcommand)]
pub enum SetFocusCommands {
    /// Select a workspace by index (0-based)
    Workspace {
        /// Workspace index
        index: usize,
    },
    /// Move focus in a spatial direction (left, right, up, down)
    Direction {
        /// Direction: left, right, up, down
        direction: String,
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
    },
    /// Create a new tab in the focused pane
    Tab {
        /// Working directory for the new tab
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Split a pane group or surface
    Split {
        /// Split level: pane-group (upper layout) or surface (lower layout)
        #[arg(long)]
        level: String,
        /// Target: numeric ID, "this" (current surface), or nickname. Omit for focused target
        #[arg(long)]
        target: Option<String>,
        /// Split direction: vertical (default) or horizontal
        #[arg(long, default_value = "vertical")]
        direction: String,
        /// Metadata JSON to set on the new surface (e.g. '{"nickname":"build"}')
        #[arg(long)]
        meta: Option<String>,
        /// Working directory for the new surface
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Open a Markdown file viewer tab
    Markdown {
        /// Path to the markdown file
        #[arg()]
        path: String,
    },
    /// Open a file explorer tab
    Explorer {
        /// Root directory path (default: home directory)
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CloseCommands {
    /// Close the active tab in the focused pane
    Tab,
    /// Close the focused pane (unsplit)
    Pane,
    /// Close the focused surface within a SurfaceGroup
    Surface,
}

#[derive(Subcommand)]
pub enum ClaudeCommands {
    /// Launch Claude Code in a new workspace
    Launch {
        /// Workspace name (default: "claude")
        #[arg(long)]
        workspace: Option<String>,
        /// Working directory
        #[arg(long)]
        directory: Option<String>,
        /// Task description to pass to claude
        #[arg(long)]
        task: Option<String>,
    },
    /// Spawn a child Claude instance in a new pane
    Spawn {
        /// Split direction: vertical (default) or horizontal
        #[arg(long)]
        direction: Option<String>,
        /// Working directory for the child
        #[arg(long)]
        cwd: Option<String>,
        /// Role label for the child
        #[arg(long)]
        role: Option<String>,
        /// Nickname for the child
        #[arg(long)]
        nickname: Option<String>,
        /// Initial prompt to send to claude
        #[arg(long)]
        prompt: Option<String>,
    },
    /// List children of the focused (or specified) Claude parent
    Children,
    /// Show the parent of the focused (or specified) Claude child
    Parent,
    /// Kill a child Claude instance by surface ID
    Kill {
        /// Child surface ID to kill
        #[arg(long)]
        child: u32,
    },
    /// Respawn a child Claude instance
    Respawn {
        /// Child surface ID to respawn
        #[arg(long)]
        child: u32,
        /// Working directory for the new child
        #[arg(long)]
        cwd: Option<String>,
        /// Role label for the new child
        #[arg(long)]
        role: Option<String>,
        /// Nickname for the new child
        #[arg(long)]
        nickname: Option<String>,
        /// Initial prompt to send to claude
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Broadcast text to all children of a parent Claude instance
    Broadcast {
        /// Text to send to all children
        #[arg()]
        text: String,
        /// Filter children by role
        #[arg(long)]
        role: Option<String>,
    },
    /// Wait for a specific child Claude instance to become idle/needs_input/exited
    Wait {
        /// Child surface ID to wait for
        #[arg(long)]
        child: u32,
        /// Timeout in seconds (default: 30)
        #[arg(long, default_value = "30")]
        timeout: u64,
    },
    /// Claude Code hook integration (called by Claude Code's hook system)
    Hook {
        /// Hook event type: stop, notification, prompt-submit, session-start
        #[arg()]
        event: String,
        /// Surface ID (auto-detected from TASTY_SURFACE_ID env var if not provided)
        #[arg(long)]
        surface: Option<u32>,
    },
}

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
}

/// Run the CLI client: connect to a running tasty instance and execute the command.
pub fn run_client(command: Commands) -> Result<()> {
    let port = IpcServer::read_port_file()?;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .map_err(|e| anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port, e
        ))?;

    // ClaudeHook is special: it may send multiple requests
    if let Commands::Claude { command: ClaudeCommands::Hook { ref event, ref surface } } = command {
        run_claude_hook(&mut stream, event, *surface)?;
        return Ok(());
    }

    // ClaudeWait is special: it polls until the child reaches a terminal state
    if let Commands::Claude { command: ClaudeCommands::Wait { child, timeout } } = command {
        run_claude_wait(&mut stream, child, timeout)?;
        return Ok(());
    }

    let request = command_to_request(&command);
    let json = serde_json::to_string(&request)?;
    writeln!(stream, "{}", json)?;
    stream.flush()?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response: JsonRpcResponse = serde_json::from_str(trimmed)?;

        if let Some(error) = response.error {
            eprintln!("Error ({}): {}", error.code, error.message);
            std::process::exit(1);
        }

        if let Some(result) = response.result {
            format_output(&command, &result);
        }
        break;
    }

    Ok(())
}
