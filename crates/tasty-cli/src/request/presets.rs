//! `tasty preset` / `tasty file-handler` CLI → JsonRpcRequest 매핑.
//!
//! `read_json_file_or_stdin` 는 preset save 의 file 인자 처리에 사용.

use crate::commands::{
    CompletionStrategyCommands, FileHandlerCommands, HookHandlerCommands, PresetCommands,
};

pub(super) fn file_handler_command_to_method_params(
    command: &FileHandlerCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        FileHandlerCommands::Reload => ("file_handler.reload", serde_json::Value::Null),
        FileHandlerCommands::Dispatch {
            path,
            depth,
            origin_surface,
            ignore_size_limit,
        } => (
            "file_handler.dispatch",
            serde_json::json!({
                "path": path,
                "depth": depth,
                "origin_surface_id": origin_surface,
                "ignore_size_limit": ignore_size_limit,
            }),
        ),
    }
}

pub(super) fn completion_strategy_command_to_method_params(
    command: &CompletionStrategyCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        CompletionStrategyCommands::List => ("completion_strategy.list", serde_json::json!({})),
    }
}

pub(super) fn hook_handler_command_to_method_params(
    command: &HookHandlerCommands,
) -> (&'static str, serde_json::Value) {
    use HookHandlerCommands as H;
    match command {
        H::List => ("hook_handler.list", serde_json::json!({})),
        H::Reload => ("hook_handler.reload", serde_json::Value::Null),
        H::Dispatch {
            id,
            body,
            header,
            query,
        } => {
            // body/header/query 는 JSON 문자열 → Value 파싱(서버가 치환 컨텍스트로 사용).
            let parse = |s: &Option<String>| {
                s.as_deref()
                    .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
            };
            (
                "hook_handler.dispatch",
                serde_json::json!({
                    "id": id,
                    "body": parse(body),
                    "headers": parse(header),
                    "query": parse(query),
                }),
            )
        }
    }
}

/// Read --file (or "-" for stdin) and parse as JSON.
pub(super) fn read_json_file_or_stdin(path: &str) -> Result<serde_json::Value, String> {
    use std::io::Read;
    let raw = if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| tasty_i18n::t_fmt("cli.preset.stdin_read_failed", &e.to_string()))?;
        buf
    } else {
        std::fs::read_to_string(path)
            .map_err(|e| tasty_i18n::t_fmt("cli.preset.file_read_failed", &e.to_string()))?
    };
    serde_json::from_str(&raw).map_err(|e| tasty_i18n::t_fmt("cli.preset.not_json", &e.to_string()))
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
                    eprintln!("{}", tasty_i18n::t_fmt("cli.preset.save_read_failed", &e));
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
