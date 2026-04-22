use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::model::SplitDirection;
use crate::state::{AppState, ClaudeChildEntry};

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

    let ws_idx = match state.add_workspace_background(None, crate::model::SurfaceType::Terminal) {
        Ok(idx) => idx,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    state.engine.workspaces[ws_idx].name = workspace_name.to_string();

    // Get the surface ID of the newly created workspace's terminal
    let surface_id = {
        let ws = &state.engine.workspaces[ws_idx];
        let pane_id = ws.focused_pane;
        ws.pane_layout()
            .find_pane(pane_id)
            .and_then(|pane| pane.tabs.get(pane.active_tab))
            .and_then(|tab| tab.focused_surface_id())
    };

    if let Some(sid) = surface_id {
        if let Some(dir) = directory {
            if let Some(terminal) = state.find_terminal_by_id_mut(sid) {
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
        if let Some(terminal) = state.find_terminal_by_id_mut(sid) {
            terminal.send_key(&format!("{}\r", cmd));
        }
    }

    let ws_id = state.engine.workspaces[ws_idx].id;
    JsonRpcResponse::success(
        id,
        json!({
            "workspace_id": ws_id,
            "workspace_name": workspace_name,
            "surface_id": surface_id,
        }),
    )
}

pub(crate) fn handle_claude_spawn(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let workspace_param = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(ws) => ws.to_string(),
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing required '--workspace' parameter");
        }
    };

    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .map(String::from);
    let nickname = params
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let parent_surface_id = super::caller_surface_id(params).unwrap_or(0);
    if parent_surface_id == 0 {
        return JsonRpcResponse::invalid_params(
            id,
            "Cannot determine parent surface. Set TASTY_SURFACE_ID.",
        );
    }

    spawn_in_workspace(
        state,
        id,
        parent_surface_id,
        &workspace_param,
        cwd,
        role,
        nickname,
        prompt,
    )
}

/// Workspace-based spawn: auto-manage spawn pane and 2×2 grid placement.
fn spawn_in_workspace(
    state: &mut AppState,
    id: serde_json::Value,
    parent_surface_id: u32,
    ws_target: &str,
    cwd: Option<String>,
    role: Option<String>,
    nickname: Option<String>,
    prompt: Option<String>,
) -> JsonRpcResponse {
    // Resolve workspace by ID or name
    let ws_idx = resolve_workspace(state, ws_target);
    let ws_idx = match ws_idx {
        Some(idx) => idx,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Workspace '{}' not found", ws_target),
            );
        }
    };
    let ws_id = state.engine.workspaces[ws_idx].id;

    // Save and restore focus
    let saved_workspace = state.active_workspace;
    let saved_pane = state.engine.workspaces[saved_workspace].focused_pane;

    // Check if spawn pane exists and is still valid
    let spawn_pane_key = (parent_surface_id, ws_id);
    let existing_spawn_pane = state
        .engine
        .claude
        .spawn_panes
        .get(&spawn_pane_key)
        .copied();
    let spawn_pane_id = match existing_spawn_pane {
        Some(pid) if pane_exists_in_workspace(state, ws_idx, pid) => pid,
        _ => {
            // Create spawn pane: pane-level vertical split in the workspace
            let any_pane_id = state.engine.workspaces[ws_idx].focused_pane;
            match state.split_pane_targeted(
                Some(any_pane_id),
                SplitDirection::Vertical,
                None,
                crate::model::SurfaceType::Terminal,
            ) {
                Ok((new_pane_id, _new_surface_id)) => {
                    state
                        .engine
                        .claude
                        .spawn_panes
                        .insert(spawn_pane_key, new_pane_id);
                    new_pane_id
                }
                Err(e) => {
                    return JsonRpcResponse::internal_error(id, e.to_string());
                }
            }
        }
    };

    // Now find the best placement within the spawn pane
    let child_surface_id =
        match find_and_spawn_in_pane(state, ws_idx, spawn_pane_id, spawn_pane_key) {
            Ok(sid) => sid,
            Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
        };

    let child_index = state.next_child_index(parent_surface_id);
    let entry = ClaudeChildEntry {
        child_surface_id,
        index: child_index,
        cwd: cwd.clone(),
        role: role.clone(),
        nickname: nickname.clone(),
    };
    state.register_child(parent_surface_id, entry);

    start_claude_in_surface(state, child_surface_id, cwd.as_deref(), prompt.as_deref());

    // Restore focus
    state.active_workspace = saved_workspace;
    state.engine.workspaces[saved_workspace].focused_pane = saved_pane;

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": child_surface_id,
            "child_index": child_index,
            "parent_surface_id": parent_surface_id,
            "spawn_pane_id": spawn_pane_id,
            "workspace_id": ws_id,
        }),
    )
}

