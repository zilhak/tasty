use super::ScopeArgs;
use clap::Subcommand;

/// `tasty memory secret ...` subcommands.
#[derive(Subcommand)]
pub enum MemorySecretCommands {
    /// Store a secret value at scope/key.
    Put {
        #[command(flatten)]
        scope: ScopeArgs,
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
    /// Read a single secret entry.
    Get {
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long)]
        key: String,
    },
    /// Delete a secret (idempotent).
    Delete {
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long)]
        key: String,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Check whether a secret exists at scope/key.
    Exists {
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long)]
        key: String,
    },
    /// List secret entries in a scope.
    List {
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Count secrets in a scope.
    Count {
        #[command(flatten)]
        scope: ScopeArgs,
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
