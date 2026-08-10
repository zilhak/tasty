//! `tasty agent ...` subcommand 정의.

use clap::{Subcommand, ValueEnum};

/// `agent task-run` action.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum TaskRunAction {
    Start,
    Stop,
    Status,
}

impl TaskRunAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskRunAction::Start => "start",
            TaskRunAction::Stop => "stop",
            TaskRunAction::Status => "status",
        }
    }
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
        /// TaskCommand as JSON (e.g. `{"kind":"run","command":["cargo","build"]}`).
        /// `workspace_id` is auto-filled from `--workspace-id` if omitted; if present
        /// and different, `--workspace-id` wins and a warning is returned.
        /// Accepts inline JSON or `@path/to/file.json`.
        #[arg(long)]
        command: String,
        /// Comma-separated dependency task IDs.
        #[arg(long, value_delimiter = ',')]
        depends_on: Vec<String>,
        /// On-failure policy: abort | continue_downstream | fallback:<task_id> (default: abort).
        /// Which task you set this on matters: abort/continue_downstream must be
        /// set on the dependent (downstream) task; fallback must be set on the
        /// task that may itself fail (upstream) — setting fallback on a
        /// downstream task has no effect on skip cascades from a failed dependency.
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
    /// Wait for a task to reach a terminal state (blocking). Omitted = wait up
    /// to 10 minutes (provisional default). `--timeout-ms 0` = wait indefinitely.
    TaskAwait {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
        #[arg(long)]
        timeout_ms: Option<u64>,
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
    /// Start/stop/inspect the agent task runner for a workspace.
    /// runner 는 Ready task 를 자동 dispatch + Running task 의 완료를 감지하는
    /// host 측 thread. 같은 workspace 에 두 번 start 호출은 idempotent.
    TaskRun {
        #[arg(long)]
        workspace_id: u32,
        /// start | stop | status. 기본: status.
        #[arg(long, value_enum, default_value_t = TaskRunAction::Status)]
        action: TaskRunAction,
    },
    /// Manually report a task's terminal result (succeeded | failed).
    /// runner thread 가 dispatch 한 task 외 *외부/수동* task 의 완료 신호용.
    TaskSetResult {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
        /// Terminal state: `succeeded` | `failed`.
        #[arg(long)]
        state: String,
        /// Optional output JSON. Accepts inline JSON or `@path/to/file.json`.
        #[arg(long)]
        output: Option<String>,
        /// Optional error message (recommended for state=failed).
        #[arg(long)]
        error: Option<String>,
        /// Optional process exit code.
        #[arg(long)]
        exit_code: Option<i32>,
    },
    /// Delete a task. Rejected by default if other tasks still reference it
    /// (depends_on / on_failure.fallback.task / reduce.inputs) — the
    /// referencing task IDs are returned. `Running` tasks are always
    /// rejected regardless of `--cascade`/`--force` (cancel first).
    TaskDelete {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        id: String,
        /// Also delete every task that transitively references this one.
        #[arg(long, default_value_t = false)]
        cascade: bool,
        /// Skip the reference check only — state constraints (Running) still apply.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Bulk-delete tasks matching state/age filters. At least one of
    /// `--states`/`--older-than-ms` is required. Only tasks that are safe
    /// to delete (not Running, not referenced from outside the matched set)
    /// are actually removed; the rest are reported as retained.
    TaskPurge {
        #[arg(long)]
        workspace_id: u32,
        /// Comma-separated state names (waiting|ready|running|succeeded|failed|cancelled|skipped|unknown).
        #[arg(long, value_delimiter = ',')]
        states: Vec<String>,
        /// Only tasks whose age (created_at, or finished_at for terminal
        /// tasks) exceeds this many milliseconds are candidates.
        #[arg(long)]
        older_than_ms: Option<u64>,
        /// Compute and print the plan without deleting anything.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
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
    /// List all barriers in a workspace.
    BarrierList {
        #[arg(long)]
        workspace_id: u32,
    },
    /// Delete a barrier (no-op if missing).
    BarrierDelete {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
    },
    /// List all semaphores in a workspace.
    SemaphoreList {
        #[arg(long)]
        workspace_id: u32,
    },
    /// Delete a semaphore (no-op if missing).
    SemaphoreDelete {
        #[arg(long)]
        workspace_id: u32,
        #[arg(long)]
        name: String,
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
