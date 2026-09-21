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
    /// List the surface kinds this instance has actually registered.
    ///
    /// This is the runtime fact, not a manifest declaration: a kind shows up here
    /// only if the host registered it, so `--type <kind>` works for exactly the
    /// kinds listed. Host builtins and plugin-provided kinds both appear.
    SurfaceKinds,
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
    /// Show request pressure since this instance started: where the time went
    /// while answering requests.
    ///
    /// The answer is one block per population, and the populations differ on
    /// purpose. `queue_before_gate` is measured before the permission gate, so
    /// it also counts requests that were later rejected. `handler_after_gate`
    /// is measured after it, so it counts only requests that ran.
    /// `plugin_round_trip` is how long this instance waited for a plugin to
    /// answer, counting only the requests that were answered at all. `db` is
    /// how long writes took to settle on disk, counting only commits that
    /// succeeded. Do not subtract one block from another: a request can pass
    /// the gate and still be answered before the handler block sees it.
    ///
    /// `connections` is the fifth block and it is not time, it is seats: every
    /// TCP connection attached to this port, including attach and mesh streams
    /// that never send a request. `live` is the only value here that goes
    /// down, `limit` is the ceiling the server enforces and travels with the
    /// values so the two can be read together, and `refused_saturated` counts
    /// connections turned away at that ceiling — before they became requests,
    /// so they appear in none of the four blocks above.
    ///
    /// Three of those four — `queue_before_gate`, `handler_after_gate` and
    /// `plugin_round_trip` — also carry a `*_hist` with the distribution of
    /// the same observations, because an average and a maximum cannot tell
    /// "everything is slightly slow" from "most are fast and a few are not".
    /// `bounds_us` and `counts` travel together; `counts` is one entry longer
    /// and is not cumulative, so the entries sum to the observation count and
    /// the last one means only that the top bound was passed. Quantiles are
    /// not computed here. `db` is a duration too but has no distribution, and
    /// `connections` has none because it is not a duration at all.
    ///
    /// `db_pragmas` is the sixth block and it is not a running total at all:
    /// for each SQLite database (`memory_db`, `state_db`) it shows the
    /// connection settings that were requested when the database was opened
    /// next to the values read back from it, because a request such as
    /// journal_mode=WAL can be silently refused. `degraded` is true when any
    /// setting did not take; the database is still in use. `state_db` is null
    /// when that database is not open in this process, which is always the
    /// case for a headless instance.
    ///
    /// An average with nothing behind it comes back as null, not zero.
    Pressure,
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
