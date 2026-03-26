use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::ipc::server::IpcServer;

#[derive(Parser)]
#[command(name = "tasty", about = "GPU-accelerated terminal emulator for AI coding agents")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Run in headless mode (no GUI, IPC only)
    #[arg(long)]
    pub headless: bool,

    /// Custom port file path (for test isolation)
    #[arg(long)]
    pub port_file: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List workspaces
    List,
    /// Create a new workspace
    NewWorkspace {
        /// Name for the new workspace
        #[arg(long)]
        name: Option<String>,
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
    /// Split the focused pane
    Split {
        /// Split direction: vertical (default) or horizontal
        #[arg(long, default_value = "vertical")]
        direction: String,
    },
    /// Create a new tab in the focused pane
    NewTab,
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

/// Send a single JSON-RPC request and read the response.
fn send_request(stream: &mut TcpStream, request: &JsonRpcRequest) -> Result<serde_json::Value> {
    let json = serde_json::to_string(request)?;
    writeln!(stream, "{}", json)?;
    stream.flush()?;

    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response: JsonRpcResponse = serde_json::from_str(trimmed)?;

        if let Some(error) = response.error {
            anyhow::bail!("Error ({}): {}", error.code, error.message);
        }

        return Ok(response.result.unwrap_or(serde_json::Value::Null));
    }
}

/// Build a JSON-RPC request from method and params.
fn make_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
    }
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

