//! 헤드리스에서도 쓰는 디버그 터미널 조회와 바이트 주입.
//! glyph_color는 렌더러와 같은 색 계산 함수를 사용하되 선택·커서 등의 덮어쓰기는 제외한다.

#![cfg(debug_assertions)]

use super::params::{self, p_try};
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

pub(super) fn handle_debug_cell_info(
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

/// PTY와 셸을 거치지 않고 VTE 파서에 입력한다.
/// bytes는 hex, text는 UTF-8 그대로 받으며 text의 이스케이프 표기는 해석하지 않는다.
#[cfg(debug_assertions)]
pub(super) fn handle_debug_feed_bytes(
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

/// 셀 속성과 기본 배경으로 렌더러의 색 계산 결과를 반환한다.
/// 선택·링크 hover·커서·IME에 따른 색 덮어쓰기는 포함하지 않는다.
#[cfg(debug_assertions)]
pub(super) fn handle_debug_glyph_color(
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
