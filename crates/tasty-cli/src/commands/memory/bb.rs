use clap::Subcommand;

/// `tasty memory bb ...` subcommands.
#[derive(Subcommand)]
pub enum MemoryBbCommands {
    /// Create a new blackboard with optional schema (JSON).
    Create {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        /// Schema JSON literal. Stored as-is; no validation performed.
        #[arg(long)]
        schema: Option<String>,
    },
    /// Write a field value.
    Put {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
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
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        field: String,
    },
    /// Read all fields of a blackboard (`_meta` excluded).
    GetAll {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Read the `_meta` entry (schema/created_by/...).
    GetMeta {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Delete a single field.
    DeleteField {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        field: String,
        #[arg(long)]
        cas: Option<u64>,
    },
    /// Delete the entire blackboard (`_meta` + all fields).
    Delete {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// List blackboard names in a workspace.
    List {
        #[arg(long)]
        workspace: u32,
    },
    /// Check whether a blackboard exists (= `_meta` present).
    Exists {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Capture the current bb state as a snapshot (`tasty.bb.<name>.snapshots.<id>`).
    Snapshot {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
    /// Read a snapshot JSON.
    SnapshotGet {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
    /// List snapshot ids for a bb.
    SnapshotList {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
    },
    /// Delete a snapshot.
    SnapshotDelete {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
    /// Restore bb fields from a snapshot (replaces current fields).
    SnapshotRestore {
        #[arg(long)]
        workspace: u32,
        #[arg(long)]
        name: String,
        #[arg(long)]
        snapshot_id: String,
    },
}
