use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub(crate) fn handle_screen_text(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
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
    let text = engine
        .find_terminal_by_id(surface_id)
        .map(|t| match lines {
            Some(n) => t.screen_text_lines(n),
            None => t.screen_text(),
        })
        .unwrap_or_default();
    JsonRpcResponse::success(id, json!({ "text": text, "surface_id": surface_id }))
}

pub(crate) fn handle_cursor_position(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if let Some(terminal) = engine.find_terminal_by_id(surface_id) {
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
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let (name, pid) = engine
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
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
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

    let cols = engine.default_cols;
    let rows = engine.default_rows;
    let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
    let waker = engine.make_waker(surface_id);
    let new_terminal = match tasty_terminal::Terminal::new(
        tasty_terminal::TerminalConfig {
            cols,
            rows,
            shell: sh.shell_ref(),
            args: &sh.args_ref(),
            surface_id,
            working_dir: cwd.as_deref(),
            initial_input: None,
        },
        waker,
    ) {
        Ok(t) => t,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    match engine.replace_terminal_by_id(surface_id, new_terminal) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id })),
        Err(e) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

/// surface_id를 포함하는 pane을 찾아 `pane_id`와 존재 여부를 반환.
/// 플러그인이 자기 자식 surface를 죽이거나 wait할 때, 호스트 트리에 여전히
/// 살아있는지 확인하기 위해 사용한다.
pub(crate) fn handle_surface_locate(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let pane_id = engine.find_pane_for_surface(surface_id);
    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "pane_id": pane_id,
            "exists": pane_id.is_some(),
        }),
    )
}
