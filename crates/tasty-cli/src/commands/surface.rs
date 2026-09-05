//! `tasty surface` subcommand 정의 — surface 대상 액션(completion 등).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum SurfaceCommands {
    /// Signal that a surface has completed its work (or needs input), raising
    /// attention highlight.
    ///
    /// The target surface is always given explicitly with --surface; it is
    /// never inferred from focus.
    Completion {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
        /// Attention kind: "completion" (default) or "needs_input". Any other
        /// value is treated as "completion".
        #[arg(long)]
        kind: Option<String>,
    },
    /// Print where the cursor sits in a terminal surface, as column and row.
    ///
    /// The target surface is always given explicitly with --surface; it is
    /// never inferred from focus.
    CursorPosition {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Print whether the program inside a terminal surface has grabbed the
    /// mouse, and at which level.
    ///
    /// Use it before sending mouse sequences or expecting drag selection to
    /// work: when tracking is on, the program consumes mouse input; when it is
    /// off, mouse sequences would land on screen as stray characters. The
    /// target surface is always given explicitly with --surface.
    MouseTracking {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Print the process currently in the foreground of a terminal surface.
    ///
    /// Use it to tell whether a shell is idle or is running something. The
    /// target surface is always given explicitly with --surface.
    ForegroundProcess {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Print which pane a surface belongs to, and whether it is still in the
    /// tree at all.
    ///
    /// Use it to check that a surface you created or are waiting on has not
    /// been closed. The target surface is always given explicitly with
    /// --surface.
    Locate {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Restart the shell of a terminal surface in place, keeping the surface,
    /// its tab and its position.
    ///
    /// The target surface is always given explicitly with --surface.
    RespawnTerminal {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Fire a surface hook by name, as if that event had just happened.
    ///
    /// The target surface is always given explicitly with --surface.
    FireHook {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
        /// Hook event name to fire (required)
        #[arg(long)]
        event: String,
    },
    /// Inspect or clear the attention highlight of a surface.
    ///
    /// The reverse of `completion`. The other two clear paths (real render
    /// focus and reading the notification) are GUI-local events, so this is
    /// the only way to clear attention on a headless instance.
    Attention {
        #[command(subcommand)]
        command: SurfaceAttentionCommands,
    },
}

#[derive(Subcommand)]
pub enum SurfaceAttentionCommands {
    /// Show the attention kind recorded for a surface: "completion",
    /// "needs_input", or null when the surface has none.
    ///
    /// The target surface is always given explicitly with --surface; it is
    /// never inferred from focus. Allowed while the surface is occupied by a
    /// remote attach session (this is a read-only query).
    Get {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Clear the attention highlight of a surface.
    ///
    /// The target surface is always given explicitly with --surface; it is
    /// never inferred from focus. Clearing a surface that has no attention
    /// succeeds and reports "cleared": false. Rejected for a surface that is
    /// hard-occupied by a remote attach session, and for a mirror surface of
    /// a remote workspace: in both cases the attention belongs to the
    /// instance that owns the surface.
    Clear {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
        /// Only clear when the recorded kind matches: "completion" or
        /// "needs_input". Omit to clear regardless of kind. Any other value
        /// is rejected.
        #[arg(long)]
        kind: Option<String>,
    },
}
