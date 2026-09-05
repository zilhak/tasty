//! `tasty read` / `tasty unset` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ReadCommands {
    /// Read output since last mark
    #[command(name = "since-mark")]
    SinceMark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Strip ANSI escape sequences from output
        #[arg(long)]
        strip_ansi: bool,
    },
    /// Parse output since last mark with builtin parsers (path/url/prompt_boundary/exit_code)
    #[command(name = "parse-since-mark")]
    ParseSinceMark {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Comma-separated parser ids. Default = all builtins.
        #[arg(long, value_delimiter = ',')]
        parsers: Option<Vec<String>>,
    },
    /// Read from a surface's message queue (consumes oldest message)
    Queue {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Filter by sender surface ID
        #[arg(long)]
        from: Option<u32>,
        /// Peek without consuming
        #[arg(long)]
        peek: bool,
        /// Clear all messages instead of reading
        #[arg(long)]
        clear: bool,
    },
    /// Read current screen text of a surface
    Screen {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Number of lines to read from the bottom, counted from the end of the
        /// content: trailing blank rows are skipped and the shortfall is filled from
        /// scrollback. The ceiling is the content, not the grid — rows below the last
        /// printed line never come back, so a mostly empty screen returns fewer than N
        /// even with scrollback to spare. On the alternate screen (a full-screen TUI)
        /// the fill comes from the *primary* buffer's scrollback and arrives with no
        /// marker, so a large N can prepend shell history above the TUI's own screen.
        #[arg(long)]
        lines: Option<usize>,
        /// Include dim (ghost-suggestion, e.g. Claude Code autocomplete overlay) cells.
        /// Default excludes them so unsubmitted UI suggestions aren't mistaken for
        /// real buffer content.
        #[arg(long)]
        show_dim: bool,
    },
    /// List recorded shell commands (OSC 133) for a surface
    Commands {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// Limit number of returned records
        #[arg(long)]
        limit: Option<usize>,
        /// Only include records ended at or after this unix-ms timestamp
        #[arg(long)]
        since: Option<i64>,
    },
    /// Most recent recorded command for a surface
    #[command(name = "last-command")]
    LastCommand {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Recorded command at index (negative = from end)
    #[command(name = "command-at")]
    CommandAt {
        /// Surface ID (default: focused terminal)
        #[arg(long)]
        surface: Option<u32>,
        /// 0-based index; negatives count from the end
        #[arg(long, allow_hyphen_values = true)]
        index: i64,
    },
}

#[derive(Subcommand)]
pub enum UnsetCommands {
    /// Remove a hook
    Hook {
        /// Hook ID to remove
        #[arg(long)]
        hook: u64,
    },
    /// Remove a global hook by ID
    GlobalHook {
        /// Hook ID to remove
        #[arg(long)]
        hook: u32,
    },
}
