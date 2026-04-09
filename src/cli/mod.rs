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

    /// Show all commands in a tree (use with -h)
    #[arg(short = 'a', long = "all")]
    pub all: bool,
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
    Mark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Strip ANSI escape sequences from output
        #[arg(long)]
        strip_ansi: bool,
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
    /// Create a new tab in the specified pane
    Tab {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
        /// Working directory for the new tab
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Split a pane group or surface
    Split {
        /// Split level: pane-group (upper layout) or surface (lower layout)
        #[arg(long)]
        level: String,
        /// Target: numeric ID, "this" (current surface), or nickname (required)
        #[arg(long)]
        target: String,
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
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
    },
    /// Open a file explorer tab
    Explorer {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
        /// Root directory path (default: home directory)
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CloseCommands {
    /// Close the active tab in the specified pane
    Tab {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
    },
    /// Close the specified pane (unsplit)
    Pane {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
    },
    /// Close the specified surface within a SurfaceGroup
    Surface {
        /// Target surface ID (required)
        #[arg(long)]
        surface: u32,
    },
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
        /// Parent surface ID (default: TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
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
    /// List children of the specified Claude parent
    Children {
        /// Parent surface ID (default: TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Show the parent of the specified Claude child
    Parent {
        /// Child surface ID (default: TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
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
        /// Parent surface ID (default: TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
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
                if self.required { format!("<{}>", self.name) }
                else { format!("[{}]", self.name) }
            }
            Some(f) => {
                if self.required { format!("{} <{}>", f, self.name) }
                else { format!("[{} <{}>]", f, self.name) }
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
            flag: a.get_long().map(|l| format!("--{}", l))
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
    visible_args(cmd).iter().map(|a| a.compact()).collect::<Vec<_>>().join(" ")
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
        let found = current.get_subcommands()
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
    println!("{}", cmd.get_about().map(|s| s.to_string()).unwrap_or_default());
    println!();

    fn print_node(cmd: &clap::Command, prefix: &str, connector: &str) {
        let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
        let args = format_args(cmd);
        if args.is_empty() {
            println!("{}{} — {}", prefix, cmd.get_name(), about);
        } else {
            println!("{}{} {} — {}", prefix, cmd.get_name(), args, about);
        }
        let _ = connector; // used by caller for children
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
                let child_prefix = if child_is_last { "└── " } else { "├── " };
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
