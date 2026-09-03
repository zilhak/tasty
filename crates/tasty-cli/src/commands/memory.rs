//! Agent memory CLI subcommands — `MemoryCommands` + 5개 sub (Cache / Goal / Plan / Bb / Secret).
//!
//! Scope formats: `global`, `account:<userid>`, `window:<id>`, `workspace:<id>`, `surface:<id>`.
//! `--surface <id>` 같은 alias 가 대응 scope 로 정규화된다.

use clap::Subcommand;

/// Agent memory CLI. Scope formats:
/// `global`, `account:<userid>`, `window:<id>`, `workspace:<id>`, `surface:<id>`.
/// Aliases such as `--surface <id>` are normalized to the matching scope.
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
    /// Blackboard — per-workspace key-value collections (`tasty.bb.<name>.*`).
    Bb {
        #[command(subcommand)]
        command: MemoryBbCommands,
    },
    /// Plan — per-workspace declarative work breakdown (`tasty.plan.<plan_id>`).
    Plan {
        #[command(subcommand)]
        command: MemoryPlanCommands,
    },
    /// Cache — per-workspace TTL cache (`tasty.cache.<key>`).
    Cache {
        #[command(subcommand)]
        command: MemoryCacheCommands,
    },
    /// Goal — a single goal sentence per surface (`tasty.goal`).
    Goal {
        #[command(subcommand)]
        command: MemoryGoalCommands,
    },
}

mod bb;
mod cache;
mod goal;
mod plan;
mod secret;

pub use bb::MemoryBbCommands;
pub use cache::MemoryCacheCommands;
pub use goal::MemoryGoalCommands;
pub use plan::MemoryPlanCommands;
pub use secret::MemorySecretCommands;
