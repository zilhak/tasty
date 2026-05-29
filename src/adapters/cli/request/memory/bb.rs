use crate::cli::commands::MemoryBbCommands;

use super::read_value_arg;

pub(super) fn memory_bb_command_to_method_params(
    command: &MemoryBbCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryBbCommands::*;
    match command {
        Create {
            workspace,
            name,
            schema,
        } => {
            let mut p = serde_json::json!({ "workspace_id": workspace, "name": name });
            if let Some(raw) = schema.as_deref() {
                let v: serde_json::Value = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Error: --schema is not valid JSON: {e}");
                        std::process::exit(1);
                    }
                };
                p["schema"] = v;
            }
            ("memory.bb_create", p)
        }
        Put {
            workspace,
            name,
            field,
            value,
            value_b64,
            content_type,
            cas,
        } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "field": field,
            });
            if let Some(b64) = value_b64.as_deref() {
                p["value_b64"] = serde_json::json!(b64);
            } else if let Some(v) = value.as_deref() {
                let raw = match read_value_arg(v) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: failed to read value file: {e}");
                        std::process::exit(1);
                    }
                };
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    p["value"] = parsed;
                } else {
                    p["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!("Error: 'memory bb put' requires --value or --value-b64");
                std::process::exit(1);
            }
            if let Some(ct) = content_type.as_deref() {
                p["content_type"] = serde_json::json!(ct);
            }
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.bb_put", p)
        }
        Get {
            workspace,
            name,
            field,
        } => (
            "memory.bb_get",
            serde_json::json!({ "workspace_id": workspace, "name": name, "field": field }),
        ),
        GetAll { workspace, name } => (
            "memory.bb_get_all",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        GetMeta { workspace, name } => (
            "memory.bb_get_meta",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        DeleteField {
            workspace,
            name,
            field,
            cas,
        } => {
            let mut p = serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "field": field,
            });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.bb_delete_field", p)
        }
        Delete { workspace, name } => (
            "memory.bb_delete",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        List { workspace } => (
            "memory.bb_list",
            serde_json::json!({ "workspace_id": workspace }),
        ),
        Exists { workspace, name } => (
            "memory.bb_exists",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        Snapshot {
            workspace,
            name,
            snapshot_id,
        } => (
            "memory.bb_snapshot",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
        SnapshotGet {
            workspace,
            name,
            snapshot_id,
        } => (
            "memory.bb_snapshot_get",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
        SnapshotList { workspace, name } => (
            "memory.bb_snapshot_list",
            serde_json::json!({ "workspace_id": workspace, "name": name }),
        ),
        SnapshotDelete {
            workspace,
            name,
            snapshot_id,
        } => (
            "memory.bb_snapshot_delete",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
        SnapshotRestore {
            workspace,
            name,
            snapshot_id,
        } => (
            "memory.bb_snapshot_restore",
            serde_json::json!({
                "workspace_id": workspace,
                "name": name,
                "snapshot_id": snapshot_id,
            }),
        ),
    }
}
