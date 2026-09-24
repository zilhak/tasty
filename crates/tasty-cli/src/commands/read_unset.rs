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
        /// Read from this position instead of the mark: the next_cursor of a previous read.
        /// The server keeps nothing for you, so several readers never move each other.
        /// Needs --stream from the same earlier reply
        #[arg(long, requires = "stream")]
        cursor: Option<u64>,
        /// Stream token from an earlier reply. A position from another stream (the surface
        /// was reused or its terminal respawned) is refused instead of applied
        #[arg(long)]
        stream: Option<String>,
        /// Upper bound on the raw output bytes this read returns (the server clamps it to
        /// 1..=its retention, so 0 reads one byte rather than meaning no limit — leave the
        /// flag out for no limit). Continue from next_cursor for the rest
        #[arg(long)]
        max_bytes: Option<usize>,
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
        /// Read up to N lines ending at the last nonblank screen row. Trailing blank
        /// screen rows are skipped; any shortfall is filled from the primary scrollback.
        /// If the screen content and scrollback together contain fewer than N lines,
        /// return all available lines. On the alternate screen (a full-screen TUI),
        /// primary scrollback is still prepended without a marker, so the result may
        /// include shell history before the TUI's screen text.
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
