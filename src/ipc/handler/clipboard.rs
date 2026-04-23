//! `tool.clipboard.*` IPC 핸들러. AI 에이전트가 클립보드 히스토리를 조회/조작한다.
//!
//! 포커스 독립성 원칙: viewer_open은 **항상 focus 없이** popup을 연다. 사용자가
//! 작업 중인 포커스를 뺏지 않는다. 사용자가 focus를 원하면 단축키를 눌러야 한다.

use serde_json::{Value, json};

use crate::clipboard_history::ClipboardSource;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

fn source_str(s: ClipboardSource) -> &'static str {
    match s {
        ClipboardSource::System => "system",
        ClipboardSource::Internal => "internal",
    }
}

pub fn handle_list(state: &AppState, id: Value, params: &Value) -> JsonRpcResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let total = state.engine.clipboard_history.len();
    let entries: Vec<Value> = state
        .engine
        .clipboard_history
        .entries()
        .take(limit.unwrap_or(usize::MAX))
        .enumerate()
        .map(|(i, e)| {
            json!({
                "index": i,
                "text": e.display_text(),
                "is_image": e.is_image(),
                "source": source_str(e.source),
                "age_ms": e.captured_at.elapsed().as_millis() as u64,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "total": total, "entries": entries }))
}

pub fn handle_get(state: &AppState, id: Value, params: &Value) -> JsonRpcResponse {
    let idx = match params.get("index").and_then(|v| v.as_u64()) {
        Some(n) => n as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index'"),
    };
    match state.engine.clipboard_history.get(idx) {
        Some(e) => JsonRpcResponse::success(
            id,
            json!({
                "index": idx,
                "text": e.display_text(),
                "is_image": e.is_image(),
                "source": source_str(e.source),
                "age_ms": e.captured_at.elapsed().as_millis() as u64,
            }),
        ),
        None => JsonRpcResponse::invalid_params(id, format!("Index {idx} out of range")),
    }
}

pub fn handle_paste(state: &mut AppState, id: Value, params: &Value) -> JsonRpcResponse {
    use crate::clipboard_history::ClipboardContent;

    let idx = match params.get("index").and_then(|v| v.as_u64()) {
        Some(n) => n as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index'"),
    };
    let content = match state.engine.clipboard_history.get(idx) {
        Some(e) => e.content.clone(),
        None => return JsonRpcResponse::invalid_params(id, format!("Index {idx} out of range")),
    };
    match content {
        ClipboardContent::Text(text) => {
            match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.clone())) {
                Ok(()) => {
                    state.engine.record_internal_copy(&text);
                    JsonRpcResponse::success(id, json!({ "ok": true, "index": idx }))
                }
                Err(e) => {
                    JsonRpcResponse::internal_error(id, format!("clipboard set_text failed: {e}"))
                }
            }
        }
        ClipboardContent::Image(img) => {
            match image::load_from_memory_with_format(&img.png_bytes, image::ImageFormat::Png) {
                Ok(dyn_img) => {
                    let rgba = dyn_img.to_rgba8();
                    let arboard_img = arboard::ImageData {
                        width: rgba.width() as usize,
                        height: rgba.height() as usize,
                        bytes: rgba.into_raw().into(),
                    };
                    match arboard::Clipboard::new().and_then(|mut cb| cb.set_image(arboard_img)) {
                        Ok(()) => {
                            state.engine.clipboard_history.record_image(
                                img,
                                crate::clipboard_history::ClipboardSource::Internal,
                            );
                            JsonRpcResponse::success(id, json!({ "ok": true, "index": idx }))
                        }
                        Err(e) => JsonRpcResponse::internal_error(
                            id,
                            format!("clipboard set_image failed: {e}"),
                        ),
                    }
                }
                Err(e) => {
                    JsonRpcResponse::internal_error(id, format!("Failed to decode PNG: {e}"))
                }
            }
        }
    }
}

pub fn handle_remove(state: &mut AppState, id: Value, params: &Value) -> JsonRpcResponse {
    let idx = match params.get("index").and_then(|v| v.as_u64()) {
        Some(n) => n as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index'"),
    };
    match state.engine.clipboard_history.remove_at(idx) {
        Some(_) => JsonRpcResponse::success(id, json!({ "ok": true, "index": idx })),
        None => JsonRpcResponse::invalid_params(id, format!("Index {idx} out of range")),
    }
}

pub fn handle_clear(state: &mut AppState, id: Value) -> JsonRpcResponse {
    state.engine.clipboard_history.clear();
    JsonRpcResponse::success(id, json!({ "ok": true }))
}

pub fn handle_viewer_open(state: &mut AppState, id: Value) -> JsonRpcResponse {
    state.dialogs.clipboard_viewer = crate::clipboard_viewer_ui::ClipboardViewerState::default();
    state.popups.open_centered("clipboard_viewer");
    JsonRpcResponse::success(id, json!({ "ok": true }))
}
