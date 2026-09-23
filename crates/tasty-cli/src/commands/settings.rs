//! Global terminal input rules and remote-transfer storage settings.
//! Settings are instance-wide; no focused surface or workspace is required.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum SettingsCommands {
    /// List application-specific Shift+Enter rules.
    GetInputRules,
    /// Set an application's Shift+Enter encoding (true = LF, false = platform default).
    SetInputRule {
        #[arg(long)]
        app: String,
        #[arg(long, action = clap::ArgAction::Set)]
        shift_enter_newline: bool,
    },
    /// Remove an application's input rule.
    RemoveInputRule {
        #[arg(long)]
        app: String,
    },
    /// Seed an absent rule once, preserving later user edits and deletion.
    InitializeInputRule {
        #[arg(long)]
        app: String,
        #[arg(long, action = clap::ArgAction::Set)]
        shift_enter_newline: bool,
    },
    /// Show the receive-side storage folder and size cap for remote transfers
    /// (bulk file channel).
    GetRemoteTransfer,
    /// Set the remote-transfer storage folder and/or size cap. At least one
    /// must be given.
    SetRemoteTransfer {
        /// Storage folder path (empty string = default `~/.tasty/transfers/`).
        #[arg(long)]
        dir: Option<String>,
        /// Maximum folder size in MiB.
        #[arg(long)]
        max_mb: Option<u64>,
    },
}
