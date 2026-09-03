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
