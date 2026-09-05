//! `tasty file-handler` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum FileHandlerCommands {
    /// Reload `~/.tasty/file-handlers.toml`. Host and plugin entries are
    /// unaffected.
    Reload,
    /// Open a path through the file-handler dispatch flow — the same route the
    /// explorer's double-click takes.
    Dispatch {
        /// Path to dispatch.
        path: String,
        /// Detection depth: `cheap` (extension only) or `deep` (sniff contents).
        #[arg(long, default_value = "cheap")]
        depth: String,
        /// Add the resulting surface as a tab of this surface's pane instead of
        /// the focused pane.
        #[arg(long)]
        origin_surface: Option<u32>,
        /// Skip the large-file confirmation popup and open immediately.
        #[arg(long)]
        ignore_size_limit: bool,
    },
}
