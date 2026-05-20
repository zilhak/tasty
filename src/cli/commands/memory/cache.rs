use clap::Subcommand;

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

