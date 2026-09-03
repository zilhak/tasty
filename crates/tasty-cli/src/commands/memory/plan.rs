use clap::Subcommand;

/// `tasty memory plan ...` subcommands.
#[derive(Subcommand)]
pub enum MemoryPlanCommands {
    /// Create a new plan. `--steps` accepts a JSON array of step objects.
    Create {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        title: String,
        /// JSON array of steps (e.g. `'[{"id":"a","title":"first"}]'`).
        #[arg(long)]
        steps: Option<String>,
    },
    /// Read full plan JSON.
    Get {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
    },
    /// List plan ids in a workspace.
    List {
        #[arg(long)]
        workspace: u32,
    },
    /// Delete a plan.
    Delete {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
    },
    /// Append or insert a step.
    AddStep {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        /// JSON object for the step.
        #[arg(long)]
        step: String,
        /// Insert position (0-based). Default: append.
        #[arg(long)]
        position: Option<usize>,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Remove a step (rejected if referenced by `depends_on`).
    RemoveStep {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        step_id: String,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Update step state and/or notes.
    UpdateStep {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        step_id: String,
        /// One of: pending | in_progress | completed | failed | skipped.
        #[arg(long)]
        state: Option<String>,
        /// Set notes to this value. Use `--clear-notes` instead to remove.
        #[arg(long, conflicts_with = "clear_notes")]
        notes: Option<String>,
        /// Clear the notes field (sets to None).
        #[arg(long)]
        clear_notes: bool,
        #[arg(long)]
        cas: Option<u64>,
    },
}