/// Handle the claude-hook subcommand, which maps Claude Code hook events to IPC calls.
fn run_claude_hook(stream: &mut TcpStream, event: &str, surface_arg: Option<u32>) -> Result<()> {
    // Resolve surface_id: --surface arg > TASTY_SURFACE_ID env var > null (server uses focused)
    let surface_id = surface_arg.or_else(|| {
        std::env::var("TASTY_SURFACE_ID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
    });

    let surface_param = match surface_id {
        Some(sid) => serde_json::json!(sid),
        None => serde_json::Value::Null,
    };

    match event {
        "stop" => {
            // Claude stopped → set idle, then fire claude-idle hook
            let req1 = make_request(
                "claude.set_idle_state",
                serde_json::json!({ "surface_id": surface_param, "idle": true }),
            );
            send_request(stream, &req1)?;

            let req2 = make_request(
                "surface.fire_hook",
                serde_json::json!({ "surface_id": surface_param, "event": "claude-idle" }),
            );
            let result = send_request(stream, &req2)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "notification" => {
            // Claude needs input → set needs_input, then fire needs-input hook
            let req1 = make_request(
                "claude.set_needs_input",
                serde_json::json!({ "surface_id": surface_param, "needs_input": true }),
            );
            send_request(stream, &req1)?;

            let req2 = make_request(
                "surface.fire_hook",
                serde_json::json!({ "surface_id": surface_param, "event": "needs-input" }),
            );
            let result = send_request(stream, &req2)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "prompt-submit" | "session-start" | "active" => {
            // Claude became active → clear idle/needs_input
            let req = make_request(
                "claude.set_idle_state",
                serde_json::json!({ "surface_id": surface_param, "idle": false }),
            );
            let result = send_request(stream, &req)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            eprintln!(
                "Unknown claude-hook event: '{}'. Use: stop, notification, prompt-submit, session-start",
                event
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Handle the claude-wait subcommand: poll until child is idle/needs_input/exited or timeout.
fn run_claude_wait(stream: &mut TcpStream, child: u32, timeout: u64) -> Result<()> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(timeout);

    loop {
        let req = make_request(
            "claude.wait",
            serde_json::json!({ "child_surface_id": child }),
        );
        let result = send_request(stream, &req)?;

        let state = result
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("active")
            .to_string();

        match state.as_str() {
            "idle" | "needs_input" | "exited" => {
                println!("{}", serde_json::to_string_pretty(&result)?);
                return Ok(());
            }
            _ => {
                if Instant::now() >= deadline {
                    eprintln!("Timeout: child {} did not reach a terminal state within {}s", child, timeout);
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn command_to_request(command: &Commands) -> JsonRpcRequest {
    let (method, params) = match command {
        Commands::Info => ("system.info", serde_json::json!({})),
        Commands::List => ("workspace.list", serde_json::json!({})),
        Commands::NewWorkspace { name } => (
            "workspace.create",
            serde_json::json!({ "name": name.as_deref().unwrap_or("") }),
        ),
        Commands::SelectWorkspace { index } => (
            "workspace.select",
            serde_json::json!({ "index": index }),
        ),
        Commands::Send { text } => ("surface.send", serde_json::json!({ "text": text })),
        Commands::SendKey { key } => ("surface.send_key", serde_json::json!({ "key": key })),
        Commands::Notify { body, title } => (
            "notification.create",
            serde_json::json!({ "title": title, "body": body }),
        ),
        Commands::Notifications => ("notification.list", serde_json::json!({})),
        Commands::Tree => ("tree", serde_json::json!({})),
        Commands::Split { direction } => (
            "pane.split",
            serde_json::json!({ "direction": direction }),
        ),
        Commands::NewTab => ("tab.create", serde_json::json!({})),
        Commands::CloseTab => ("tab.close", serde_json::json!({})),
        Commands::ClosePane => ("pane.close", serde_json::json!({})),
        Commands::CloseSurface => ("surface.close", serde_json::json!({})),
        Commands::Surfaces => ("surface.list", serde_json::json!({})),
        Commands::Panes => ("pane.list", serde_json::json!({})),
        Commands::SetHook {
            surface,
            event,
            command,
            once,
        } => (
            "hook.set",
            serde_json::json!({
                "surface_id": surface,
                "event": event,
                "command": command,
                "once": once,
            }),
        ),
        Commands::ListHooks { surface } => (
            "hook.list",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::UnsetHook { hook } => (
            "hook.unset",
            serde_json::json!({ "hook_id": hook }),
        ),
        Commands::SetMark { surface } => (
            "surface.set_mark",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::ReadSinceMark {
            surface,
            strip_ansi,
        } => (
            "surface.read_since_mark",
            serde_json::json!({
                "surface_id": surface,
                "strip_ansi": strip_ansi,
            }),
        ),
        Commands::Claude {
            workspace,
            directory,
            task,
        } => (
            "claude.launch",
            serde_json::json!({
                "workspace": workspace,
                "directory": directory,
                "task": task,
            }),
        ),
        Commands::ClaudeSpawn {
            direction,
            cwd,
            role,
            nickname,
            prompt,
        } => (
            "claude.spawn",
            serde_json::json!({
                "direction": direction,
                "cwd": cwd,
                "role": role,
                "nickname": nickname,
                "prompt": prompt,
            }),
        ),
        Commands::ClaudeChildren => ("claude.children", serde_json::json!({})),
        Commands::ClaudeParent => ("claude.parent", serde_json::json!({})),
        Commands::ClaudeKill { child } => (
            "claude.kill",
            serde_json::json!({ "child_surface_id": child }),
        ),
        Commands::ClaudeRespawn {
            child,
            cwd,
            role,
            nickname,
            prompt,
        } => (
            "claude.respawn",
            serde_json::json!({
                "child_surface_id": child,
                "cwd": cwd,
                "role": role,
                "nickname": nickname,
                "prompt": prompt,
            }),
        ),
        Commands::MessageSend { to, content, from } => (
            "message.send",
            serde_json::json!({
                "to_surface_id": to,
                "content": content,
                "from_surface_id": from,
            }),
        ),
        Commands::MessageRead { surface, from, peek } => (
            "message.read",
            serde_json::json!({
                "surface_id": surface,
                "from_surface_id": from,
                "peek": peek,
            }),
        ),
        Commands::MessageCount { surface } => (
            "message.count",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::MessageClear { surface } => (
            "message.clear",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::FocusDirection { direction } => (
            "focus.direction",
            serde_json::json!({ "direction": direction }),
        ),
        Commands::SurfaceMeta { action, surface, key, value } => {
            let method = match action.as_str() {
                "set" => "surface.meta_set",
                "get" => "surface.meta_get",
                "unset" => "surface.meta_unset",
                "list" => "surface.meta_list",
                _ => "surface.meta_list",
            };
            (
                method,
                serde_json::json!({
                    "surface_id": surface,
                    "key": key,
                    "value": value,
                }),
            )
        }
        Commands::ClaudeBroadcast { text, role } => (
            "claude.broadcast",
            serde_json::json!({
                "text": text,
                "role": role,
            }),
        ),
        // ClaudeHook is handled separately in run_client before reaching here
        Commands::ClaudeHook { .. } => unreachable!("ClaudeHook is handled in run_client"),
        // ClaudeWait is handled separately in run_client before reaching here
        Commands::ClaudeWait { .. } => unreachable!("ClaudeWait is handled in run_client"),
        Commands::GlobalHookSet {
            condition,
            command,
            label,
        } => (
            "global_hook.set",
            serde_json::json!({
                "condition": condition,
                "command": command,
                "label": label,
            }),
        ),
        Commands::GlobalHookList => ("global_hook.list", serde_json::json!({})),
        Commands::GlobalHookUnset { hook } => (
            "global_hook.unset",
            serde_json::json!({ "hook_id": hook }),
        ),
        Commands::IsTyping { surface } => (
            "surface.is_typing",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::OpenMarkdown { path } => (
            "tab.open_markdown",
            serde_json::json!({ "file_path": path }),
        ),
        Commands::OpenExplorer { path } => (
            "tab.open_explorer",
            serde_json::json!({ "path": path }),
        ),
    };

    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
    }
}

fn format_output(command: &Commands, result: &serde_json::Value) {
    match command {
        Commands::Tree => format_tree(result),
        Commands::List => format_workspace_list(result),
        Commands::Panes => format_pane_list(result),
        Commands::Notifications => format_notification_list(result),
        _ => {
            // Pretty print JSON
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }
}

fn format_tree(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if active { " *" } else { "" };
            println!("Workspace: {}{}", name, marker);

            if let Some(panes) = ws.get("panes").and_then(|v| v.as_array()) {
                for pane in panes {
                    let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let focused = pane.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
                    let pfx = if focused { ">" } else { " " };
                    println!("  {} Pane {}", pfx, pid);

                    if let Some(tabs) = pane.get("tabs").and_then(|v| v.as_array()) {
                        for tab in tabs {
                            let tname = tab.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let tactive = tab.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                            let tpfx = if tactive { "*" } else { " " };
                            println!("      {} {}", tpfx, tname);
                        }
                    }
                }
            }
        }
    }
}

fn format_workspace_list(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let pane_count = ws.get("pane_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let marker = if active { " *" } else { "" };
            println!("{}{} ({} panes)", name, marker, pane_count);
        }
    }
}

fn format_pane_list(result: &serde_json::Value) {
    if let Some(panes) = result.as_array() {
        for pane in panes {
            let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let focused = pane.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
            let tab_count = pane.get("tab_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let marker = if focused { " *" } else { "" };
            println!("Pane {}{} ({} tabs)", pid, marker, tab_count);
        }
    }
}

fn format_notification_list(result: &serde_json::Value) {
    if let Some(notifs) = result.as_array() {
        if notifs.is_empty() {
            println!("No notifications");
            return;
        }
        for n in notifs {
            let title = n.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let body = n.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let read = n.get("read").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if read { " " } else { "*" };
            if body.is_empty() {
                println!("{} {}", marker, title);
            } else {
                println!("{} {}: {}", marker, title, body);
            }
        }
    }
}
