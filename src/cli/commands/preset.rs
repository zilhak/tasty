//! `tasty preset ...` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum PresetCommands {
    /// List presets of a given kind.
    List {
        /// "workspace" | "tab" | "pane"
        #[arg(long)]
        kind: String,
    },
    /// Show a preset's JSON.
    Get {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        name: String,
    },
    /// Save a preset from a JSON file ("-" reads stdin).
    Save {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        name: String,
        /// Path to JSON file. "-" reads stdin.
        #[arg(long)]
        file: String,
        /// Allow overwriting existing preset.
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
    /// Delete a preset.
    Delete {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        name: String,
    },
    /// Rename a preset.
    Rename {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// Capture current workspace / tab / pane into a preset.
    Capture {
        #[arg(long)]
        kind: String,
        /// Source ID — workspace_id (kind=workspace), tab_id (kind=tab), or pane_id (kind=pane).
        /// Required; CLI is focus-independent.
        #[arg(long)]
        source_id: u32,
        /// Preset name. Omit → auto-generated unique name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Apply a preset. Focus is not changed (focus-independent CLI).
    Apply {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        name: String,
        /// For kind=tab: target pane ID.
        #[arg(long)]
        target_pane: Option<u32>,
        /// For kind=pane: target workspace ID.
        #[arg(long)]
        target_workspace: Option<u32>,
    },
}

