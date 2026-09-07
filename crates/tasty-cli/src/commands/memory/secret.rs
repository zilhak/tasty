use super::ScopeArgs;
use clap::Subcommand;

/// `tasty memory secret ...` subcommands.
#[derive(Subcommand)]
pub enum MemorySecretCommands {
    /// Store a secret value at scope/key.
    Put {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Secret key within the scope.
        #[arg(long)]
        key: String,
        /// Value. Treated as JSON if it parses, otherwise plain text. `@path` reads from file.
        #[arg(long)]
        value: Option<String>,
        /// Force content type.
        #[arg(long)]
        value_b64: Option<String>,
        /// Force content type.
        #[arg(long)]
        content_type: Option<String>,
        /// Relative TTL in seconds. Conflicts with --expires-at.
        #[arg(long, conflicts_with = "expires_at")]
        ttl: Option<u64>,
        /// Absolute expiry (unix ms). Conflicts with --ttl.
        #[arg(long)]
        expires_at: Option<i64>,
        /// CAS version (must match the current entry version).
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Read a single secret entry.
    Get {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Secret key within the scope.
        #[arg(long)]
        key: String,
    },
    /// Delete a secret (idempotent).
    Delete {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Secret key within the scope.
        #[arg(long)]
        key: String,
        /// CAS version (must match the current entry version).
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Check whether a secret exists at scope/key.
    Exists {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Secret key within the scope.
        #[arg(long)]
        key: String,
    },
    /// List secret entries in a scope.
    List {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Only keys starting with this prefix.
        #[arg(long)]
        prefix: Option<String>,
        /// Maximum number of entries to return.
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
    /// Count secrets in a scope.
    Count {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Only keys starting with this prefix.
        #[arg(long)]
        prefix: Option<String>,
    },
    /// List scopes that hold at least one secret.
    Scopes,
    /// Secret store statistics. Scope selector is optional.
    Stats {
        #[command(flatten)]
        scope: ScopeArgs,
    },
}
