//! Debug 빌드 전용 IPC 핸들러 — cell_info / screen_attrs / feed_bytes /
//! glyph_color / inject_mouse / inject_key.
//!
//! 디버그 빌드에서만 라우터에 등록되며, 사용자의 키/마우스 입력을 재현하는
//! 저수준 IPC 표면을 제공한다. release 빌드에는 본 모듈이 컴파일되지 않는다.

#![cfg(debug_assertions)]

use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

pub(super) fn handle_debug_cell_info(
    _state: &AppState,
    engine: &crate::core::CoreState,
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
    if let Some(terminal) = engine.find_terminal_by_id(surface_id) {
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
pub(super) fn cell_info_to_json(info: &tasty_terminal::CellInfo) -> serde_json::Value {
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
pub(super) fn handle_debug_screen_attrs(
    _state: &AppState,
    engine: &crate::core::CoreState,
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
    if let Some(terminal) = engine.find_terminal_by_id(surface_id) {
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
pub(super) fn handle_debug_feed_bytes(
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
    let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) else {
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
pub(super) fn handle_debug_glyph_color(
    _state: &AppState,
    engine: &crate::core::CoreState,
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
    let theme = crate::theme::theme();
    let term_surface = theme.surface("terminal");
    let (default_bg, default_fg) = match bg_mode {
        "focused" => (
            term_surface.focused_bg.to_gpu_rgba(),
            term_surface.focused_fg.to_gpu_rgba(),
        ),
        "unfocused" => (
            term_surface.unfocused_bg.to_gpu_rgba(),
            term_surface.unfocused_fg.to_gpu_rgba(),
        ),
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("bg_mode must be 'focused' or 'unfocused', got '{}'", other),
            );
        }
    };
    let ansi = theme.ansi_palette();
    let Some(terminal) = engine.find_terminal_by_id(surface_id) else {
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
    let (bg, fg) = crate::renderer::resolve_cell_colors(&attrs, default_bg, default_fg, &ansi);
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
pub(super) fn rgba_to_json(rgba: tasty_type_appearance::color::GpuRgba) -> serde_json::Value {
    let rgba = rgba.as_array();
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
pub(super) fn require_input_simulation(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: &serde_json::Value,
) -> Result<(), JsonRpcResponse> {
    if !engine.input_simulation_enabled {
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
pub(super) fn handle_debug_inject_mouse(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, engine, &id) {
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
    let button = params.get("button").and_then(|v| v.as_u64()).unwrap_or(0);
    // event_type: "press", "release", "move". Default: "press"
    let event_type = params
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("press");

    let cb = match event_type {
        "press" | "release" => button as u8,
        "move" => 32 + button as u8,
        _ => return JsonRpcResponse::invalid_params(id, "event_type must be press/release/move"),
    };
    // SGR mouse(1006). col/row 입력은 0-indexed → 1-based 로. 공용 인코더 사용.
    let bytes = tasty_terminal::encode_mouse_report(
        true,
        cb,
        (col + 1) as usize,
        (row + 1) as usize,
        event_type == "release",
    );

    if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
        terminal.send_bytes(&bytes);
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// `debug.host_popup.list` — 호스트 빌트인 popup(`PopupDef`) 전체 목록을 반환.
///
/// plugin 이 contribute 한 popup 은 `debug.popup.list` 가 담당한다. 이쪽은 tasty
/// 본체가 `popup::defs::all_defs()` 로 정의한 빌트인 popup (tools_menu / port_scanner
/// / command_palette / remote_tool 등) 전용이다. 사용자 클릭 경로 없이 popup 을
/// 직접 띄워 시각 검증하기 위한 debug 격리 표면.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_host_popup_list(
    state: &AppState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let items: Vec<_> = crate::adapters::ui::popup::defs::all_defs()
        .iter()
        .map(|def| {
            // 열려 있는 popup 은 현재 rect 와 z_seq 를 함께 노출한다 — 겹친 popup 의
            // 마우스 소유권 판정(`popup/occlusion.rs`)을 debug 로 검증할 때 좌표를
            // 실측 없이 조준하기 위한 관찰면. z_seq 는 plugin popup 과 공유하는 전역
            // 시퀀스라 `debug.popup.list` 값과 직접 비교된다.
            let geom = state.popups.open_geometry(def.id);
            json!({
                "id": def.id,
                "title_key": def.title_key,
                "headless": def.headless,
                "close_on_outside_click": def.close_on_outside_click,
                "open": geom.is_some(),
                "z_seq": geom.map(|(z, _)| z),
                "rect": geom.map(|(_, r)| {
                    json!({ "x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height() })
                }),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "popups": items }))
}

/// `debug.host_popup.open` — `{ popup_id }` 로 호스트 빌트인 popup 을 화면 중앙에
/// 강제로 띄운다. 사용자 클릭(사이드바 도구 버튼 → 메뉴 → 항목) 을 재현하는
/// 디버그 동작이므로 release 에 노출되지 않는다.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_host_popup_open(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(popup_id) = params.get("popup_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'popup_id' parameter");
    };
    // 런타임 문자열 → 정적 def id (`PopupId` 는 &'static str). 정의에 없는 id 는 거부.
    let Some(def) = crate::adapters::ui::popup::defs::find(popup_id) else {
        return JsonRpcResponse::error(id, -32602, format!("host popup '{popup_id}' not found"));
    };
    // `workspace_scope` 는 런타임 스코프 주입(`OpenPopupMode::WithScope`)을 쓰는
    // popup 을 위한 것이다 — 그런 popup 은 `CenteredFocused` 로 열면 스코프가
    // 기본값(`Window`)에 머물러 워크스페이스 가시성 게이트가 아예 발동하지 않는다.
    let workspace_scope = params
        .get("workspace_scope")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mode = if workspace_scope {
        crate::intent::OpenPopupMode::WithScope(crate::adapters::ui::popup::PopupScope::Workspace(
            state.active_workspace,
        ))
    } else {
        crate::intent::OpenPopupMode::CenteredFocused
    };
    state.dispatch_intent(crate::intent::UiIntent::OpenPopup { id: def.id, mode }.from_agent_ipc());
    JsonRpcResponse::success(
        id,
        json!({ "opened": def.id, "workspace_scope": workspace_scope }),
    )
}

/// `debug.host_popup.close` — `{ popup_id }` 로 호스트 빌트인 popup 을 닫는다.
/// 여러 popup 을 차례로 스크린샷할 때 직전 popup 을 정리하는 용도.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_host_popup_close(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(popup_id) = params.get("popup_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'popup_id' parameter");
    };
    let Some(def) = crate::adapters::ui::popup::defs::find(popup_id) else {
        return JsonRpcResponse::error(id, -32602, format!("host popup '{popup_id}' not found"));
    };
    state.dispatch_intent(crate::intent::UiIntent::ClosePopup { id: def.id }.from_agent_ipc());
    JsonRpcResponse::success(id, json!({ "closed": def.id }))
}

/// `debug.modifier_hint.hold` — `{ ctrl, alt, option, shift, elapsed_ms? }` 로 오버레이의
/// 홀드 조합을 직접 세팅한다(생략 축 = false, 모두 false 면 홀드 해제). `elapsed_ms` 가
/// 있으면 타이머를 그만큼 과거로 백데이트해 표시 지연 게이트를 즉시 통과시킨다.
///
/// 원칙1상 오버레이는 실 modifier 홀드로만 뜨지만, 이는 PTY raw 주입이 아니라 오버레이
/// 내부 상태만 세팅하는 force-state 라 `host_popup.open` 과 동일하게 debug 격리로 충분하다.
/// 응답은 `state` 와 동일한 렌더 상태 덤프. release 미노출.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_modhint_hold(
    state: &mut AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let axis = |k: &str| params.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    state
        .modifier_hint
        .update_hold(axis("ctrl"), axis("alt"), axis("option"), axis("shift"));
    if let Some(ms) = params.get("elapsed_ms").and_then(|v| v.as_u64()) {
        state
            .modifier_hint
            .debug_backdate(std::time::Duration::from_millis(ms));
    }
    let theme = crate::theme::theme();
    let reduced_motion = engine.settings.accessibility.reduced_motion;
    let dump = crate::adapters::ui::modifier_hint_overlay::debug_state_json(
        &state.modifier_hint,
        &engine.settings,
        &theme,
        reduced_motion,
    );
    JsonRpcResponse::success(id, dump)
}

/// `debug.modifier_hint.state` — 오버레이의 현재 렌더 상태를 draw 경로와 동일 로직으로
/// 재평가해 덤프한다(held / 지연 / alpha / visible / header_combo / 좁혀진 sections). 스크린샷
/// 없이 좁힘·즉시갱신·지연을 자동 단정하기 위한 debug 격리 표면. release 미노출.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_modhint_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let theme = crate::theme::theme();
    let reduced_motion = engine.settings.accessibility.reduced_motion;
    let dump = crate::adapters::ui::modifier_hint_overlay::debug_state_json(
        &state.modifier_hint,
        &engine.settings,
        &theme,
        reduced_motion,
    );
    JsonRpcResponse::success(id, dump)
}

/// `debug.banner.list` — 빌트인 배너 정의 목록 + 현재 표시/대기 상태를 반환.
///
/// 배너는 사용자 행동에서만 발사되므로(발화 정책 §불가침) 이 표면은 release 에
/// 노출되지 않는다. plugin 기여 배너는 (도입 시) 별도 표면이 담당한다.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let defs: Vec<_> = crate::adapters::ui::banner::defs::all_defs()
        .iter()
        .map(|def| {
            json!({
                "id": def.id,
                "ttl_seconds": def.ttl_seconds,
            })
        })
        .collect();
    let shown: Vec<_> = state
        .banners
        .shown_banners()
        .map(|b| {
            let queued: Vec<&str> = state
                .banners
                .queued_banners(&b.scope)
                .map(|q| q.id)
                .collect();
            json!({
                "id": b.id,
                "scope": b.scope.to_token(),
                "remaining_seconds": b.remaining_seconds(),
                "queued": queued,
            })
        })
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "defs": defs,
            "shown": shown,
            "total_queued": state.banners.total_queued(),
        }),
    )
}

/// `debug.banner.show` — `{ banner_id, scope }` 로 배너를 직접 발화한다.
///
/// 사용자 조작(마우스 캡쳐 surface 에서 드래그 등) 을 재현하는 디버그 동작이라
/// release 에 없다. `scope` 는 `view` / `workspace:<i>` / `pane:<id>` /
/// `tab:<pane>:<i>` / `surface:<id>` 토큰. def 의 ttl 을 그대로 적용한다.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_show(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(banner_id) = params.get("banner_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'banner_id' parameter");
    };
    let Some(scope_token) = params.get("scope").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'scope' parameter");
    };
    let Some(def) = crate::adapters::ui::banner::defs::find(banner_id) else {
        return JsonRpcResponse::error(id, -32602, format!("banner '{banner_id}' not found"));
    };
    let Some(scope) = crate::adapters::ui::BannerScope::from_token(scope_token) else {
        return JsonRpcResponse::error(id, -32602, format!("invalid scope '{scope_token}'"));
    };
    let banner = match def.ttl_seconds {
        Some(secs) => crate::adapters::ui::BannerState::with_ttl(def.id, scope, secs),
        None => crate::adapters::ui::BannerState::persistent(def.id, scope),
    };
    let outcome = state.banners.push(banner);
    JsonRpcResponse::success(id, json!({ "outcome": format!("{outcome:?}") }))
}

