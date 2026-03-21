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
    /// List surfaces (terminals) in the active workspace
    Surfaces,
    /// List panes in the active workspace
    Panes,
    /// Show system info
    Info,
}

/// Run the CLI client: connect to a running tasty instance and execute the command.
pub fn run_client(command: Commands) -> Result<()> {
    let port = IpcServer::read_port_file()?;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .map_err(|e| anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port, e
        ))?;

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
        Commands::Surfaces => ("surface.list", serde_json::json!({})),
        Commands::Panes => ("pane.list", serde_json::json!({})),
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
