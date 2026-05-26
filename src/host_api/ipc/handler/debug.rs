//! Debug 빌드 전용 IPC 핸들러 — cell_info / screen_attrs / feed_bytes /
//! glyph_color / inject_mouse / inject_key.
//!
//! 디버그 빌드에서만 라우터에 등록되며, 사용자의 키/마우스 입력을 재현하는
//! 저수준 IPC 표면을 제공한다. release 빌드에는 본 모듈이 컴파일되지 않는다.

#![cfg(debug_assertions)]

use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

pub(super) fn handle_debug_cell_info(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
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
    engine: &crate::engine_state::EngineState,
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
    engine: &mut crate::engine_state::EngineState,
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
    engine: &crate::engine_state::EngineState,
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
        "focused" => engine
            .settings
            .appearance
            .terminal_colors
            .focused_bg
            .to_float(),
        "unfocused" => engine
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
pub(super) fn rgba_to_json(rgba: [f32; 4]) -> serde_json::Value {
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
    engine: &crate::engine_state::EngineState,
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
    engine: &mut crate::engine_state::EngineState,
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
        "press" => button as u8,
        "release" => button as u8,
        "move" => 32 + button as u8,
        _ => return JsonRpcResponse::invalid_params(id, "event_type must be press/release/move"),
    };
    let suffix = if event_type == "release" { "m" } else { "M" };

    // SGR mouse encoding: ESC [ < Cb ; Cx ; Cy M/m (1-indexed)
    let seq = format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, suffix);

    if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&seq);
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// Inject a key event into a surface's PTY.
#[cfg(debug_assertions)]
pub(super) fn handle_debug_inject_key(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
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
