//! `tasty file-handler` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum FileHandlerCommands {
    /// Reload `~/.tasty/file-handlers.toml`. Host and plugin entries are
    /// unaffected.
    Reload,
    /// List the file format detectors as they are merged now — display name,
    /// icon, enabled state and rules — together with what each source (host,
    /// plugin, user) contributed.
    Detectors,
    /// Open a path through the file-handler dispatch flow — the same route the
    /// explorer's double-click takes.
    Dispatch {
        /// Path to dispatch.
        path: String,
        /// Detection depth: `cheap` (extension, filename glob, directory check)
        /// or `deep` (also inspect contents). Ignored when the origin surface
        /// belongs to a mirror workspace: the path is a remote file, so only the
        /// extension and filename glob are checked. The response still echoes
        /// the requested depth.
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
