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
    ///
    /// Prints the acceptance (with a `run` number) on one line, then waits for
    /// the final result and prints it on a second line: `state` is
    /// `succeeded` or `failed`, and `result` says what was opened or why it
    /// was not. An opened tab is reported in `surface_id`. Exits with code 1
    /// unless the final state is `succeeded`.
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
        /// Print only the acceptance and return without waiting. Look the
        /// result up later with `tasty file-handler result --run <N>`.
        #[arg(long)]
        no_wait: bool,
    },
    /// Show the progress or final result of a dispatch by its `run` number.
    ///
    /// `done` is false while it is still running. The last 128 runs, counted
    /// together with hook handler dispatches, are kept; an older or unknown
    /// run is an error.
    Result {
        /// The `run` number from the dispatch acceptance.
        #[arg(long)]
        run: u64,
    },
}