/// Find the best slot in a spawn pane and create a new terminal surface there.
/// Implements the 2×2 grid algorithm: 1→2→3→4→new tab→repeat.
fn find_and_spawn_in_pane(
    state: &mut AppState,
    ws_idx: usize,
    spawn_pane_id: u32,
    _spawn_pane_key: (u32, u32),
) -> anyhow::Result<u32> {
    let pane = state.engine.workspaces[ws_idx]
        .pane_layout()
        .find_pane(spawn_pane_id)
        .ok_or_else(|| anyhow::anyhow!("spawn pane {} not found", spawn_pane_id))?;

    // Find a tab with < 4 surfaces, or use the first surface of the initial tab
    let mut target_tab_index = None;
    let mut target_surface_count = 0;
    let mut target_surface_ids: Vec<u32> = Vec::new();

    for (i, tab) in pane.tabs.iter().enumerate() {
        let surface_ids = tab.all_surface_ids();
        let count = surface_ids.len();
        if count < 4 {
            target_tab_index = Some(i);
            target_surface_count = count;
            target_surface_ids = surface_ids;
            break;
        }
    }

    if let Some(tab_idx) = target_tab_index {
        // We have a tab with room — determine split direction and target
        let (target_sid, direction) = pick_split_target(target_surface_count, &target_surface_ids);

        // If target_surface_count == 0, the initial surface is in the pane already
        // Use surface-level split
        let new_surface_id = state.engine.next_ids.next_surface();
        let cols = state.engine.default_cols;
        let rows = state.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&state.engine.settings);
        let waker = state.engine.make_waker(new_surface_id);
        let terminal = tasty_terminal::Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            sh.shell_ref(),
            &sh.args_ref(),
            new_surface_id,
            waker,
            None,
        )?;
        let new_surface: Box<dyn crate::model::Surface> = Box::new(crate::model::TerminalSurface {
            id: new_surface_id,
            terminal,
            deferred_spawn: None,
        });

        let ws = &mut state.engine.workspaces[ws_idx];
        let pane = ws
            .pane_layout_mut()
            .find_pane_mut(spawn_pane_id)
            .ok_or_else(|| anyhow::anyhow!("spawn pane {} not found", spawn_pane_id))?;

        // Determine if we're splitting the first surface (initial spawn pane surface)
        if target_surface_count <= 1 {
            // The spawn pane was just created with 1 surface — split it
            let first_sid = pane.tabs[tab_idx].surface().all_surface_ids();
            if let Some(&sid) = first_sid.first() {
                pane.split_surface_by_id_with_surface(sid, direction, new_surface)?;
            }
        } else {
            pane.split_surface_by_id_with_surface(target_sid, direction, new_surface)?;
        }

        state.send_fast_init(new_surface_id);
        Ok(new_surface_id)
    } else {
        // All tabs are full (4 surfaces each) — create a new tab
        let new_tab_id = state.engine.next_ids.next_tab();
        let new_surface_id = state.engine.next_ids.next_surface();
        let cols = state.engine.default_cols;
        let rows = state.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&state.engine.settings);
        let waker = state.engine.make_waker(new_surface_id);

        let ws = &mut state.engine.workspaces[ws_idx];
        let pane = ws
            .pane_layout_mut()
            .find_pane_mut(spawn_pane_id)
            .ok_or_else(|| anyhow::anyhow!("spawn pane {} not found", spawn_pane_id))?;
        pane.add_tab_background_with_shell(
            new_tab_id,
            new_surface_id,
            cols,
            rows,
            sh.shell_ref(),
            &sh.args_ref(),
            waker,
            None,
        )?;

        state.send_fast_init(new_surface_id);
        Ok(new_surface_id)
    }
}

