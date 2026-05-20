use clap::Subcommand;

/// `tasty memory secret ...` 서브커맨드.
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
