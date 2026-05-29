//! `tasty preset` / `tasty file-handler` / `tasty script` CLI → JsonRpcRequest 매핑.
//!
//! `read_json_file_or_stdin` 는 preset save 의 file 인자 처리에 사용.

use crate::cli::commands::{FileHandlerCommands, PresetCommands, ScriptCommands};

pub(super) fn file_handler_command_to_method_params(
    command: &FileHandlerCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        FileHandlerCommands::Reload => ("file_handler.reload", serde_json::Value::Null),
    }
}

pub(super) fn script_command_to_method_params(
    command: &ScriptCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        ScriptCommands::Reload => ("script.reload", serde_json::Value::Null),
    }
}

/// Read --file (or "-" for stdin) and parse as JSON.
pub(super) fn read_json_file_or_stdin(path: &str) -> Result<serde_json::Value, String> {
    use std::io::Read;
    let raw = if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("stdin read failed: {e}"))?;
        buf
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("file read failed: {e}"))?
    };
    serde_json::from_str(&raw).map_err(|e| format!("invalid JSON: {e}"))
}

pub(super) fn preset_command_to_method_params(
    command: &PresetCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        PresetCommands::List { kind } => ("preset.list", serde_json::json!({ "kind": kind })),
        PresetCommands::Get { kind, name } => (
            "preset.get",
            serde_json::json!({ "kind": kind, "name": name }),
        ),
        PresetCommands::Save {
            kind,
            name,
            file,
            overwrite,
        } => {
            let data = match read_json_file_or_stdin(file) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            (
                "preset.save",
                serde_json::json!({
                    "kind": kind,
                    "name": name,
                    "data": data,
                    "overwrite": overwrite,
                }),
            )
        }
        PresetCommands::Delete { kind, name } => (
            "preset.delete",
            serde_json::json!({ "kind": kind, "name": name }),
        ),
        PresetCommands::Rename { kind, from, to } => (
            "preset.rename",
            serde_json::json!({ "kind": kind, "from": from, "to": to }),
        ),
        PresetCommands::Capture {
            kind,
            source_id,
            name,
        } => (
            "preset.capture",
            serde_json::json!({
                "kind": kind,
                "source_id": source_id,
                "name": name,
            }),
        ),
        PresetCommands::Apply {
            kind,
            name,
            target_pane,
            target_workspace,
        } => (
            "preset.apply",
            serde_json::json!({
                "kind": kind,
                "name": name,
                "target_pane_id": target_pane,
                "target_workspace_id": target_workspace,
            }),
        ),
    }
}
