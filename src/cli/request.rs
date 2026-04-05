use crate::ipc::protocol::JsonRpcRequest;
use super::Commands;

/// Resolve a target string for split/other commands.
/// - "this" → numeric surface ID from TASTY_SURFACE_ID env var
/// - numeric string → passed through as-is
/// - other string → passed through as-is (server resolves as nickname)
fn resolve_target(target: &str) -> String {
    if target == "this" {
        std::env::var("TASTY_SURFACE_ID").unwrap_or_else(|_| target.to_string())
    } else {
        target.to_string()
    }
}

pub fn command_to_request(command: &Commands) -> JsonRpcRequest {
    let (method, params) = match command {
        Commands::Info => ("system.info", serde_json::json!({})),
        Commands::Debug => ("debug.info", serde_json::json!({})),
        Commands::NewWindow => ("window.create", serde_json::json!({})),
        Commands::Windows => ("window.list", serde_json::json!({})),
        Commands::List => ("workspace.list", serde_json::json!({})),
        Commands::NewWorkspace { name, cwd } => (
            "workspace.create",
            serde_json::json!({ "name": name.as_deref().unwrap_or(""), "cwd": cwd }),
        ),
        Commands::SelectWorkspace { index } => (
            "workspace.select",
            serde_json::json!({ "index": index }),
        ),
        Commands::Send { text } => ("surface.send", serde_json::json!({ "text": text })),
        Commands::SendKey { key } => ("surface.send_key", serde_json::json!({ "key": key })),
        Commands::Notify { body, title } => (
            "notification.create",
            serde_json::json!({ "title": title, "body": body }),
        ),
        Commands::Notifications => ("notification.list", serde_json::json!({})),
        Commands::Tree => ("tree", serde_json::json!({})),
        Commands::Split { level, target, direction, meta, cwd } => {
            let resolved_target = target.as_deref().map(resolve_target);
            let meta_value = meta
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            (
                "split",
                serde_json::json!({
                    "level": level,
                    "target": resolved_target,
                    "direction": direction,
                    "meta": meta_value,
                    "cwd": cwd,
                }),
            )
        }
        Commands::NewTab { cwd } => ("tab.create", serde_json::json!({ "cwd": cwd })),
        Commands::CloseTab => ("tab.close", serde_json::json!({})),
        Commands::ClosePane => ("pane.close", serde_json::json!({})),
        Commands::CloseSurface => ("surface.close", serde_json::json!({})),
        Commands::Surfaces => ("surface.list", serde_json::json!({})),
        Commands::Panes => ("pane.list", serde_json::json!({})),
        Commands::SetHook {
            surface,
            event,
            command,
            once,
        } => (
            "hook.set",
            serde_json::json!({
                "surface_id": surface,
                "event": event,
                "command": command,
                "once": once,
            }),
        ),
        Commands::ListHooks { surface } => (
            "hook.list",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::UnsetHook { hook } => (
            "hook.unset",
            serde_json::json!({ "hook_id": hook }),
        ),
        Commands::SetMark { surface } => (
            "surface.set_mark",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::ReadSinceMark {
            surface,
            strip_ansi,
        } => (
            "surface.read_since_mark",
            serde_json::json!({
                "surface_id": surface,
                "strip_ansi": strip_ansi,
            }),
        ),
        Commands::Claude {
            workspace,
            directory,
            task,
        } => (
            "claude.launch",
            serde_json::json!({
                "workspace": workspace,
                "directory": directory,
                "task": task,
            }),
        ),
        Commands::ClaudeSpawn {
            direction,
            cwd,
            role,
            nickname,
            prompt,
        } => (
            "claude.spawn",
            serde_json::json!({
                "direction": direction,
                "cwd": cwd,
                "role": role,
                "nickname": nickname,
                "prompt": prompt,
            }),
        ),
        Commands::ClaudeChildren => ("claude.children", serde_json::json!({})),
        Commands::ClaudeParent => ("claude.parent", serde_json::json!({})),
        Commands::ClaudeKill { child } => (
            "claude.kill",
            serde_json::json!({ "child_surface_id": child }),
        ),
        Commands::ClaudeRespawn {
            child,
            cwd,
            role,
            nickname,
            prompt,
        } => (
            "claude.respawn",
            serde_json::json!({
                "child_surface_id": child,
                "cwd": cwd,
                "role": role,
                "nickname": nickname,
                "prompt": prompt,
            }),
        ),
        Commands::MessageSend { to, content, from } => (
            "message.send",
            serde_json::json!({
                "to_surface_id": to,
                "content": content,
                "from_surface_id": from,
            }),
        ),
        Commands::MessageRead { surface, from, peek } => (
            "message.read",
            serde_json::json!({
                "surface_id": surface,
                "from_surface_id": from,
                "peek": peek,
            }),
        ),
        Commands::MessageCount { surface } => (
            "message.count",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::MessageClear { surface } => (
            "message.clear",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::FocusDirection { direction } => (
            "focus.direction",
            serde_json::json!({ "direction": direction }),
        ),
        Commands::SurfaceMeta { action, surface, key, value } => {
            let method = match action.as_str() {
                "set" => "surface.meta_set",
                "get" => "surface.meta_get",
                "unset" => "surface.meta_unset",
                "list" => "surface.meta_list",
                _ => "surface.meta_list",
            };
            (
                method,
                serde_json::json!({
                    "surface_id": surface,
                    "key": key,
                    "value": value,
                }),
            )
        }
        Commands::ClaudeBroadcast { text, role } => (
            "claude.broadcast",
            serde_json::json!({
                "text": text,
                "role": role,
            }),
        ),
        // ClaudeHook is handled separately in run_client before reaching here
        Commands::ClaudeHook { .. } => unreachable!("ClaudeHook is handled in run_client"),
        // ClaudeWait is handled separately in run_client before reaching here
        Commands::ClaudeWait { .. } => unreachable!("ClaudeWait is handled in run_client"),
        Commands::GlobalHookSet {
            condition,
            command,
            label,
        } => (
            "global_hook.set",
            serde_json::json!({
                "condition": condition,
                "command": command,
                "label": label,
            }),
        ),
        Commands::GlobalHookList => ("global_hook.list", serde_json::json!({})),
        Commands::GlobalHookUnset { hook } => (
            "global_hook.unset",
            serde_json::json!({ "hook_id": hook }),
        ),
        Commands::IsTyping { surface } => (
            "surface.is_typing",
            serde_json::json!({ "surface_id": surface }),
        ),
        Commands::OpenMarkdown { path } => (
            "tab.open_markdown",
            serde_json::json!({ "file_path": path }),
        ),
        Commands::OpenExplorer { path } => (
            "tab.open_explorer",
            serde_json::json!({ "path": path }),
        ),
    };

    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
    }
}
