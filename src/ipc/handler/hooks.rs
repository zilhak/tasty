use serde_json::json;
use tasty_hooks::HookEvent;

use crate::global_hooks::HookCondition;
use crate::ipc::protocol::JsonRpcResponse;
use crate::model::SplitDirection;
use crate::state::{AppState, ClaudeChildEntry};

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
                format!("Unknown event type: '{}'. Use: process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS, claude-idle, needs-input", event_str),
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

    let hook_id = state.engine.hook_manager.add_hook(surface_id, event, command, once);
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
        .engine.hook_manager
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

    let removed = state.engine.hook_manager.remove_hook(hook_id);
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
    state.engine.workspaces[ws_idx].name = workspace_name.to_string();

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

    let ws_id = state.engine.workspaces[ws_idx].id;
    JsonRpcResponse::success(
        id,
        json!({
            "workspace_id": ws_id,
            "workspace_name": workspace_name,
        }),
    )
}

pub(crate) fn handle_claude_spawn(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    // Get parent surface_id (from params or focused)
    let parent_surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let parent_surface_id = match parent_surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::internal_error(id, "No focused surface".to_string()),
    };

    // Focus the parent surface first so split happens in the right pane
    state.focus_surface(parent_surface_id);

    let direction = match params.get("direction").and_then(|v| v.as_str()) {
        Some("horizontal") | Some("h") => SplitDirection::Horizontal,
        _ => SplitDirection::Vertical,
    };

    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params.get("role").and_then(|v| v.as_str()).map(String::from);
    let nickname = params.get("nickname").and_then(|v| v.as_str()).map(String::from);
    let prompt = params.get("prompt").and_then(|v| v.as_str()).map(String::from);

    // Split pane to create a new terminal
    let child_surface_id = match state.split_pane_get_surface(direction) {
        Ok(sid) => sid,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    // Register the parent-child relationship
    let child_index = state.next_child_index(parent_surface_id);
    let entry = ClaudeChildEntry {
        child_surface_id,
        index: child_index,
        cwd: cwd.clone(),
        role: role.clone(),
        nickname: nickname.clone(),
    };
    state.register_child(parent_surface_id, entry);

    // Send cd command if cwd provided
    if let Some(dir) = &cwd {
        if let Some(terminal) = state.find_terminal_by_id_mut(child_surface_id) {
            let normalized = dir.replace('\\', "/");
            let escaped = shell_escape::escape(normalized.into());
            terminal.send_key(&format!("cd {}\r", escaped));
        }
    }

    // Send claude command
    if let Some(terminal) = state.find_terminal_by_id_mut(child_surface_id) {
        terminal.send_key("claude\r");
    }

    // Send prompt if provided
    if let Some(p) = &prompt {
        if let Some(terminal) = state.find_terminal_by_id_mut(child_surface_id) {
            let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
            terminal.send_key(&format!("{}\r", escaped));
        }
    }

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": child_surface_id,
            "child_index": child_index,
            "parent_surface_id": parent_surface_id,
        }),
    )
}

pub(crate) fn handle_claude_children(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let parent_surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let parent_surface_id = match parent_surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::success(id, json!([])),
    };

    let children: Vec<_> = state
        .children_of(parent_surface_id)
        .iter()
        .map(|c| {
            json!({
                "child_surface_id": c.child_surface_id,
                "index": c.index,
                "cwd": c.cwd,
                "role": c.role,
                "nickname": c.nickname,
                "state": state.claude_state_of(c.child_surface_id),
            })
        })
        .collect();

    JsonRpcResponse::success(id, json!(children))
}

pub(crate) fn handle_claude_parent(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let child_surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let child_surface_id = match child_surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::invalid_params(id, "No focused surface"),
    };

    match state.parent_of(child_surface_id) {
        Some(parent_id) => {
            let status = if state.engine.claude_closed_parents.contains(&parent_id) {
                "closed"
            } else {
                "active"
            };
            JsonRpcResponse::success(
                id,
                json!({
                    "parent_surface_id": parent_id,
                    "status": status,
                }),
            )
        }
        None => JsonRpcResponse::success(
            id,
            json!({
                "parent_surface_id": null,
                "status": "none",
            }),
        ),
    }
}

pub(crate) fn handle_claude_kill(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let child_surface_id = match params.get("child_surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'child_surface_id' parameter"),
    };

    // Find the pane containing this surface
    let pane_id = match state.find_pane_for_surface(child_surface_id) {
        Some(pid) => pid,
        None => return JsonRpcResponse::invalid_params(id, format!("Surface {} not found", child_surface_id)),
    };

    // Close the pane
    let removed = state.close_pane_by_id(pane_id);
    if removed {
        // unregister_child and mark_parent_closed are called inside close_pane_by_id indirectly
        // but close_pane_by_id doesn't do claude cleanup, so do it here
        state.unregister_child(child_surface_id);
        state.mark_parent_closed(child_surface_id);
    }

    JsonRpcResponse::success(id, json!({ "killed": removed }))
}