/// `debug.banner.close` — `{ banner_id }` 로 배너를 닫는다 (표시 중이면 큐 head 승격).
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_close(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(banner_id) = params.get("banner_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'banner_id' parameter");
    };
    let Some(def) = crate::adapters::ui::banner::defs::find(banner_id) else {
        return JsonRpcResponse::error(id, -32602, format!("banner '{banner_id}' not found"));
    };
    let closed = state.banners.close_by_id(def.id);
    JsonRpcResponse::success(id, json!({ "closed": closed }))
}

/// `debug.banner.set_countdown` — `{ scope, seconds }` 로 표시 중 TTL 배너의
/// 남은 시간을 강제 설정한다 (만료 직전 상태 등 시각 검증용).
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_set_countdown(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(scope_token) = params.get("scope").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'scope' parameter");
    };
    let Some(seconds) = params.get("seconds").and_then(|v| v.as_u64()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'seconds' parameter");
    };
    let Some(scope) = crate::adapters::ui::BannerScope::from_token(scope_token) else {
        return JsonRpcResponse::error(id, -32602, format!("invalid scope '{scope_token}'"));
    };
    let applied = state.banners.set_countdown(&scope, seconds as u32);
    JsonRpcResponse::success(id, json!({ "applied": applied }))
}

/// Inject a key event into a surface's PTY.
#[cfg(debug_assertions)]
pub(super) fn handle_debug_inject_key(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, engine, &id) {
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
                    Err(_) => return JsonRpcResponse::invalid_params(id, "Invalid hex in 'bytes'"),
                }
            }
            result
        }
        None => match params.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.as_bytes().to_vec(),
            None => {
                return JsonRpcResponse::invalid_params(id, "Missing 'bytes' or 'text' parameter");
            }
        },
    };

    if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&String::from_utf8_lossy(&bytes));
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// 포커스 pane 의 활성 탭 전환 — 사용자의 탭 클릭 재현. release 미노출
/// ([`handle_debug_switch_workspace`] 의 탭 대응). egui-mesh 텍스처 상태의 탭
/// 전환/복귀 검증 등 탭 가시성 시나리오 재현에 쓴다.
pub(super) fn handle_debug_switch_tab(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = match params.get("index").and_then(|v| v.as_u64()) {
        Some(i) => i as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    if state.goto_tab_in_pane(engine, index) {
        JsonRpcResponse::success(id, json!({"switched": true, "active": index}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Tab index {index} out of range"))
    }
}

/// 워크스페이스 close — 사용자의 워크스페이스 컨텍스트 메뉴 "Close workspace"
/// (`src/view/main/redraw.rs` 의 native 메뉴 응답 `Some(6)`) 재현. release 미노출.
///
/// release IPC 의 `surface.close` 로는 이 경로에 도달할 수 없다 — cascade close 는
/// **탭/패인이 하나만 남았을 때만** workspace 단계까지 올라가므로 cleanup 대상이
/// 항상 surface 1개다. "탭이 많은 워크스페이스를 통째로 닫는" 비용(close 계측
/// `path="gui"`)은 이 메뉴 항목으로만 발생하고, 그래서 계측 기준선을 잡으려면
/// 이 항목을 재현할 수단이 필요하다.
///
/// 사용자 상태(closed_items undo 스택 / 포커스)를 건드리는 사용자 행동이므로
/// release 표면에는 두지 않는다 (CLAUDE.md "사용자 행동 ↔ 에이전트 행동 분리").
pub(super) fn handle_debug_close_workspace(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = match params.get("index").and_then(|v| v.as_u64()) {
        Some(i) => i as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    if index >= engine.workspaces.len() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {index} out of range"),
        );
    }
    // GUI 메뉴 경로는 마지막 workspace 를 닫으면 `request_close()` 로 창까지 닫는다.
    // debug IPC 는 그 창 종료까지 재현하지 않으므로, workspaces 가 비어 다음 redraw
    // 의 `active_workspace()` 가 패닉하는 상태를 만들지 않도록 거절한다.
    if engine.workspaces.len() == 1 {
        return JsonRpcResponse::invalid_params(
            id,
            "Refusing to close the last workspace (would leave no workspace)",
        );
    }
    let closed = state.close_workspace_at(engine, index, crate::state::WorkspaceCloseOrigin::User);
    JsonRpcResponse::success(id, json!({"closed": closed, "index": index}))
}

/// 워크스페이스 활성 전환 — 사용자의 포커스 조작(워크스페이스 전환) 재현. release 미노출.
/// `active_workspace` 인덱스 변경뿐이라 OS 의존성 없음.
pub(super) fn handle_debug_switch_workspace(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = match params.get("index").and_then(|v| v.as_u64()) {
        Some(i) => i as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    if index >= engine.workspaces.len() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {index} out of range"),
        );
    }
    state.switch_workspace(engine, index);
    JsonRpcResponse::success(id, json!({"switched": true, "active": index}))
}
