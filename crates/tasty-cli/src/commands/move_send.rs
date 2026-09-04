//! `tasty move` / `tasty send` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum MoveCommands {
    /// Move a tab to a different position within the same pane
    Tab {
        /// Pane ID
        #[arg(long)]
        pane: u32,
        /// Source tab index (0-based)
        #[arg(long)]
        from: u64,
        /// Destination tab index (0-based)
        #[arg(long)]
        to: u64,
    },
    /// Move a workspace to a different position
    Workspace {
        /// Source workspace index (0-based)
        #[arg(long)]
        from: u64,
        /// Destination workspace index (0-based)
        #[arg(long)]
        to: u64,
    },
}

#[derive(Subcommand)]
pub enum SendCommands {
    /// Send text to a terminal surface
    Text {
        /// Text to send
        text: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Skip the send when the person is typing, deciding and sending in one
        /// step. Reports "sent": false with "reason": "typing" instead. Checking
        /// with `tasty is-typing` first cannot do this - they may start typing
        /// between the two commands.
        #[arg(long)]
        wait_idle: bool,
    },
    /// Send a key to a terminal surface (enter, tab, escape, up, down, etc.)
    Key {
        /// Key name
        key: String,
        /// Target surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Send a message to a surface's queue
    Queue {
        /// Target surface ID
        #[arg(long)]
        to: u32,
        /// Message content
        #[arg()]
        content: String,
        /// Sender surface ID (default: focused)
        #[arg(long)]
        from: Option<u32>,
    },
}
