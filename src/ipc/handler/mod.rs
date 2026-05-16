use std::borrow::Cow;

use serde_json::json;

use crate::ipc::alias;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::state::AppState;

pub mod approval;
mod clipboard;
mod hooks;
pub mod ime;
mod image;
#[cfg(target_os = "macos")]
mod input_source;
mod memory;
mod message;
mod meta;
mod notification;
mod output;
mod pane;
pub mod plugin;
#[cfg(debug_assertions)]
pub mod popup;
mod surface;
mod tab;
mod telemetry;
#[cfg(debug_assertions)]
mod tool;
mod workspace;

/// Handle a JSON-RPC request against the application state.
/// Returns a JSON-RPC response.
///
/// 라우터 구조:
/// 1. **engine 핸들러** (`route_engine_handler`): AppState UI 필드를 만지지 않는
///    핸들러 60+개. `&mut AppState`를 받지만 본문이 `state.engine`만 접근하거나
///    AppState 메서드(현재는 engine-only)만 호출한다. 단계 07에서 plugin 권한
///    게이트가 이 진입점에서 동작한다.
/// 2. **GUI 의존 핸들러** (`route_gui_handler`): UI state(popups/dialogs/active_workspace)
///    를 만져야 하는 소수 핸들러. 권한 게이트 대상 외부.
/// 3. **debug 핸들러** (`route_debug_handler`): debug build 전용. release에서는 정의 안 됨.
pub fn handle(state: &mut AppState, request: &JsonRpcRequest) -> JsonRpcResponse {
    handle_with_caller(state, request, &CallerContext::Local)
}

/// caller가 명시된 라우터 진입점. CLI/네트워크 IPC는 [`CallerContext::Local`],
/// plugin process가 호출한 명령은 [`CallerContext::Plugin`]을 전달한다.
///
/// 권한 게이트는 라우터의 가장 바깥에서 한 번만 실행된다. plugin이 호출한
/// 명령이 권한을 통과하지 못하면 `permission_denied` 에러로 즉시 회신.
pub fn handle_with_caller(
    state: &mut AppState,
    request: &JsonRpcRequest,
    caller: &CallerContext,
) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);

    let canonical = alias::canonicalize(&request.method);
    if alias::is_deprecated(&request.method) {
        tracing::warn!(
            "ipc method '{}' is deprecated; use '{canonical}' (will be removed at 1.0)",
            request.method
        );
    }

    if let Err(e) = caller.ensure_allowed(canonical) {
        tracing::warn!("ipc permission denied: {e}");
        return JsonRpcResponse::error(id, -32001, &format!("permission_denied: {e}"));
    }

    // Phase 4.2 텔레메트리 미들웨어: 비-host caller 의 IPC 호출을 자동 카운트.
    // `telemetry.*` 자체와 `_host` agent 는 카운트 제외 (재귀 폭주 / 자기-측정 방지).
    // 카운트는 cap_eval 도입 전이라 best-effort 만 — 실패해도 dispatch 는 진행.
    telemetry::record_ipc_call(state, caller, canonical);

    // 옛 이름이면 method를 새 이름으로 교체한 임시 request를 라우터에 전달.
    let routed: Cow<JsonRpcRequest> = if canonical == request.method {
        Cow::Borrowed(request)
    } else {
        Cow::Owned(JsonRpcRequest {
            jsonrpc: request.jsonrpc.clone(),
            method: canonical.to_string(),
            params: request.params.clone(),
            id: request.id.clone(),
        })
    };
    let request = routed.as_ref();

    if let Some(resp) = route_engine_handler(state, caller, request, id.clone()) {
        return resp;
    }

    if let Some(resp) = route_gui_handler(state, request, id.clone()) {
        return resp;
    }

    #[cfg(debug_assertions)]
    if let Some(resp) = route_debug_handler(state, request, id.clone()) {
        return resp;
    }

    JsonRpcResponse::method_not_found(id, &request.method)
}

