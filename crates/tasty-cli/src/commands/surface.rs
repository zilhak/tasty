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
}