/// Determine which surface to split and in which direction, based on current count.
/// Returns (target_surface_id_to_split, direction).
///
/// Layout progression:
/// - 1 surface → split vertically (left|right) → target: the sole surface
/// - 2 surfaces (left, right) → split left horizontally (top-left, bottom-left | right)
/// - 3 surfaces (TL, BL, R) → split right horizontally (TL, BL | TR, BR)
fn pick_split_target(count: usize, surface_ids: &[u32]) -> (u32, SplitDirection) {
    match count {
        0 | 1 => {
            // Split the single surface vertically (creates left|right)
            let sid = surface_ids.first().copied().unwrap_or(0);
            (sid, SplitDirection::Vertical)
        }
        2 => {
            // Split the first (left) surface horizontally
            let sid = surface_ids[0];
            (sid, SplitDirection::Horizontal)
        }
        3 => {
            // Split the second (right) surface horizontally
            // In the layout: [left-top, left-bottom, right]
            // surface_ids[2] is the right surface
            let sid = surface_ids[2];
            (sid, SplitDirection::Horizontal)
        }
        _ => {
            // Should not reach here (caller checks < 4), but fallback
            let sid = surface_ids.last().copied().unwrap_or(0);
            (sid, SplitDirection::Vertical)
        }
    }
}

/// Resolve workspace by ID (numeric string) or name.
fn resolve_workspace(state: &AppState, target: &str) -> Option<usize> {
    // Try as numeric ID first
    if let Ok(ws_id) = target.parse::<u32>() {
        return state.engine.workspaces.iter().position(|ws| ws.id == ws_id);
    }
    // Try as name
    state
        .engine
        .workspaces
        .iter()
        .position(|ws| ws.name == target)
}

/// Check if a pane exists in a specific workspace.
fn pane_exists_in_workspace(state: &AppState, ws_idx: usize, pane_id: u32) -> bool {
    state.engine.workspaces[ws_idx]
        .pane_layout()
        .find_pane(pane_id)
        .is_some()
}

/// Start claude in a child surface: cd to cwd, then run claude with optional prompt.
fn start_claude_in_surface(
    state: &mut AppState,
    surface_id: u32,
    cwd: Option<&str>,
    prompt: Option<&str>,
) {
    if let Some(dir) = cwd {
        if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
            let normalized = dir.replace('\\', "/");
            let escaped = shell_escape::escape(normalized.into());
            terminal.send_key(&format!("cd {}\r", escaped));
        }
    }

    if let Some(p) = prompt {
        let prompt_path = std::env::temp_dir().join(format!("tasty-prompt-{}.txt", surface_id));
        if let Err(e) = std::fs::write(&prompt_path, p) {
            tracing::warn!("Failed to write prompt file: {e}");
        }
        if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
            terminal.send_key(&format!("claude \"$(cat '{}')\"\r", prompt_path.display()));
        }
    } else if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key("claude\r");
    }
}

pub(crate) fn handle_claude_children(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let parent_surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let children: Vec<_> = state
        .children_of(parent_surface_id)
        .iter()
        .map(|c| {
            let mut entry = json!({
                "child_surface_id": c.child_surface_id,
                "index": c.index,
                "cwd": c.cwd,
                "role": c.role,
                "nickname": c.nickname,
                "state": state.claude_state_of(c.child_surface_id),
            });
            if let Some(terminal) = state.find_terminal_by_id(c.child_surface_id) {
                if let Some(fg) = terminal.foreground_process_info() {
                    entry["foreground_process"] = json!(fg.name);
                    entry["foreground_pid"] = json!(fg.pid);
                }
            }
            entry
        })
        .collect();

    JsonRpcResponse::success(id, json!(children))
}