/// engine-substate handlers — UI에 의존하지 않음. 단계 07 권한 게이트 대상.
///
/// 현재는 시그니처가 `&mut AppState`이지만 본문이 GUI를 만지지 않는다. 향후
/// AppState 메서드들이 `EngineState`로 이전되면 시그니처를 `&mut EngineState`로
/// 좁힐 예정 (별도 작업).
fn route_engine_handler(
    state: &mut AppState,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "system.info" => handle_system_info(state, id),
        // workspace
        "workspace.list" => workspace::handle_workspace_list(state, id),
        "workspace.create" => workspace::handle_workspace_create(state, id, &request.params),
        "workspace.update" => workspace::handle_workspace_update(state, id, &request.params),
        "workspace.move" => workspace::handle_workspace_move(state, id, &request.params),
        // pane / split
        "pane.list" => pane::handle_pane_list(state, id),
        "pane.close" => pane::handle_pane_close(state, id, &request.params),
        "split" => pane::handle_split(state, id, &request.params),
        // tab
        "tab.list" => tab::handle_tab_list(state, id, &request.params),
        "tab.create" => tab::handle_tab_create(state, id, &request.params),
        "tab.close" => tab::handle_tab_close(state, id, &request.params),
        "tab.move" => tab::handle_tab_move(state, id, &request.params),
        // surface
        "surface.close" => surface::handle_surface_close(state, id, &request.params),
        "surface.close_self" => surface::handle_surface_close_self(state, id, &request.params),
        "surface.list" => surface::handle_surface_list(state, id),
        "surface.send" => surface::handle_surface_send(state, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(state, id, &request.params),
        "surface.send_combo" => surface::handle_surface_send_combo(state, id, &request.params),
        "surface.send_to" => surface::handle_surface_send_to(state, id, &request.params),
        "surface.wake" => surface::handle_surface_wake(state, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(state, id, &request.params),
        "surface.read_since_mark" => surface::handle_read_since_mark(state, id, &request.params),
        "surface.parse_since_mark" => {
            surface::handle_parse_since_mark(state, id, &request.params)
        }
        "surface.commands" => surface::handle_commands(state, id, &request.params),
        "surface.last_command" => surface::handle_last_command(state, id, &request.params),
        "surface.command_at" => surface::handle_command_at(state, id, &request.params),
        "output.observe_start" => output::handle_observe_start(state, id, &request.params),
        "output.observe_stop" => output::handle_observe_stop(state, id, &request.params),
        "output.observe_list" => output::handle_observe_list(state, id),
        "output.observe_info" => output::handle_observe_info(state, id, &request.params),
        "surface.screen_text" => surface::handle_screen_text(state, id, &request.params),
        "surface.cursor_position" => surface::handle_cursor_position(state, id, &request.params),
        "surface.foreground_process" => {
            surface::handle_foreground_process(state, id, &request.params)
        }
        "surface.locate" => surface::handle_surface_locate(state, id, &request.params),
        "surface.respawn_terminal" => {
            surface::handle_surface_respawn_terminal(state, id, &request.params)
        }
        "surface.is_typing" => handle_is_typing(state, id, &request.params),
        "surface.send_wait_idle" => handle_send_wait_idle(state, id, &request.params),
        "surface.fire_hook" => hooks::handle_surface_fire_hook(state, id, &request.params),
        "surface.meta.set" => meta::handle_surface_meta_set(state, id, &request.params),
        "surface.meta.get" => meta::handle_surface_meta_get(state, id, &request.params),
        "surface.meta.unset" => meta::handle_surface_meta_unset(state, id, &request.params),
        "surface.meta.list" => meta::handle_surface_meta_list(state, id, &request.params),
        // hooks
        "hook.set" => hooks::handle_hook_set(state, id, &request.params),
        "hook.list" => hooks::handle_hook_list(state, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(state, id, &request.params),
        "global_hook.set" => hooks::handle_global_hook_set(state, id, &request.params),
        "global_hook.list" => hooks::handle_global_hook_list(state, id),
        "global_hook.unset" => hooks::handle_global_hook_unset(state, id, &request.params),
        // tree
        "tree" => handle_tree(state, id),
        // message
        "message.send" => message::handle_message_send(state, id, &request.params),
        "message.read" => message::handle_message_read(state, id, &request.params),
        "message.count" => message::handle_message_count(state, id, &request.params),
        "message.clear" => message::handle_message_clear(state, id, &request.params),
        // tool.clipboard (read/write only — viewer_open is GUI)
        "tool.clipboard.list" => clipboard::handle_list(state, id, &request.params),
        "tool.clipboard.get" => clipboard::handle_get(state, id, &request.params),
        "tool.clipboard.paste" => clipboard::handle_paste(state, id, &request.params),
        "tool.clipboard.remove" => clipboard::handle_remove(state, id, &request.params),
        "tool.clipboard.clear" => clipboard::handle_clear(state, id),
        // input source (macOS)
        #[cfg(target_os = "macos")]
        "surface.switch_input_source" => {
            input_source::handle_switch_input_source(id, &request.params)
        }
        #[cfg(target_os = "macos")]
        "surface.raw_key" => input_source::handle_raw_key(id, &request.params),
        // notification (focus-independent — workspace_id/surface_id로 라우팅)
        "notification.list" => notification::handle_notification_list(state, id),
        "notification.create" => {
            notification::handle_notification_create(state, id, &request.params)
        }
        // image surface 조작 — com.tasty.image plugin이 외부에 노출하는 namespace의
        // 호스트 어댑터. plugin 비활성 상태에서도 CLI/직접 IPC로 호출 가능.
        "image.open" => image::handle_open(state, id, &request.params),
        "image.save" => image::handle_save(state, id, &request.params),
        "image.export_png" => image::handle_export_png(state, id, &request.params),
        "image.next" => image::handle_next(state, id, &request.params),
        "image.prev" => image::handle_prev(state, id, &request.params),
        "image.paste" => image::handle_paste(state, id, &request.params),
        "image.list" => image::handle_list(state, id),
        // memory: regular (공유 네임스페이스 + owner enforcement)
        "memory.put" => memory::handle_put(state, caller, id, &request.params),
        "memory.get" => memory::handle_get(state, caller, id, &request.params),
        "memory.delete" => memory::handle_delete(state, caller, id, &request.params),
        "memory.list" => memory::handle_list(state, caller, id, &request.params),
        "memory.exists" => memory::handle_exists(state, caller, id, &request.params),
        "memory.count" => memory::handle_count(state, caller, id, &request.params),
        "memory.scopes" => memory::handle_scopes(state, caller, id, &request.params),
        "memory.stats" => memory::handle_stats(state, caller, id, &request.params),
        "memory.query" => memory::handle_query(state, caller, id, &request.params),
        "memory.export" => memory::handle_export(state, caller, id, &request.params),
        "memory.import" => memory::handle_import(state, caller, id, &request.params),
        // memory: secret (plugin 별 사전 분할)
        "memory.secret.put" => memory::handle_secret_put(state, caller, id, &request.params),
        "memory.secret.get" => memory::handle_secret_get(state, caller, id, &request.params),
        "memory.secret.delete" => memory::handle_secret_delete(state, caller, id, &request.params),
        "memory.secret.list" => memory::handle_secret_list(state, caller, id, &request.params),
        "memory.secret.exists" => memory::handle_secret_exists(state, caller, id, &request.params),
        "memory.secret.count" => memory::handle_secret_count(state, caller, id, &request.params),
        "memory.secret.scopes" => memory::handle_secret_scopes(state, caller, id, &request.params),
        "memory.secret.stats" => memory::handle_secret_stats(state, caller, id, &request.params),
        // memory: 유지 보수 (host 전용)
        "memory.gc" => memory::handle_gc(state, caller, id, &request.params),
        // approval (휴먼 핸드오프) — await 는 process_ipc 에서 worker thread 로 분리 처리.
        "approval.request" => approval::handle_request(state, caller, id, &request.params),
        "approval.respond" => approval::handle_respond(state, caller, id, &request.params),
        "approval.cancel" => approval::handle_cancel(state, caller, id, &request.params),
        "approval.get" => approval::handle_get(state, caller, id, &request.params),
        "approval.list" => approval::handle_list(state, caller, id, &request.params),
        "approval.history" => approval::handle_history(state, caller, id, &request.params),
        "approval.summary.set" => approval::handle_summary_set(state, caller, id, &request.params),
        "approval.summary.get" => approval::handle_summary_get(state, caller, id, &request.params),
        // telemetry (관측 / 비용) — 단계 4.1
        "telemetry.record" => telemetry::handle_record(state, caller, id, &request.params),
        "telemetry.record_batch" => {
            telemetry::handle_record_batch(state, caller, id, &request.params)
        }
        "telemetry.summary" => telemetry::handle_summary(state, caller, id, &request.params),
        "telemetry.timeseries" => telemetry::handle_timeseries(state, caller, id, &request.params),
        "telemetry.top" => telemetry::handle_top(state, caller, id, &request.params),
        // telemetry.cap — Phase 4.3 (CRUD; eval/action wiring 은 후속)
        "telemetry.cap.set" => telemetry::handle_cap_set(state, caller, id, &request.params),
        "telemetry.cap.list" => telemetry::handle_cap_list(state, caller, id, &request.params),
        "telemetry.cap.remove" => telemetry::handle_cap_remove(state, caller, id, &request.params),
        "telemetry.cap.status" => telemetry::handle_cap_status(state, caller, id, &request.params),
        "telemetry.cap.reset" => telemetry::handle_cap_reset(state, caller, id, &request.params),
        _ => return None,
    })
}

/// GUI-dependent handlers — UI 상태(popups/dialogs)를 직접 만지므로 권한 게이트
/// 대상 외부에 있다. release 빌드에서는 비어 있다 — 사용자 입력 재현용 GUI
/// 동작은 모두 debug 전용으로 격리됨.
#[allow(unused_variables)]
fn route_gui_handler(
    state: &mut AppState,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    None
}

#[cfg(debug_assertions)]
fn route_debug_handler(
    state: &mut AppState,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "ui.state" => handle_ui_state(state, id),
        "debug.cell_info" => handle_debug_cell_info(state, id, &request.params),
        "debug.screen_attrs" => handle_debug_screen_attrs(state, id, &request.params),
        "debug.glyph_color" => handle_debug_glyph_color(state, id, &request.params),
        "debug.feed_bytes" => handle_debug_feed_bytes(state, id, &request.params),
        "debug.inject_mouse" => handle_debug_inject_mouse(state, id, &request.params),
        "debug.inject_key" => handle_debug_inject_key(state, id, &request.params),
        // 도구 메뉴 — 사용자 클릭 자동화. release 미노출.
        "debug.tool.list" => tool::handle_list(state, id),
        "debug.tool.invoke" => tool::handle_invoke(state, id, &request.params),
        _ => return None,
    })
}

