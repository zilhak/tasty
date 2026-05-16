use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

/// Parse key combo strings like "ctrl+c", "ctrl+shift+c", "alt+x" into terminal bytes.
///
/// 왼쪽부터 `ctrl+`/`shift+`/`alt+` 프리픽스를 벗겨내고 남은 부분을 키 토큰으로 본다.
/// `split('+')`을 쓰지 않는 이유는 `"ctrl++"`(Ctrl+`+`)처럼 키와 구분자가 충돌하는
/// 경우를 올바르게 해석하기 위함. `"plus"`/`"minus"`/`"equals"` 같은 심볼 이름도
/// 허용한다.
fn parse_key_combo(input: &str) -> Option<Vec<u8>> {
    if input.is_empty() {
        return None;
    }

    let mut has_ctrl = false;
    // shift-only 시퀀스는 현재 미지원 — 프리픽스만 떼어내고 modifier로는 사용하지 않는다.
    let mut _has_shift = false;
    let mut has_alt = false;
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        if !has_ctrl && lower.starts_with("ctrl+") {
            has_ctrl = true;
            rest = &rest[5..];
        } else if !_has_shift && lower.starts_with("shift+") {
            _has_shift = true;
            rest = &rest[6..];
        } else if !has_alt && lower.starts_with("alt+") {
            has_alt = true;
            rest = &rest[4..];
        } else {
            break;
        }
    }

    if rest.is_empty() {
        return None;
    }
    if matches!(rest.to_ascii_lowercase().as_str(), "ctrl" | "shift" | "alt") {
        return None;
    }
    if !has_ctrl && !has_alt {
        return None;
    }

    // 심볼 이름을 단일 문자로 정규화.
    let key: &str = match rest.to_ascii_lowercase().as_str() {
        "plus" => "+",
        "minus" => "-",
        "equals" => "=",
        _ => rest,
    };

    let mut bytes = Vec::new();

    if has_ctrl && key.chars().count() == 1 {
        let ch = key.chars().next()?.to_ascii_lowercase();
        if ch >= 'a' && ch <= 'z' {
            if has_alt {
                bytes.push(0x1B);
            }
            bytes.push(ch as u8 - b'a' + 1);
            return Some(bytes);
        } else if ch == '[' {
            bytes.push(0x1B);
            return Some(bytes);
        } else if ch == '\\' {
            bytes.push(0x1C);
            return Some(bytes);
        } else if ch == ']' {
            bytes.push(0x1D);
            return Some(bytes);
        }
    }

    if has_alt && !has_ctrl && key.chars().count() == 1 {
        bytes.push(0x1B);
        bytes.extend_from_slice(key.as_bytes());
        return Some(bytes);
    }

    None
}

use super::require_surface_id;

pub(crate) fn handle_surface_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let mut surfaces = Vec::new();
    for ws in &state.engine.workspaces {
        for &pane_id in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                    collect_tab_surface_info(state, tab, pane_id, ws.id, tab_idx, &mut surfaces);
                }
            }
        }
    }
    JsonRpcResponse::success(id, json!(surfaces))
}

fn collect_tab_surface_info(
    state: &AppState,
    tab: &crate::model::Tab,
    pane_id: u32,
    workspace_id: u32,
    tab_idx: usize,
    out: &mut Vec<serde_json::Value>,
) {
    if tab.is_split() {
        // Split tab: iterate through the layout
        collect_surface_layout_info(state, tab.layout(), pane_id, workspace_id, tab_idx, out);
    } else {
        // Single surface tab
        let surface = tab.surface();
        if let Some(node) = surface.as_terminal_surface() {
            let mut entry = json!({
                "id": node.id,
                "pane_id": pane_id,
                "workspace_id": workspace_id,
                "tab_index": tab_idx,
                "type": "Terminal",
                "cols": node.terminal.cols(),
                "rows": node.terminal.rows(),
                "busy": state.is_surface_busy(node.id),
                "pty_ready": true,
            });
            if let Some(fg) = node.terminal.foreground_process_info() {
                entry["foreground_process"] = json!(fg.name);
                entry["foreground_pid"] = json!(fg.pid);
            }
            out.push(entry);
        } else if let Some(id) = surface.surface_id() {
            // Non-terminal surfaces (Markdown, Explorer, Html, Empty)
            // EmptySurface placeholders backing a deferred terminal still
            // expose `type: "Terminal"` so agents can target them like any
            // other terminal — they just report `pty_ready: false` until the
            // PTY is spawned (auto on send, manual via `tasty wake`).
            let deferred = tab.is_surface_deferred(id);
            let mut entry = json!({
                "id": id,
                "pane_id": pane_id,
                "workspace_id": workspace_id,
                "tab_index": tab_idx,
                "type": if deferred { "Terminal" } else { surface.type_name() },
                "busy": false,
            });
            if deferred {
                entry["pty_ready"] = json!(false);
            }
            out.push(entry);
        }
    }
}