pub(crate) fn handle_claude_parent(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let child_surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    match state.parent_of(child_surface_id) {
        Some(parent_id) => {
            let status = if state.engine.claude.closed_parents.contains(&parent_id) {
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
    let parent_surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let child_index = match params.get("child_index").and_then(|v| v.as_u64()) {
        Some(idx) => idx as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'child_index' parameter"),
    };

    let child_surface_id = match state
        .children_of(parent_surface_id)
        .iter()
        .find(|c| c.index == child_index)
    {
        Some(c) => c.child_surface_id,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Child index {} not found for parent {}",
                    child_index, parent_surface_id
                ),
            );
        }
    };

    let pane_id = match state.find_pane_for_surface(child_surface_id) {
        Some(pid) => pid,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Surface {} not found", child_surface_id),
            );
        }
    };

    let removed = state.close_pane_by_id(pane_id);
    if removed {
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
    let parent_surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let child_index = match params.get("child_index").and_then(|v| v.as_u64()) {
        Some(idx) => idx as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'child_index' parameter"),
    };

    let child_surface_id = match state
        .children_of(parent_surface_id)
        .iter()
        .find(|c| c.index == child_index)
    {
        Some(c) => c.child_surface_id,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Child index {} not found for parent {}",
                    child_index, parent_surface_id
                ),
            );
        }
    };

    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .map(String::from);
    let nickname = params
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Create a new terminal in the same surface (layout unchanged).
    // The old terminal is dropped, sending SIGHUP to its PTY process.
    let cols = state.engine.default_cols;
    let rows = state.engine.default_rows;
    let sh = crate::engine_state::ShellConfig::from_settings(&state.engine.settings);
    let waker = state.engine.make_waker(child_surface_id);
    let new_terminal = match tasty_terminal::Terminal::new_with_shell_args_cwd(
        cols,
        rows,
        sh.shell_ref(),
        &sh.args_ref(),
        child_surface_id,
        waker,
        None,
    ) {
        Ok(t) => t,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    if let Err(e) = state.engine.replace_terminal_by_id(child_surface_id, new_terminal) {
        return JsonRpcResponse::internal_error(id, e.to_string());
    }

    // Update child entry metadata (cwd, role, nickname)
    if let Some(children) = state.engine.claude.parent_children.get_mut(&parent_surface_id) {
        if let Some(entry) = children.iter_mut().find(|c| c.index == child_index) {
            if cwd.is_some() {
                entry.cwd = cwd.clone();
            }
            if role.is_some() {
                entry.role = role.clone();
            }
            if nickname.is_some() {
                entry.nickname = nickname.clone();
            }
        }
    }

    // Start claude in the respawned surface
    start_claude_in_surface(state, child_surface_id, cwd.as_deref(), prompt.as_deref());

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": child_surface_id,
            "child_index": child_index,
            "parent_surface_id": parent_surface_id,
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
        .map(|v| v as u32);

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
        .map(|v| v as u32);

    let surface_id = match surface_id {
        Some(sid) => sid,
        None => return JsonRpcResponse::internal_error(id, "No focused surface".to_string()),
    };

    let needs_input = match params.get("needs_input").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'needs_input' parameter (bool)");
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
    let parent_surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };

    let role_filter = params
        .get("role")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

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

/// Send a message to a Claude Code instance with guaranteed submission.
///
/// Claude Code's handleEnter logic:
/// - `\` before Enter → newline (not submit)
/// - Enter alone → submit
///
/// This handler converts multi-line messages into the PTY sequence that
/// Claude Code interprets correctly: each line break becomes `\` + `\r`
/// (backslash+Enter = newline insertion), and the final `\r` triggers submission.
pub(crate) fn handle_claude_tell(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let message = match params.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'message' parameter"),
    };

    // Build the PTY sequence:
    // - Split by \n
    // - Join with \<CR> (backslash + carriage return = newline in Claude Code)
    // - End with <CR> (carriage return = submit)
    // - If last line ends with \, append a space to prevent it from escaping the final CR
    let lines: Vec<&str> = message.split('\n').collect();
    let mut pty_text = String::new();
    for (i, line) in lines.iter().enumerate() {
        pty_text.push_str(line);
        if i < lines.len() - 1 {
            // Not the last line: backslash + CR = newline in Claude Code
            pty_text.push('\\');
            pty_text.push('\r');
        }
    }
    // If the text ends with \, add a space so the final \r is treated as submit
    if pty_text.ends_with('\\') {
        pty_text.push(' ');
    }
    // Final CR = submit
    pty_text.push('\r');

    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&pty_text);
        JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

pub(crate) fn handle_claude_wait(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let parent_surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let child_index = match params.get("child_index").and_then(|v| v.as_u64()) {
        Some(idx) => idx as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'child_index' parameter"),
    };

    let child_surface_id = match state
        .children_of(parent_surface_id)
        .iter()
        .find(|c| c.index == child_index)
    {
        Some(c) => c.child_surface_id,
        None => return JsonRpcResponse::success(id, json!({ "state": "exited" })),
    };

    let exists = state.find_pane_for_surface(child_surface_id).is_some();
    if !exists {
        return JsonRpcResponse::success(id, json!({ "state": "exited" }));
    }

    let claude_state = state.claude_state_of(child_surface_id);
    JsonRpcResponse::success(id, json!({ "state": claude_state }))
}
