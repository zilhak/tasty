use clap::Subcommand;

/// `tasty memory bb ...` subcommands.
#[derive(Subcommand)]
pub enum MemoryBbCommands {
    /// Create a new blackboard with optional schema (JSON).
    Create {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Schema JSON literal. Stored as-is; no validation performed.
        #[arg(long)]
        schema: Option<String>,
    },
    /// Write a field value.
    Put {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Field name.
        #[arg(long)]
        field: String,
        /// Value. Treated as JSON if it parses, otherwise plain text. `@path` reads from file.
        #[arg(long)]
        value: Option<String>,
        /// Base64-encoded binary payload.
        #[arg(long)]
        value_b64: Option<String>,
        /// Force content type.
        #[arg(long)]
        content_type: Option<String>,
        /// CAS version (must match current field version).
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Read a single field.
    Get {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Field name.
        #[arg(long)]
        field: String,
    },
    /// Read all fields of a blackboard (`_meta` excluded).
    GetAll {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
    },
    /// Read the `_meta` entry (schema/created_by/...).
    GetMeta {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
    },
    /// Delete a single field.
    DeleteField {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Field name.
        #[arg(long)]
        field: String,
        /// CAS version (must match the current field version).
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Delete the entire blackboard (`_meta` + all fields).
    ///
    /// Its snapshots go with it. They live under the board's own key prefix, so
    /// a snapshot is a point you can roll back to while the board exists — not a
    /// backup that outlives deleting it.
    Delete {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
    },
    /// List blackboard names in a workspace.
    List {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
    },
    /// Check whether a blackboard exists (= `_meta` present).
    Exists {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
    },
    /// Capture the current bb state as a snapshot (`tasty.bb.<name>.snapshots.<id>`).
    Snapshot {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Snapshot id.
        #[arg(long)]
        snapshot_id: String,
    },
    /// Read a snapshot JSON.
    SnapshotGet {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Snapshot id.
        #[arg(long)]
        snapshot_id: String,
    },
    /// List snapshot ids for a bb.
    SnapshotList {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
    },
    /// Delete a snapshot.
    SnapshotDelete {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Snapshot id.
        #[arg(long)]
        snapshot_id: String,
    },
    /// Restore bb fields from a snapshot (replaces current fields).
    ///
    /// Every current field is deleted first, so a field added after the snapshot
    /// was taken is gone, not merged. `_meta` is the exception: an existing one
    /// is left alone and only a missing one is rebuilt from the snapshot, so
    /// restoring does not bring back the schema the board had at capture time.
    SnapshotRestore {
        /// Workspace id that owns the blackboard.
        #[arg(long)]
        workspace: u32,
        /// Blackboard name.
        #[arg(long)]
        name: String,
        /// Snapshot id.
        #[arg(long)]
        snapshot_id: String,
    },
}
