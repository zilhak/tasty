use serde_json::json;
use tasty_hooks::HookEvent;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub(crate) fn handle_hook_set(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Unknown event type: '{}'. Use: process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS", event_str),
            )
        }
    };

    let command = match params.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'command' parameter"),
    };

    let once = params
        .get("once")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let hook_id = state.hook_manager.add_hook(surface_id, event, command, once);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

pub(crate) fn handle_hook_list(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let hooks: Vec<_> = state
        .hook_manager
        .list_hooks(surface_id)
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "surface_id": h.surface_id,
                "event": h.event.to_display_string(),
                "command": h.command,
                "once": h.once,
            })
        })
        .collect();

    JsonRpcResponse::success(id, json!(hooks))
}

pub(crate) fn handle_hook_unset(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
    };

    let removed = state.hook_manager.remove_hook(hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

pub(crate) fn handle_claude_launch(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let workspace_name = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("claude");
    let directory = params.get("directory").and_then(|v| v.as_str());
    let task = params.get("task").and_then(|v| v.as_str());

    match state.add_workspace() {
        Ok(_) => {}
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    }

    let ws_idx = state.active_workspace;
    state.workspaces[ws_idx].name = workspace_name.to_string();

    if let Some(dir) = directory {
        if let Some(terminal) = state.focused_terminal_mut() {
            // Normalize backslashes to forward slashes for bash compatibility
            let normalized = dir.replace('\\', "/");
            let escaped = shell_escape::escape(normalized.into());
            terminal.send_key(&format!("cd {}\r", escaped));
        }
    }

    let mut cmd = "claude".to_string();
    if let Some(t) = task {
        let escaped = shell_escape::escape(t.into());
        cmd.push_str(&format!(" --task {}", escaped));
    }
    if let Some(terminal) = state.focused_terminal_mut() {
        terminal.send_key(&format!("{}\r", cmd));
    }

    let ws_id = state.workspaces[ws_idx].id;
    JsonRpcResponse::success(
        id,
        json!({
            "workspace_id": ws_id,
            "workspace_name": workspace_name,
        }),
    )
}