/// Extract a required surface_id from params. Returns Err(JsonRpcResponse) if missing.
fn require_surface_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'surface_id' parameter")
        })
}

/// Extract a required pane_id from params. Returns Err(JsonRpcResponse) if missing.
fn require_pane_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("pane_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'pane_id' parameter")
        })
}

/// Extract optional caller_surface_id from params.
fn caller_surface_id(params: &serde_json::Value) -> Option<u32> {
    params
        .get("caller_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
}

/// Check if a surface belongs to a pane (directly or in any tab).
fn surface_belongs_to_pane(state: &AppState, surface_id: u32, pane_id: u32) -> bool {
    state.find_pane_for_surface(surface_id) == Some(pane_id)
}

/// Apply metadata key-value pairs to a surface.
fn apply_meta(surface_id: u32, meta: Option<&serde_json::Map<String, serde_json::Value>>) {
    if let Some(map) = meta {
        for (key, value) in map {
            if let Some(v) = value.as_str() {
                if let Err(e) = crate::surface_meta::SurfaceMetaStore::set(surface_id, key, v) {
                    tracing::warn!(
                        "surface_meta set failed for surface {surface_id} key '{key}': {e}"
                    );
                }
            }
        }
    }
}

fn handle_system_info(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "workspace_count": state.engine.workspaces.len(),
            "active_workspace": state.active_workspace,
        }),
    )
}

