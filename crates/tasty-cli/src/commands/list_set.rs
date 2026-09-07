//! `tasty list` / `tasty set` subcommand 정의.

use clap::Subcommand;

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
    /// Show GPU resource counts (wgpu report + per-window renderer stats)
    GpuStats,
    /// List notifications
    Notifications,
    /// List registered timers and what is currently waking this instance
    Timers,
    /// List hooks
    Hooks {
        /// Filter by surface ID
        #[arg(long)]
        surface: Option<u32>,
    },
    /// List all global hooks
    GlobalHooks,
    /// Show the resolved global theme snapshot (colors, font sizes, ui scale).
    Theme,
    /// List the recently opened files of one surface kind.
    Recent {
        /// Surface kind to query (e.g. `markdown`). The host does not know the
        /// kind names — the caller supplies one.
        #[arg(long)]
        kind: String,
    },
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
        /// Inline shell command to execute when the event fires (backward-compat;
        /// wrapped as an anonymous hook handler). Mutually exclusive with --handler.
        #[arg(long)]
        command: Option<String>,
        /// Shared hook-handler id to bind (e.g. host/my-handler). The handler must
        /// accept the hook trigger source. Mutually exclusive with --command.
        #[arg(long)]
        handler: Option<String>,
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
    /// Update workspace name, subtitle, description, or SSH attach mapping
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
        /// Map this workspace to a saved SSH profile (a remote machine).
        #[arg(long)]
        ssh_profile: Option<String>,
        /// Map to a one-off inline SSH target, e.g. --ssh user@host.
        #[arg(long)]
        ssh: Option<String>,
        /// Workspace id on the mapped remote tasty to attach to.
        #[arg(long)]
        remote_workspace: Option<u32>,
        /// Clear the existing SSH attach mapping.
        #[arg(long)]
        clear_mapping: bool,
        /// Move this workspace to another category (name or id).
        #[arg(long)]
        category: Option<String>,
    },
    /// Set a global hook (timer-based)
    /// Set the working directory a remote surface reports to the host.
    Cwd {
        /// Target surface id.
        #[arg(long)]
        surface: u32,
        /// Directory path. Omit to clear it.
        #[arg(long)]
        path: Option<String>,
    },
    /// Set the URL of a webview-kind surface.
    Url {
        /// Target surface id.
        #[arg(long)]
        surface: u32,
        /// URL to load.
        #[arg(long)]
        url: String,
    },
    /// Set a global hook — fires on a schedule, not bound to any surface
    GlobalHook {
        /// Condition: interval:SECS, once:SECS
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
