use clap::Subcommand;

/// `tasty memory goal ...` subcommands.
///
/// `--surface` is optional: when omitted it is resolved from the caller's
/// `TASTY_SURFACE_ID` environment variable (the main use case is an agent
/// calling this about itself). It is an error if neither is available.
#[derive(Subcommand)]
pub enum MemoryGoalCommands {
    /// Set the surface's goal (overwrites any existing one). Blank goals are rejected.
    Set {
        /// Goal sentence.
        goal: String,
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Read the surface's goal (returns null if unset).
    Get {
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Remove the surface's goal (idempotent).
    Clear {
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
}