#[cfg(debug_assertions)]
fn handle_ui_state(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let ws = state.active_workspace();
    let pane_count = ws.pane_layout().all_pane_ids().len();
    let focused_pane_id = ws.focused_pane;
    let tab_count = ws
        .pane_layout()
        .find_pane(focused_pane_id)
        .map(|p| p.tabs.len())
        .unwrap_or(0);
    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "notification_panel_open": state.popups.is_open("notifications"),
            "active_workspace": state.active_workspace,
            "workspace_count": state.engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
        }),
    )
}

#[cfg(debug_assertions)]
fn handle_debug_cell_info(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    let col = match params.get("col").and_then(|v| v.as_u64()) {
        Some(c) => c as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'col' parameter"),
    };
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        if let Some(info) = terminal.cell_info(row, col) {
            JsonRpcResponse::success(id, cell_info_to_json(&info))
        } else {
            JsonRpcResponse::success(id, json!({"text": "", "fg": "default", "bg": "default"}))
        }
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

#[cfg(debug_assertions)]
fn cell_info_to_json(info: &tasty_terminal::CellInfo) -> serde_json::Value {
    json!({
        "text": info.text,
        "fg": info.fg,
        "bg": info.bg,
        "bold": info.bold,
        "italic": info.italic,
        "underline": info.underline,
        "strikethrough": info.strikethrough,
        "inverse": info.inverse,
        "width": info.width,
        "intensity": info.intensity,
        "underline_style": info.underline_style,
        "underline_color": info.underline_color,
        "blink": info.blink,
        "invisible": info.invisible,
        "overline": info.overline,
        "vertical_align": info.vertical_align,
    })
}

#[cfg(debug_assertions)]
fn handle_debug_screen_attrs(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        let cells: Vec<_> = terminal
            .row_cells(row)
            .into_iter()
            .map(|(col, info)| {
                let mut obj = cell_info_to_json(&info);
                if let Some(map) = obj.as_object_mut() {
                    map.insert("col".into(), json!(col));
                }
                obj
            })
            .collect();
        JsonRpcResponse::success(id, json!({"row": row, "cells": cells}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// Inject raw VTE bytes directly into a surface's terminal, bypassing the PTY
/// and the shell. Useful for renderer/parser tests that need deterministic
/// escape sequences without depending on shell escaping rules.
///
/// Accepts either `bytes` (hex string) or `text` (UTF-8 string with optional
/// `\xHH` escape support disabled — the text is fed verbatim).
#[cfg(debug_assertions)]
fn handle_debug_feed_bytes(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let bytes: Vec<u8> = if let Some(hex) = params.get("bytes").and_then(|v| v.as_str()) {
        let hex = hex.trim();
        if hex.len() % 2 != 0 {
            return JsonRpcResponse::invalid_params(id, "'bytes' hex must have even length");
        }
        let mut out = Vec::with_capacity(hex.len() / 2);
        for i in (0..hex.len()).step_by(2) {
            match u8::from_str_radix(&hex[i..i + 2], 16) {
                Ok(b) => out.push(b),
                Err(_) => return JsonRpcResponse::invalid_params(id, "Invalid hex in 'bytes'"),
            }
        }
        out
    } else if let Some(t) = params.get("text").and_then(|v| v.as_str()) {
        t.as_bytes().to_vec()
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing 'bytes' or 'text' parameter");
    };
    let Some(terminal) = state.find_terminal_by_id_mut(surface_id) else {
        return JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id));
    };
    terminal.process_bytes(&bytes);
    JsonRpcResponse::success(id, json!({"fed": bytes.len()}))
}

/// Returns the (bg, fg) RGBA pair the renderer would push to the GPU for a single
/// cell, given only its `CellAttributes` and the surface's default background.
///
/// This intentionally bypasses contextual overrides (selection, link hover, cursor,
/// IME preedit) — the goal is to verify the renderer's per-cell color resolution.
/// If the renderer omits a transformation (e.g. SGR 2 dim handling), this method
/// will report colors that match the (broken) GPU output, exposing the gap.
#[cfg(debug_assertions)]
fn handle_debug_glyph_color(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    let col = match params.get("col").and_then(|v| v.as_u64()) {
        Some(c) => c as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'col' parameter"),
    };
    // bg_mode: "focused" (default) | "unfocused"
    let bg_mode = params
        .get("bg_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("focused");
    let default_bg = match bg_mode {
        "focused" => state
            .engine
            .settings
            .appearance
            .terminal_colors
            .focused_bg
            .to_float(),
        "unfocused" => state
            .engine
            .settings
            .appearance
            .terminal_colors
            .unfocused_bg
            .to_float(),
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("bg_mode must be 'focused' or 'unfocused', got '{}'", other),
            );
        }
    };
    let Some(terminal) = state.find_terminal_by_id(surface_id) else {
        return JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id));
    };
    let Some(attrs) = terminal.cell_attrs(row, col) else {
        return JsonRpcResponse::success(
            id,
            json!({
                "row": row,
                "col": col,
                "in_bounds": false,
            }),
        );
    };
    let (bg, fg) = crate::renderer::resolve_cell_colors(&attrs, default_bg);
    JsonRpcResponse::success(
        id,
        json!({
            "row": row,
            "col": col,
            "in_bounds": true,
            "bg_mode": bg_mode,
            "default_bg": rgba_to_json(default_bg),
            "bg": rgba_to_json(bg),
            "fg": rgba_to_json(fg),
        }),
    )
}

