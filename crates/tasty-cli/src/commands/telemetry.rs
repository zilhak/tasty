//! `tasty telemetry ...` subcommand 정의 — Telemetry + Anomaly + Cap.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum TelemetryCommands {
    /// Record several events in one call — they share one timestamp, so their
    /// order is preserved. Use this instead of a loop of `record` when the
    /// ordering between the events matters.
    RecordBatch {
        /// Events as a JSON array; each element takes the same fields as
        /// `record` (`metric`, `value`, `op`, `tags`, ...).
        #[arg(long)]
        events: String,
    },
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
