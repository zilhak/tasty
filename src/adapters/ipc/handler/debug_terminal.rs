//! Debug 빌드 전용 IPC 핸들러 중 **터미널 그리드만 보는 것** — 셀 속성 조회 ·
//! 행 단위 속성 조회 · VTE 바이트 직접 주입.
//!
//! `debug.rs` 와 갈라져 있는 이유는 `debug_nav.rs` 와 같다 — 그쪽 모듈이
//! `#[cfg(all(debug_assertions, feature = "gui"))]` 라 헤드리스에서 통째로
//! 사라지는데, 여기 셋은 `CoreState::find_terminal_by_id` 로 얻은 터미널만
//! 만지고 gui 게이트된 심볼을 하나도 안 쓴다. 터미널 그리드는 headless 에도
//! 그대로 있다 — 없는 것은 그것을 **그리는** 층이다.
//!
//! `debug.glyph_color` 도 여기 있다. 그것은 그리드가 아니라 **렌더러의 색 해석**을
//! 묻지만, 그 해석 자체(`cell_palette::compute_cell_colors`)는 `CellAttributes` 와
//! 색 타입만 쓰는 순수 함수다 — 한동안 `#[cfg(feature = "gui")] mod gfx;` 아래
//! 있었을 뿐이고, 그래서 그 모듈을 게이트 밖으로 꺼냈다. 렌더러와 **같은 함수**를
//! 부르는 것이 이 메서드의 정의라(렌더러가 빠뜨린 변환이 있으면 그대로 노출된다),
//! 복제가 아니라 이동이어야 했다.

#![cfg(debug_assertions)]

use super::params::{self, p_try};
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
    let row = match p_try!(params::opt_int::<u64>(params, "row", &id)) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    let col = match p_try!(params::opt_int::<u64>(params, "col", &id)) {
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
    let row = match p_try!(params::opt_int::<u64>(params, "row", &id)) {
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
    let row = match p_try!(params::opt_int::<u64>(params, "row", &id)) {
        Some(r) => r as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    let col = match p_try!(params::opt_int::<u64>(params, "col", &id)) {
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
    let (bg, fg) = crate::cell_palette::compute_cell_colors(&attrs, default_bg, default_fg, &ansi);
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