#[cfg(debug_assertions)]
fn rgba_to_json(rgba: [f32; 4]) -> serde_json::Value {
    json!({
        "r": rgba[0],
        "g": rgba[1],
        "b": rgba[2],
        "a": rgba[3],
        "hex": format!(
            "#{:02x}{:02x}{:02x}",
            (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        ),
    })
}

#[cfg(debug_assertions)]
fn require_input_simulation(
    state: &AppState,
    id: &serde_json::Value,
) -> Result<(), JsonRpcResponse> {
    if !state.engine.input_simulation_enabled {
        Err(JsonRpcResponse::error(
            id.clone(),
            -32001,
            "input simulation not enabled. Launch tasty with --enable-input-simulation",
        ))
    } else {
        Ok(())
    }
}

/// Inject a mouse event into a surface's PTY as if the terminal received it.
/// Encodes as SGR mouse (mode 1006) bytes: ESC [ < Cb ; Cx ; Cy M/m
#[cfg(debug_assertions)]
fn handle_debug_inject_mouse(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, &id) {
        return e;
    }
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // col, row: 1-indexed cell coordinates
    let col = match params.get("col").and_then(|v| v.as_u64()) {
        Some(c) => c,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'col' parameter"),
    };
    let row = match params.get("row").and_then(|v| v.as_u64()) {
        Some(r) => r,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    // button: 0=left, 1=middle, 2=right. Default: 0
    let button = params
        .get("button")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // event_type: "press", "release", "move". Default: "press"
    let event_type = params
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("press");

    let cb = match event_type {
        "press" => button as u8,
        "release" => button as u8,
        "move" => 32 + button as u8,
        _ => return JsonRpcResponse::invalid_params(id, "event_type must be press/release/move"),
    };
    let suffix = if event_type == "release" { "m" } else { "M" };

    // SGR mouse encoding: ESC [ < Cb ; Cx ; Cy M/m (1-indexed)
    let seq = format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, suffix);

    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&seq);
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// Inject a key event into a surface's PTY.
#[cfg(debug_assertions)]
fn handle_debug_inject_key(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, &id) {
        return e;
    }
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let bytes = match params.get("bytes").and_then(|v| v.as_str()) {
        Some(hex) => {
            let hex = hex.trim();
            let mut result = Vec::new();
            for i in (0..hex.len()).step_by(2) {
                match u8::from_str_radix(&hex[i..i.min(hex.len()).max(i + 2)], 16) {
                    Ok(b) => result.push(b),
                    Err(_) => {
                        return JsonRpcResponse::invalid_params(id, "Invalid hex in 'bytes'")
                    }
                }
            }
            result
        }
        None => match params.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.as_bytes().to_vec(),
            None => {
                return JsonRpcResponse::invalid_params(id, "Missing 'bytes' or 'text' parameter")
            }
        },
    };

    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&String::from_utf8_lossy(&bytes));
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