fn collect_surface_layout_info(
    state: &AppState,
    layout: &crate::model::SurfaceLayout,
    pane_id: u32,
    workspace_id: u32,
    tab_idx: usize,
    out: &mut Vec<serde_json::Value>,
) {
    match layout {
        crate::model::SurfaceLayout::Leaf(surface) => {
            let id = surface.surface_id().unwrap_or(0);
            let mut entry = json!({
                "id": id,
                "pane_id": pane_id,
                "workspace_id": workspace_id,
                "tab_index": tab_idx,
                "type": surface.type_name(),
                "busy": state.is_surface_busy(id),
            });
            if let Some(terminal) = surface.focused_terminal() {
                entry["cols"] = json!(terminal.cols());
                entry["rows"] = json!(terminal.rows());
                entry["pty_ready"] = json!(true);
                if let Some(fg) = terminal.foreground_process_info() {
                    entry["foreground_process"] = json!(fg.name);
                    entry["foreground_pid"] = json!(fg.pid);
                }
            }
            out.push(entry);
        }
        crate::model::SurfaceLayout::Split { first, second, .. } => {
            collect_surface_layout_info(state, first, pane_id, workspace_id, tab_idx, out);
            collect_surface_layout_info(state, second, pane_id, workspace_id, tab_idx, out);
        }
    }
}

pub(crate) fn handle_surface_send(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    state.engine.ensure_surface_initialized(surface_id);
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(text);
        JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

pub(crate) fn handle_surface_send_key(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };

    state.engine.ensure_surface_initialized(surface_id);

    let bytes: Vec<u8> = match key {
        "enter" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" | "esc" => b"\x1b".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => b"\x1b[15~".to_vec(),
        "f6" => b"\x1b[17~".to_vec(),
        "f7" => b"\x1b[18~".to_vec(),
        "f8" => b"\x1b[19~".to_vec(),
        "f9" => b"\x1b[20~".to_vec(),
        "f10" => b"\x1b[21~".to_vec(),
        "f11" => b"\x1b[23~".to_vec(),
        "f12" => b"\x1b[24~".to_vec(),
        other => {
            // Parse modifier+key combos like "ctrl+c", "alt+x"
            if let Some(combo_bytes) = parse_key_combo(other) {
                combo_bytes
            } else if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(other);
                return JsonRpcResponse::success(
                    id,
                    json!({ "sent": true, "surface_id": surface_id }),
                );
            } else {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("Surface {} not found", surface_id),
                );
            }
        }
    };
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_bytes(&bytes);
    }
    JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
}

/// Force-spawn the PTY of a deferred surface without sending any input.
///
/// Returns `{ "woke": true }` if this call spawned the PTY, `{ "woke": false }`
/// otherwise (already initialized or not a deferred surface). Returns
/// `invalid_params` if the surface_id refers to neither a live terminal nor a
/// deferred placeholder.
pub(crate) fn handle_surface_wake(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let was_deferred = state.engine.is_surface_deferred(surface_id);
    let woke = state.engine.ensure_surface_initialized(surface_id);
    if !woke && !was_deferred && state.engine.find_terminal_by_id(surface_id).is_none() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Surface {} not found", surface_id),
        );
    }
    JsonRpcResponse::success(
        id,
        json!({ "woke": woke, "surface_id": surface_id, "pty_ready": true }),
    )
}

pub(crate) fn handle_surface_close(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // Prevent closing the caller's own surface — use 'close self' instead
    if let Some(caller) = super::caller_surface_id(params) {
        if caller == surface_id {
            return JsonRpcResponse::invalid_params(
                id,
                "Cannot close your own surface with 'close surface'. Use 'tasty close self' instead.",
            );
        }
    }
    let kind = state.surface_kind(surface_id);
    if state.close_surface_by_id_no_snapshot(surface_id) {
        if let Some(k) = kind {
            state.enqueue_surface_closed(surface_id, k, false);
        }
        JsonRpcResponse::success(id, json!({ "closed": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({ "closed": false, "surface_id": surface_id, "reason": "surface not found" }),
        )
    }
}

/// Close the calling surface itself. Only way for a surface to close itself.
pub(crate) fn handle_surface_close_self(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let kind = state.surface_kind(surface_id);
    if state.close_surface_by_id_no_snapshot(surface_id) {
        if let Some(k) = kind {
            state.enqueue_surface_closed(surface_id, k, false);
        }
        JsonRpcResponse::success(id, json!({ "closed": true, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::success(
            id,
            json!({ "closed": false, "surface_id": surface_id, "reason": "surface not found" }),
        )
    }
}

pub(crate) fn handle_set_mark(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    state.set_mark(Some(surface_id));
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

pub(crate) fn handle_read_since_mark(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let strip_ansi = params
        .get("strip_ansi")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let text = state.read_since_mark(Some(surface_id), strip_ansi);
    JsonRpcResponse::success(id, json!({ "text": text, "surface_id": surface_id }))
}

/// `surface.parse_since_mark` — read_since_mark 결과를 `tasty-output` 빌트인
/// 파서들로 분해. `parsers` 가 생략되면 `DEFAULT_PARSER_IDS` 사용. `prompt_boundary`
/// /`exit_code` 같이 ANSI escape 자체가 의미인 파서를 쓸 수 있도록 raw 텍스트
/// (strip_ansi=false) 를 항상 입력으로 한다.
pub(crate) fn handle_parse_since_mark(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let parser_ids: Vec<String> = match params.get("parsers") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(s)) => {
            s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        }
        _ => tasty_output::DEFAULT_PARSER_IDS.iter().map(|s| s.to_string()).collect(),
    };

    let text = state.read_since_mark(Some(surface_id), false);
    let items = match tasty_output::parse_buffer(
        &text,
        parser_ids.iter().map(String::as_str),
    ) {
        Ok(v) => v,
        Err(unknown) => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("unknown parser: '{unknown}'"),
            );
        }
    };

    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "parsers": parser_ids,
            "items": items,
        }),
    )
}

