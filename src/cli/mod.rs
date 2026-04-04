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
}

#[derive(Subcommand)]
pub enum Commands {
    /// List workspaces
    List,
    /// Create a new window
    NewWindow,
    /// List all windows
    Windows,
    /// Create a new workspace
    NewWorkspace {
        /// Name for the new workspace
        #[arg(long)]
        name: Option<String>,
        /// Working directory for the new workspace
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Select a workspace by index (0-based)
    SelectWorkspace {
        /// Workspace index
        index: usize,
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
    /// List notifications
    Notifications,
    /// Show tree view of workspaces, panes, and tabs
    Tree,
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
    /// Create a new tab in the focused pane
    NewTab {
        /// Working directory for the new tab
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Close the active tab in the focused pane
    CloseTab,
    /// Close the focused pane (unsplit)
    ClosePane,
    /// Close the focused surface within a SurfaceGroup
    CloseSurface,
    /// List surfaces (terminals) in the active workspace
    Surfaces,
    /// List panes in the active workspace
    Panes,
    /// Show system info
    Info,
    /// Set a hook on a surface
    SetHook {
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
    /// List hooks
    ListHooks {
        /// Filter by surface ID
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Remove a hook
    UnsetHook {
        /// Hook ID to remove
        #[arg(long)]
        hook: u64,
    },
    /// Set a read mark on a surface
    SetMark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
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
    /// Launch Claude Code in a new workspace
    Claude {
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
    ClaudeSpawn {
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
    ClaudeChildren,
    /// Show the parent of the focused (or specified) Claude child
    ClaudeParent,
    /// Kill a child Claude instance by surface ID
    ClaudeKill {
        /// Child surface ID to kill
        #[arg(long)]
        child: u32,
    },
    /// Respawn a child Claude instance
    ClaudeRespawn {
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
    /// Move focus in a spatial direction (left, right, up, down)
    FocusDirection {
        /// Direction to move focus: left, right, up, down
        direction: String,
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
    /// Claude Code hook integration (called by Claude Code's hook system)
    ClaudeHook {
        /// Hook event type: stop, notification, prompt-submit, session-start
        #[arg()]
        event: String,
        /// Surface ID (auto-detected from TASTY_SURFACE_ID env var if not provided)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Set a global hook (timer or file-watching)
    GlobalHookSet {
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
    /// List all global hooks
    GlobalHookList,
    /// Remove a global hook by ID
    GlobalHookUnset {
        /// Hook ID to remove
        #[arg(long)]
        hook: u32,
    },
    /// Check if a surface is currently typing (received key input within 5 seconds)
    IsTyping {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Open a Markdown file viewer tab
    OpenMarkdown {
        /// Path to the markdown file
        #[arg()]
        path: String,
    },
    /// Open a file explorer tab
    OpenExplorer {
        /// Root directory path (default: home directory)
        #[arg(long)]
        path: Option<String>,
    },
    /// Broadcast text to all children of a parent Claude instance
    ClaudeBroadcast {
        /// Text to send to all children
        #[arg()]
        text: String,
        /// Filter children by role
        #[arg(long)]
        role: Option<String>,
    },
    /// Wait for a specific child Claude instance to become idle/needs_input/exited
    ClaudeWait {
        /// Child surface ID to wait for
        #[arg(long)]
        child: u32,
        /// Timeout in seconds (default: 30)
        #[arg(long, default_value = "30")]
        timeout: u64,
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
    if let Commands::ClaudeHook { ref event, ref surface } = command {
        run_claude_hook(&mut stream, event, *surface)?;
        return Ok(());
    }

    // ClaudeWait is special: it polls until the child reaches a terminal state
    if let Commands::ClaudeWait { child, timeout } = command {
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
