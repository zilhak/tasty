pub mod dynamic;
mod format;
mod plugin;
mod request;
pub(crate) mod transport;

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
}

#[derive(Subcommand)]
pub enum FileHandlerCommands {
    /// Reload `~/.tasty/file-handlers.toml`. host/plugin 항목은 영향 없음.
    Reload,
}

#[derive(Subcommand)]
pub enum ScriptCommands {
    /// Reload `~/.tasty/init.lua`. 기존 hook 등록은 모두 제거되고 새 init.lua 의
    /// 등록만 살아남는다.
    Reload,
}

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Create a new task within a workspace.
    TaskCreate {
        /// Workspace id (focus-independent — required).
        #[arg(long)]
        workspace_id: u32,
        /// Display name.
        #[arg(long)]
        name: String,
        /// TaskCommand as JSON (e.g. `{"kind":"run","command":["cargo","build"],"workspace_id":1}`).
        /// Accepts inline JSON or `@path/to/file.json`.
        #[arg(long)]
        command: String,
        /// Comma-separated dependency task IDs.
        #[arg(long, value_delimiter = ',')]
        depends_on: Vec<String>,
        /// On-failure policy: abort | continue_downstream | fallback:<task_id> (default: abort).
        #[arg(long, default_value = "abort")]
        on_failure: String,
        /// Metadata JSON (free-form, attached to the task).
        #[arg(long)]
        metadata: Option<String>,
    },
    /// List tasks in a workspace.
    TaskList {
        #[arg(long)]
        workspace_id: u32,
        /// Filter by state (waiting | ready | running | succeeded | failed | cancelled | skipped | unknown).
        #[arg(long)]
        state: Option<String>,
    },
    /// Fetch a single task.
    TaskGet {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
    },
    /// Poll a task's current state (returns immediately — no blocking await yet).
    TaskAwait {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
    },
    /// Cancel a task. Downstream is cascaded according to on_failure.
    TaskCancel {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
    },
    /// Retry a Failed/Cancelled/Skipped/Unknown task.
    TaskRetry {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
        /// Also reset downstream Skipped/Failed tasks back to Waiting.
        #[arg(long, default_value_t = false)]
        reset_downstream: bool,
    },
    /// Output the task DAG as JSON or Graphviz dot.
    TaskGraph {
        #[arg(long)]
        workspace_id: u32,
        /// Format: json | dot. Default: json.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Create a barrier (N개 신호가 모일 때까지 대기).
    BarrierCreate {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
        /// Required signal count (must be >= 1).
        #[arg(long)]
        count_required: u32,
        /// Optional timeout in milliseconds (from creation time).
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Signal a barrier (count_signaled++).
    BarrierSignal {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
    },
    /// Poll a barrier's current state.
    BarrierAwait {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
    },
    /// Read a barrier's current state (alias of barrier_await).
    BarrierState {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
    },
    /// Create a semaphore with N permits.
    SemaphoreCreate {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
        /// Permit count (must be >= 1).
        #[arg(long)]
        permits: u32,
    },
    /// Acquire 1 permit. Idempotent for the same holder.
    SemaphoreAcquire {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
        /// Holder id (must be non-empty).
        #[arg(long)]
        holder: String,
    },
    /// Release a permit. No-op if holder isn't currently holding.
    SemaphoreRelease {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        holder: String,
    },
    /// Acquire a lease on an arbitrary resource (advisory, cooperative).
    LeaseAcquire {
        #[arg(long)]
        workspace_id: u32,
        /// Resource identifier (e.g. `file:/path`, `workspace:foo`).
        #[arg(long)]
        resource: String,
        /// Holder id (must be non-empty).
        #[arg(long)]
        holder: String,
        /// Time-to-live in milliseconds.
        #[arg(long)]
        ttl_ms: Option<u64>,
        /// Conflict mode: fail | block (default: fail).
        #[arg(long, default_value = "fail")]
        mode: String,
    },
    /// Release a lease. Only the current holder can release.
    LeaseRelease {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        resource: String,
        #[arg(long)]
        holder: String,
    },
    /// List all leases in a workspace (expired leases auto-evicted).
    LeaseList {
        #[arg(long)]
        workspace_id: u32,
    },
    /// Combine results from N tasks into one value (strategies: first_success | all | merge_json | concat_text | custom:<cmd>).
    TaskReduce {
        #[arg(long)]
        workspace_id: u32,
        /// Comma-separated input task IDs (in reduce order).
        #[arg(long, value_delimiter = ',')]
        inputs: Vec<String>,
        /// Strategy: `first_success` | `all` | `merge_json` | `concat_text` | `custom:<command>`.
        #[arg(long)]
        strategy: String,
    },
    /// Configure a rate limit bucket for (agent, metric) — `limit` tokens per `per_ms` ms.
    RateLimitSet {
        /// Agent id.
        #[arg(long)]
        agent: String,
        /// Metric name (e.g. `ipc_calls`).
        #[arg(long)]
        metric: String,
        /// Token refill amount per window.
        #[arg(long)]
        limit: u32,
        /// Window length in milliseconds (e.g. `60000` for per-minute).
        #[arg(long)]
        per_ms: u64,
        /// Bucket capacity (defaults to `limit`).
        #[arg(long)]
        burst: Option<u32>,
    },
    /// List all configured rate limit buckets (with current token state).
    RateLimitList,
    /// Remove a rate limit bucket by id.
    RateLimitRemove {
        #[arg(long)]
        id: String,
    },
    /// Inspect rate limit state, optionally filtered by agent and/or metric.
    RateLimitStatus {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        metric: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum TelemetryCommands {
    /// Record a single metric event.
    Record {
        /// Metric name (lowercase `[a-z][a-z0-9_]*`, max 64).
        #[arg(long)]
        metric: String,
        /// Numeric value.
        #[arg(long)]
        value: f64,
        /// Operation: set | inc | dec. Default: inc.
        #[arg(long, default_value = "inc")]
        op: String,
        /// Agent id (defaults to caller — env `TASTY_AGENT_ID` or `_host`).
        #[arg(long)]
        agent: Option<String>,
        /// Workspace id binding (defaults to active workspace).
        #[arg(long)]
        workspace_id: Option<u32>,
        /// Tags as JSON object (e.g. `{"model":"opus","src":"shell"}`).
        #[arg(long)]
        tags: Option<String>,
    },
    /// Aggregate summary across events. Filters: metric, agent, workspace_id, since/until.
    Summary {
        #[arg(long)]
        metric: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        workspace_id: Option<u32>,
        /// Lower bound on unix ms (inclusive).
        #[arg(long)]
        since: Option<u64>,
        /// Upper bound on unix ms (exclusive).
        #[arg(long)]
        until: Option<u64>,
    },
    /// Window-bucketed timeseries. `--metric` is required.
    Timeseries {
        #[arg(long)]
        metric: String,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        workspace_id: Option<u32>,
        /// Window size: 1m | 1h | 1d. Default: 1m.
        #[arg(long, default_value = "1m")]
        window: String,
        #[arg(long)]
        since: Option<u64>,
        #[arg(long)]
        until: Option<u64>,
    },
    /// Top-N agents or workspaces by sum.
    Top {
        /// Grouping: agent | workspace.
        #[arg(long, default_value = "agent")]
        by: String,
        /// Maximum entries. Default: 10.
        #[arg(long, default_value_t = 10)]
        limit: u64,
        #[arg(long)]
        metric: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        workspace_id: Option<u32>,
        #[arg(long)]
        since: Option<u64>,
        #[arg(long)]
        until: Option<u64>,
    },
    /// Cost caps — threshold-based actions (stop/pause/approval/notify).
    Cap {
        #[command(subcommand)]
        command: TelemetryCapCommands,
    },
    /// Anomaly records — detected unusual patterns (call burst, etc.).
    Anomaly {
        #[command(subcommand)]
        command: TelemetryAnomalyCommands,
    },
    /// Aggregate session summary (metrics + approvals + anomalies).
    SessionSummary {
        /// Restrict to a single workspace (defaults to all).
        #[arg(long)]
        workspace_id: Option<u32>,
        /// Lower bound on event ts (unix ms, inclusive).
        #[arg(long)]
        since: Option<u64>,
        /// Upper bound on event ts (unix ms, exclusive).
        #[arg(long)]
        until: Option<u64>,
        /// Output format: markdown | json (default: markdown).
        #[arg(long, default_value = "markdown")]
        format: String,
        /// Number of top entries for ipc_calls (default: 10).
        #[arg(long)]
        top_n: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum TelemetryAnomalyCommands {
    /// List persisted anomaly records. Optional filters.
    List {
        #[arg(long)]
        agent: Option<String>,
        /// Kind filter: call_burst | slow_loop | rss_surge.
        #[arg(long)]
        kind: Option<String>,
        /// Lower bound on detection time (unix ms, inclusive).
        #[arg(long)]
        since: Option<u64>,
        /// Upper bound on detection time (unix ms, exclusive).
        #[arg(long)]
        until: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum TelemetryCapCommands {
    /// Define a new cap. Prints the generated cap id.
    Set {
        /// Agent id this cap applies to (required — no caller default).
        #[arg(long)]
        agent: String,
        /// Metric name being capped.
        #[arg(long)]
        metric: String,
        /// Threshold (positive number; sum across the window triggers the action).
        #[arg(long)]
        threshold: f64,
        /// Window: total | 1h | 1d.
        #[arg(long, default_value = "total")]
        window: String,
        /// Action: stop | pause | require_approval | notify.
        #[arg(long, default_value = "notify")]
        action: String,
    },
    /// List caps. Optional `--agent` filter.
    List {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Remove a cap by id.
    Remove {
        #[arg(long)]
        id: String,
    },
    /// Show current cumulative value vs threshold for caps. Optional `--agent` filter.
    Status {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Reset the triggered state for matching caps. Provide `--id` or `--agent`.
    Reset {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        agent: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ApprovalCommands {
    /// Request a new approval. Prints the new id on success.
    Request {
        /// Title shown in the popup / notification.
        #[arg(long)]
        title: String,
        /// Optional body (multi-line description).
        #[arg(long)]
        body: Option<String>,
        /// Comma-separated `key:label[:destructive]` triples (e.g. `approve:Approve,deny:Deny:1`).
        /// Defaults to `approve / deny` if omitted.
        #[arg(long)]
        choices: Option<String>,
        /// Default choice key applied on timeout.
        #[arg(long)]
        default_choice: Option<String>,
        /// Timeout in milliseconds. Caller may override at `await` time.
        #[arg(long)]
        timeout_ms: Option<u64>,
        /// Severity: info | warn | danger. Default: info.
        #[arg(long)]
        severity: Option<String>,
        /// Workspace id binding (defaults to active workspace).
        #[arg(long)]
        workspace_id: Option<u32>,
        /// Surface id binding (optional).
        #[arg(long)]
        surface_id: Option<u32>,
        /// Free-form JSON metadata (string passed verbatim; must parse).
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Submit a response.
    Respond {
        /// Approval id (`req_...`).
        #[arg(long)]
        id: String,
        /// Choice key (must match one of the request's `choices`).
        #[arg(long)]
        choice: String,
        /// Optional comment.
        #[arg(long)]
        comment: Option<String>,
    },
    /// Cancel a pending approval (terminal).
    Cancel {
        #[arg(long)]
        id: String,
    },
    /// Block until response / timeout / cancel. Prints the outcome as JSON.
    Await {
        #[arg(long)]
        id: String,
        /// Override request's `timeout_ms`. 0 = use request's value (or wait forever).
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Show a single approval record.
    Get {
        #[arg(long)]
        id: String,
    },
    /// List approvals. Optional state filter and workspace filter.
    List {
        /// Filter by state: pending | responded | timed_out | cancelled | terminal.
        #[arg(long)]
        state: Option<String>,
        /// Filter by workspace id.
        #[arg(long)]
        workspace_id: Option<u32>,
    },
    /// Set or read the workspace markdown summary (manual write/read only).
    Summary {
        #[command(subcommand)]
        command: ApprovalSummaryCommands,
    },
    /// Persistent history query (sources from memory store).
    History {
        /// Only entries with memory `updated_at >= since` (unix ms).
        #[arg(long)]
        since: Option<i64>,
        /// Only entries with memory `updated_at < until` (unix ms).
        #[arg(long)]
        until: Option<i64>,
        /// Filter by workspace id.
        #[arg(long)]
        workspace_id: Option<u32>,
        /// Filter by requester id (plugin/agent id or "user").
        #[arg(long)]
        requester_id: Option<String>,
        /// Filter by chosen decision key (only matches Responded).
        #[arg(long)]
        decision: Option<String>,
        /// Filter by state: pending | responded | timed_out | cancelled | terminal.
        #[arg(long)]
        state: Option<String>,
        /// Maximum entries returned (after sort, newest-first).
        #[arg(long)]
        limit: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum ApprovalSummaryCommands {
    /// Overwrite the workspace summary with `content` (or `@file`).
    Set {
        #[arg(long)]
        workspace_id: u32,
        /// Inline content, or `@path` to read from a file.
        #[arg(long)]
        content: String,
    },
    /// Print the workspace summary.
    Get {
        #[arg(long)]
        workspace_id: u32,
    },
}

#[derive(Subcommand)]
pub enum OutputCommands {
    /// Observer management subcommands
    Observe {
        #[command(subcommand)]
        command: OutputObserveCommands,
    },
}

#[derive(Subcommand)]
pub enum OutputObserveCommands {
    /// Register a new observer
    Start {
        /// Surface ID to watch (omit = all surfaces)
        #[arg(long)]
        surface: Option<u32>,
        /// Comma-separated parser ids (default: path,url,prompt_boundary,exit_code)
        #[arg(long, value_delimiter = ',')]
        parsers: Option<Vec<String>>,
        /// Comma-separated kind filter (default: all)
        #[arg(long, value_delimiter = ',')]
        kinds: Option<Vec<String>>,
        /// Sink type: "memory" or "file" (default: memory)
        #[arg(long, default_value = "memory")]
        sink: String,
        /// File path (only when --sink file; omit = ~/.tasty/observers/<id>.jsonl)
        #[arg(long)]
        path: Option<String>,
        /// Memory sink ring-buffer cap (only when --sink memory; 0 = unlimited)
        #[arg(long, default_value_t = 10_000)]
        max_records: usize,
    },
    /// Stop an observer by id
    Stop {
        /// Observer id (from `output observe list`)
        #[arg(long)]
        observer: u64,
    },
    /// List all active observers
    List,
    /// Show stats for a single observer
    Info {
        #[arg(long)]
        observer: u64,
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
    /// Diagnose a plugin's manifest — list contributed detectors / handlers and
    /// flag rule kinds the current host does not understand.
    Doctor {
        /// Plugin id (e.g. com.example.foo).
        id: String,
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
    /// Grant a temporary permission to an active agent session.
    /// Permission lives until TTL expiry or explicit revoke; base permissions
    /// (issued at session.issue) are unaffected.
    GrantAgentPermission {
        /// Agent id (e.g. `claude:child-1`).
        #[arg(long)]
        agent: String,
        /// Permission token (e.g. fs.write, surface.write).
        #[arg(long)]
        permission: String,
        /// Time-to-live in seconds. Omit for indefinite (until revoke).
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Revoke a previously-granted temporary permission from an agent session.
    /// Does not affect base permissions assigned at issue time.
    RevokeAgentPermission {
        /// Agent id.
        #[arg(long)]
        agent: String,
        /// Permission token.
        #[arg(long)]
        permission: String,
    },
    /// List base + temporary permissions for active agent sessions.
    ListAgentPermissions {
        /// Filter to a specific agent. Omit to list all active sessions.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Manually publish a capability elevation approval. Useful for operators
    /// pre-granting a permission before the agent's first call. The popup is
    /// the same one shown by automatic elevation on permission_denied.
    RequestPermission {
        /// Agent id to grant on approval.
        #[arg(long)]
        agent: String,
        /// Permission token (e.g. fs.write).
        #[arg(long)]
        permission: String,
        /// Reason shown to the user in the popup body.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Inspect plugin extensions (extends-blocks).
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },
    /// Query the IPC audit log. Returns filtered records with allow/deny decisions.
    AuditQuery {
        /// Filter by caller kind: local | internal | plugin | agent.
        #[arg(long)]
        caller_kind: Option<String>,
        /// Filter by caller id (plugin id or agent id).
        #[arg(long)]
        caller_id: Option<String>,
        /// Filter to methods starting with this prefix (e.g. `surface.`).
        #[arg(long)]
        method_prefix: Option<String>,
        /// Filter by decision: allow | deny.
        #[arg(long)]
        decision: Option<String>,
        /// Lower bound on timestamp (unix ms, inclusive).
        #[arg(long)]
        since_ms: Option<u64>,
        /// Upper bound on timestamp (unix ms, exclusive).
        #[arg(long)]
        until_ms: Option<u64>,
        /// Cap on number of records returned.
        #[arg(long)]
        limit: Option<u64>,
    },
    /// Aggregate audit records into totals + top callers/methods.
    AuditSummary {
        /// Filter by caller kind: local | internal | plugin | agent.
        #[arg(long)]
        caller_kind: Option<String>,
        /// Filter by caller id.
        #[arg(long)]
        caller_id: Option<String>,
        /// Filter to methods starting with this prefix.
        #[arg(long)]
        method_prefix: Option<String>,
        /// Filter by decision: allow | deny.
        #[arg(long)]
        decision: Option<String>,
        /// Lower bound on timestamp (unix ms, inclusive).
        #[arg(long)]
        since_ms: Option<u64>,
        /// Upper bound on timestamp (unix ms, exclusive).
        #[arg(long)]
        until_ms: Option<u64>,
        /// Top-N cap for by_caller / by_method lists. Default: 10.
        #[arg(long)]
        top_n: Option<u64>,
    },
    /// Delete audit records older than `before_ms`, or all if omitted.
    AuditClear {
        /// Delete records with ts < before_ms. Omit to clear everything.
        #[arg(long)]
        before_ms: Option<u64>,
    },
    /// Tail audit records as they arrive. Polls every `interval_ms` ms until
    /// Ctrl-C. Filters are the same as `audit-query`.
    AuditFollow {
        /// Filter by caller kind: local | internal | plugin | agent.
        #[arg(long)]
        caller_kind: Option<String>,
        /// Filter by caller id.
        #[arg(long)]
        caller_id: Option<String>,
        /// Filter to methods starting with this prefix.
        #[arg(long)]
        method_prefix: Option<String>,
        /// Filter by decision: allow | deny.
        #[arg(long)]
        decision: Option<String>,
        /// Per-poll cap on record batch size.
        #[arg(long, default_value_t = 100)]
        batch: u64,
        /// Polling interval in milliseconds.
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
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
    /// Blackboard — workspace 단위 키-값 컬렉션 (`tasty.bb.<name>.*`).
    Bb {
        #[command(subcommand)]
        command: MemoryBbCommands,
    },
    /// Plan — workspace 단위 선언적 work breakdown (`tasty.plan.<plan_id>`).
    Plan {
        #[command(subcommand)]
        command: MemoryPlanCommands,
    },
    /// Cache — workspace 단위 TTL 캐시 (`tasty.cache.<key>`).
    Cache {
        #[command(subcommand)]
        command: MemoryCacheCommands,
    },
}

/// `tasty memory cache ...` 서브커맨드.
#[derive(Subcommand)]
pub enum MemoryCacheCommands {
    /// Store a value with required TTL (seconds).
    Put {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        key: String,
        /// Value. Treated as JSON if it parses, otherwise plain text. `@path` reads from file.
        #[arg(long)]
        value: Option<String>,
        /// Base64-encoded binary payload.
        #[arg(long)]
        value_b64: Option<String>,
        /// Force content type.
        #[arg(long)]
        content_type: Option<String>,
        /// TTL in seconds (required, > 0).
        #[arg(long)]
        ttl: u64,
    },
    /// Read a cached entry (returns null if missing/expired).
    Get {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        key: String,
    },
    /// Remove a single cached entry (idempotent).
    Invalidate {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        key: String,
    },
    /// Remove all cached entries in the workspace.
    Clear {
        #[arg(long)]
        workspace: u32,
    },
    /// List cached keys in the workspace.
    List {
        #[arg(long)]
        workspace: u32,
    },
}

/// `tasty memory plan ...` 서브커맨드.
#[derive(Subcommand)]
pub enum MemoryPlanCommands {
    /// Create a new plan. `--steps` accepts a JSON array of step objects.
    Create {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        title: String,
        /// JSON array of steps (e.g. `'[{"id":"a","title":"first"}]'`).
        #[arg(long)]
        steps: Option<String>,
    },
    /// Read full plan JSON.
    Get {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
    },
    /// List plan ids in a workspace.
    List {
        #[arg(long)]
        workspace: u32,
    },
    /// Delete a plan.
    Delete {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
    },
    /// Append or insert a step.
    AddStep {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        /// JSON object for the step.
        #[arg(long)]
        step: String,
        /// Insert position (0-based). Default: append.
        #[arg(long)]
        position: Option<usize>,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Remove a step (rejected if referenced by `depends_on`).
    RemoveStep {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        step_id: String,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Update step state and/or notes.
    UpdateStep {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        step_id: String,
        /// One of: pending | in_progress | completed | failed | skipped.
        #[arg(long)]
        state: Option<String>,
        /// Set notes to this value. Use `--clear-notes` instead to remove.
        #[arg(long, conflicts_with = "clear_notes")]
        notes: Option<String>,
        /// Clear the notes field (sets to None).
        #[arg(long)]
        clear_notes: bool,
        #[arg(long)]
        cas: Option<u64>,
    },
}

/// `tasty memory bb ...` 서브커맨드. 모든 명령은 `--workspace <id>` 필수.
#[derive(Subcommand)]
pub enum MemoryBbCommands {
    /// Create a new blackboard with optional schema (JSON).
    Create {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        /// Schema JSON literal. Stored as-is; no validation performed.
        #[arg(long)]
        schema: Option<String>,
    },
    /// Write a field value.
    Put {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        field: String,
        /// Value. Treated as JSON if it parses, otherwise plain text. `@path` reads from file.
        #[arg(long)]
        value: Option<String>,
        /// Base64-encoded binary payload.
        #[arg(long)]
        value_b64: Option<String>,
        /// Force content type.
        #[arg(long)]
        content_type: Option<String>,
        /// CAS version (must match current field version).
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Read a single field.
    Get {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        field: String,
    },
    /// Read all fields of a blackboard (`_meta` excluded).
    GetAll {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Read the `_meta` entry (schema/created_by/...).
    GetMeta {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Delete a single field.
    DeleteField {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        field: String,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Delete the entire blackboard (`_meta` + all fields).
    Delete {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// List blackboard names in a workspace.
    List {
        #[arg(long)]
        workspace: u32,
    },
    /// Check whether a blackboard exists (= `_meta` present).
    Exists {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Capture the current bb state as a snapshot (`tasty.bb.<name>.snapshots.<id>`).
    Snapshot {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
    /// Read a snapshot JSON.
    SnapshotGet {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
    /// List snapshot ids for a bb.
    SnapshotList {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Delete a snapshot.
    SnapshotDelete {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
    /// Restore bb fields from a snapshot (replaces current fields).
    SnapshotRestore {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
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
        /// Tool item key (`<plugin_id>/<tool_id>`, e.g. `com.tasty.clipboard-history/open-viewer`)
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
    // plugin doctor is local-only — read manifest from disk, no IPC needed.
    if let Commands::Plugin {
        command: PluginCommands::Doctor { id },
    } = &command
    {
        return crate::cli::plugin::run_plugin_doctor(id);
    }
    // plugin audit-follow is a polling loop over plugin.audit_follow IPC.
    if let Commands::Plugin {
        command:
            PluginCommands::AuditFollow {
                caller_kind,
                caller_id,
                method_prefix,
                decision,
                batch,
                interval_ms,
            },
    } = &command
    {
        return crate::cli::plugin::run_audit_follow(
            caller_kind.as_deref(),
            caller_id.as_deref(),
            method_prefix.as_deref(),
            decision.as_deref(),
            *batch,
            *interval_ms,
        );
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
