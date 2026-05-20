//! `tasty clipboard ...` subcommand 정의.

use clap::Subcommand;

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