pub(crate) fn handle_claude_respawn(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let child_surface_id = match params.get("child_surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'child_surface_id' parameter"),
    };

    // Get the old child entry info
    let parent_id = match state.parent_of(child_surface_id) {
        Some(pid) => pid,
        None => return JsonRpcResponse::invalid_params(id, format!("Surface {} is not a claude child", child_surface_id)),
    };

    let old_index = state.children_of(parent_id)
        .iter()
        .find(|c| c.child_surface_id == child_surface_id)
        .map(|c| c.index)
        .unwrap_or(0);

    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params.get("role").and_then(|v| v.as_str()).map(String::from);
    let nickname = params.get("nickname").and_then(|v| v.as_str()).map(String::from);
    let prompt = params.get("prompt").and_then(|v| v.as_str()).map(String::from);

    // Kill old child
    let pane_id = state.find_pane_for_surface(child_surface_id);
    if let Some(pid) = pane_id {
        state.close_pane_by_id(pid);
        state.unregister_child(child_surface_id);
        state.mark_parent_closed(child_surface_id);
    }

    // Focus parent for the new split
    state.focus_surface(parent_id);

    // Spawn new child
    let new_surface_id = match state.split_pane_get_surface(SplitDirection::Vertical) {
        Ok(sid) => sid,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    // Register with same index
    let entry = ClaudeChildEntry {
        child_surface_id: new_surface_id,
        index: old_index,
        cwd: cwd.clone(),
        role: role.clone(),
        nickname: nickname.clone(),
    };
    state.register_child(parent_id, entry);

    // Send cd command if cwd provided
    if let Some(dir) = &cwd {
        if let Some(terminal) = state.find_terminal_by_id_mut(new_surface_id) {
            let normalized = dir.replace('\\', "/");
            let escaped = shell_escape::escape(normalized.into());
            terminal.send_key(&format!("cd {}\r", escaped));
        }
    }

    // Send claude command
    if let Some(terminal) = state.find_terminal_by_id_mut(new_surface_id) {
        terminal.send_key("claude\r");
    }

    // Send prompt if provided
    if let Some(p) = &prompt {
        if let Some(terminal) = state.find_terminal_by_id_mut(new_surface_id) {
            let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
            terminal.send_key(&format!("{}\r", escaped));
        }
    }

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": new_surface_id,
            "child_index": old_index,
            "parent_surface_id": parent_id,
        }),
    )
}

pub(crate) fn handle_claude_set_idle_state(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let surface_id = match surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::internal_error(id, "No focused surface".to_string()),
    };

    let idle = match params.get("idle").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'idle' parameter (bool)"),
    };

    state.set_claude_idle(surface_id, idle);
    JsonRpcResponse::success(id, json!({ "ok": true }))
}

pub(crate) fn handle_claude_set_needs_input(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let surface_id = match surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::internal_error(id, "No focused surface".to_string()),
    };

    let needs_input = match params.get("needs_input").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'needs_input' parameter (bool)")
        }
    };

    state.set_claude_needs_input(surface_id, needs_input);
    JsonRpcResponse::success(id, json!({ "ok": true }))
}

pub(crate) fn handle_claude_broadcast(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let parent_surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let parent_surface_id = match parent_surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::internal_error(id, "No focused surface".to_string()),
    };

    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };

    let role_filter = params.get("role").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Collect child surface IDs to send to (with optional role filter)
    let child_ids: Vec<u32> = state
        .children_of(parent_surface_id)
        .iter()
        .filter(|c| {
            if let Some(ref role) = role_filter {
                c.role.as_deref() == Some(role.as_str())
            } else {
                true
            }
        })
        .map(|c| c.child_surface_id)
        .collect();

    let mut sent_count = 0usize;
    for child_id in &child_ids {
        if let Some(terminal) = state.find_terminal_by_id_mut(*child_id) {
            terminal.send_key(&text);
            sent_count += 1;
        }
    }

    JsonRpcResponse::success(
        id,
        json!({
            "sent_count": sent_count,
            "children": child_ids,
        }),
    )
}

pub(crate) fn handle_claude_wait(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let child_surface_id = match params.get("child_surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'child_surface_id' parameter"),
    };

    // Check if the surface exists
    let exists = state.find_pane_for_surface(child_surface_id).is_some();
    if !exists {
        return JsonRpcResponse::success(id, json!({ "state": "exited" }));
    }

    let claude_state = state.claude_state_of(child_surface_id);
    JsonRpcResponse::success(id, json!({ "state": claude_state }))
}

pub(crate) fn handle_global_hook_set(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let condition_str = match params.get("condition").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'condition' parameter"),
    };

    let condition = match HookCondition::parse(condition_str) {
        Some(c) => c,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Invalid condition '{}'. Use: interval:SECS, once:SECS, file:/path",
                    condition_str
                ),
            )
        }
    };

    let command = match params.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'command' parameter"),
    };

    let label = params
        .get("label")
        .and_then(|v| v.as_str())
        .map(String::from);

    let hook_id = state.engine.global_hook_manager.add(condition, command, label);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

pub(crate) fn handle_global_hook_list(
    state: &AppState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let hooks: Vec<_> = state
        .engine.global_hook_manager
        .list()
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "condition": h.condition.to_display_string(),
                "command": h.command,
                "label": h.label,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(hooks))
}

pub(crate) fn handle_global_hook_unset(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
        Some(h) => h as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
    };

    let removed = state.engine.global_hook_manager.remove(hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

pub(crate) fn handle_surface_fire_hook(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .or_else(|| state.focused_surface_id());

    let surface_id = match surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::internal_error(id, "No focused surface".to_string()),
    };

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Unknown event type: '{}'", event_str),
            )
        }
    };

    let fired = state.engine.hook_manager.check_and_fire(surface_id, &[event]);
    JsonRpcResponse::success(id, json!({ "fired": fired.len() }))
}