pub(crate) fn handle_screen_text(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let lines = params
        .get("lines")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let text = state
        .find_terminal_by_id(surface_id)
        .map(|t| match lines {
            Some(n) => t.screen_text_lines(n),
            None => t.screen_text(),
        })
        .unwrap_or_default();
    JsonRpcResponse::success(id, json!({ "text": text, "surface_id": surface_id }))
}

pub(crate) fn handle_cursor_position(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        let (x, y) = terminal.surface().cursor_position();
        JsonRpcResponse::success(id, json!({ "x": x, "y": y, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// 터미널 PTY의 전경(foreground) 프로세스 이름/PID 조회.
/// 플러그인이 `claude` 같은 자식 프로세스가 살아있는지 판단하기 위해 사용한다.
/// 터미널이 없으면 `name`/`pid`가 모두 `null`로 반환된다.
pub(crate) fn handle_foreground_process(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let (name, pid) = state
        .find_terminal_by_id(surface_id)
        .and_then(|t| t.foreground_process_info())
        .map(|fg| (Some(fg.name.clone()), Some(fg.pid)))
        .unwrap_or((None, None));
    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "name": name,
            "pid": pid,
        }),
    )
}

/// 동일 surface_id를 유지한 채 PTY를 새 프로세스로 교체.
/// 호스트 `replace_terminal_by_id` 1:1 노출 — 기존 terminal은 drop되며 SIGHUP을
/// 보낸다. cwd가 주어지면 새 PTY의 working_dir로 지정.
///
/// 플러그인이 claude.respawn에서 사용. 그 핸들러는 본 IPC로 PTY를 갈아끼운 뒤
/// surface.send로 `claude` 명령을 재송신한다.
pub(crate) fn handle_surface_respawn_terminal(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);

    let cols = state.engine.default_cols;
    let rows = state.engine.default_rows;
    let sh = crate::engine_state::ShellConfig::from_settings(&state.engine.settings);
    let waker = state.engine.make_waker(surface_id);
    let new_terminal = match tasty_terminal::Terminal::new(
        tasty_terminal::TerminalConfig {
            cols,
            rows,
            shell: sh.shell_ref(),
            args: &sh.args_ref(),
            surface_id,
            working_dir: cwd.as_deref(),
        },
        waker,
    ) {
        Ok(t) => t,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    match state.engine.replace_terminal_by_id(surface_id, new_terminal) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id })),
        Err(e) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

/// surface_id를 포함하는 pane을 찾아 `pane_id`와 존재 여부를 반환.
/// 플러그인이 자기 자식 surface를 죽이거나 wait할 때, 호스트 트리에 여전히
/// 살아있는지 확인하기 위해 사용한다.
pub(crate) fn handle_surface_locate(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let pane_id = state.find_pane_for_surface(surface_id);
    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "pane_id": pane_id,
            "exists": pane_id.is_some(),
        }),
    )
}

pub(crate) fn handle_surface_send_combo(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let modifiers = params
        .get("modifiers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let has_ctrl = modifiers.iter().any(|m| m == "ctrl");
    let has_alt = modifiers.iter().any(|m| m == "alt");

    state.engine.ensure_surface_initialized(surface_id);

    let mut bytes_to_send: Vec<u8> = Vec::new();

    if has_ctrl && key.len() == 1 {
        let ch = key.chars().next().unwrap().to_ascii_lowercase();
        if ch >= 'a' && ch <= 'z' {
            bytes_to_send.push(ch as u8 - b'a' + 1);
        } else if ch == '[' {
            bytes_to_send.push(0x1B);
        } else if ch == '\\' {
            bytes_to_send.push(0x1C);
        } else if ch == ']' {
            bytes_to_send.push(0x1D);
        }
    } else {
        if has_alt {
            bytes_to_send.push(0x1B);
        }
        bytes_to_send.extend_from_slice(key.as_bytes());
    }

    let terminal = state.find_terminal_by_id_mut(surface_id);

    if let Some(terminal) = terminal {
        terminal.send_bytes(&bytes_to_send);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::internal_error(id, "No terminal found".to_string())
    }
}

// handle_pane_focus / handle_surface_focus removed: focus is user-only.

pub(crate) fn handle_surface_send_to(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    let surface_id = match params.get("surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'surface_id' parameter"),
    };
    state.engine.ensure_surface_initialized(surface_id);
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(text);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}
