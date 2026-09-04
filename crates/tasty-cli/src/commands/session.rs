use clap::Subcommand;

#[derive(Subcommand)]
pub enum SessionCommands {
    /// Issue a session token for a child agent, naming exactly the permissions
    /// it may use.
    ///
    /// The token is what the child puts in TASTY_SESSION_TOKEN. Pass
    /// --permission once per permission; a token with none can still call the
    /// methods that need no permission at all.
    Issue {
        /// Agent identity the token is issued to (required)
        #[arg(long)]
        agent_id: String,
        /// Permission token to grant, repeatable (e.g. surface.read)
        #[arg(long = "permission")]
        permissions: Vec<String>,
        /// Lifetime in milliseconds. Omit for the server default.
        #[arg(long)]
        ttl_ms: Option<u64>,
    },
    /// Revoke a session token so the agent holding it stops being trusted.
    Revoke {
        /// The token string to revoke (required)
        #[arg(long)]
        token: String,
    },
    /// List the session tokens this instance currently holds.
    List,
}