fn handle_tree(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let tree: Vec<_> = state
        .engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let mut t = ws.to_tree_json();
            t["active"] = json!(i == state.active_workspace);
            t["busy_count"] = json!(state.busy_count(&ws.all_surface_ids()));
            annotate_tree_busy(&mut t, state);
            t
        })
        .collect();
    JsonRpcResponse::success(id, json!(tree))
}

/// Walk a workspace tree JSON value and annotate every node that owns surface
/// ids with a `busy_count` field. Surface-leaf nodes also get a `busy` boolean.
fn annotate_tree_busy(node: &mut serde_json::Value, state: &AppState) {
    if let Some(obj) = node.as_object_mut() {
        // Surface leaf: has "id" but no "tabs"/"panes"/"first"/"second"
        let is_leaf = !obj.contains_key("tabs")
            && !obj.contains_key("panes")
            && !obj.contains_key("first")
            && !obj.contains_key("second")
            && obj.get("id").is_some();
        if is_leaf {
            if let Some(sid) = obj.get("id").and_then(|v| v.as_u64()) {
                obj.insert("busy".into(), json!(state.is_surface_busy(sid as u32)));
            }
            return;
        }

        // Recurse into children.
        for key in ["panes", "tabs"] {
            if let Some(arr) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                for child in arr.iter_mut() {
                    annotate_tree_busy(child, state);
                }
            }
        }
        for key in ["first", "second", "surface"] {
            if let Some(child) = obj.get_mut(key) {
                annotate_tree_busy(child, state);
            }
        }

        // After children are annotated, sum descendant busy counts.
        let mut count: u64 = 0;
        for key in ["panes", "tabs"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                for child in arr {
                    count += child.get("busy_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    if child.get("busy").and_then(|v| v.as_bool()).unwrap_or(false)
                        && child.get("busy_count").is_none()
                    {
                        count += 1;
                    }
                }
            }
        }
        for key in ["first", "second", "surface"] {
            if let Some(child) = obj.get(key) {
                count += child.get("busy_count").and_then(|v| v.as_u64()).unwrap_or(0);
                if child.get("busy").and_then(|v| v.as_bool()).unwrap_or(false)
                    && child.get("busy_count").is_none()
                {
                    count += 1;
                }
            }
        }
        // Workspaces already had busy_count set by the caller; only insert if missing.
        if !obj.contains_key("busy_count") {
            obj.insert("busy_count".into(), json!(count));
        }
    }
}

fn handle_is_typing(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let typing = state.is_typing(surface_id);
    let idle_seconds = if let Some(last) = state.engine.last_key_input.get(&surface_id) {
        last.elapsed().as_secs_f64()
    } else {
        f64::MAX
    };
    let idle_seconds_capped = if idle_seconds == f64::MAX {
        -1.0
    } else {
        idle_seconds
    };
    JsonRpcResponse::success(
        id,
        json!({
            "typing": typing,
            "idle_seconds": idle_seconds_capped,
        }),
    )
}

fn handle_send_wait_idle(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    if state.is_typing(surface_id) {
        return JsonRpcResponse::success(id, json!({ "sent": false, "reason": "typing" }));
    }
    state.engine.ensure_surface_initialized(surface_id);
    if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&text);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}
