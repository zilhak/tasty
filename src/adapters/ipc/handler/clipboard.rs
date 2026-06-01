//! `tool.clipboard.*` IPC 핸들러. AI 에이전트가 클립보드 히스토리를 조회/조작한다.
//!
//! 포커스 독립성 원칙: viewer_open은 **항상 focus 없이** popup을 연다. 사용자가
//! 작업 중인 포커스를 뺏지 않는다. 사용자가 focus를 원하면 단축키를 눌러야 한다.

use serde_json::{Value, json};

use crate::clipboard_history::ClipboardSource;
use crate::core::CoreState;
use crate::ipc::protocol::JsonRpcResponse;

fn source_str(s: ClipboardSource) -> &'static str {
    match s {
        ClipboardSource::System => "system",
        ClipboardSource::Internal => "internal",
    }
}

pub fn handle_list(engine: &CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let total = engine.clipboard_history.len();
    let entries: Vec<Value> = engine
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

pub fn handle_get(engine: &CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let idx = match params.get("index").and_then(|v| v.as_u64()) {
        Some(n) => n as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index'"),
    };
    match engine.clipboard_history.get(idx) {
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

pub fn handle_paste(
    core: &crate::core::Core,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    use crate::clipboard_history::ClipboardContent;
    use crate::ports::clipboard::ClipboardImage;

    let idx = match params.get("index").and_then(|v| v.as_u64()) {
        Some(n) => n as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index'"),
    };
    let content = match engine.clipboard_history.get(idx) {
        Some(e) => e.content.clone(),
        None => return JsonRpcResponse::invalid_params(id, format!("Index {idx} out of range")),
    };
    match content {
        ClipboardContent::Text(text) => match core.clipboard_write_text(&text) {
            Ok(()) => {
                engine.record_internal_copy(&text);
                JsonRpcResponse::success(id, json!({ "ok": true, "index": idx }))
            }
            Err(e) => {
                JsonRpcResponse::internal_error(id, format!("clipboard set_text failed: {e}"))
            }
        },
        ClipboardContent::Image(img) => {
            match image::load_from_memory_with_format(&img.png_bytes, image::ImageFormat::Png) {
                Ok(dyn_img) => {
                    let rgba = dyn_img.to_rgba8();
                    let port_img = ClipboardImage {
                        width: rgba.width(),
                        height: rgba.height(),
                        pixels: rgba.into_raw(),
                    };
                    match core.clipboard_write_image(&port_img) {
                        Ok(()) => {
                            engine.clipboard_history.record_image(
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
                Err(e) => JsonRpcResponse::internal_error(id, format!("Failed to decode PNG: {e}")),
            }
        }
    }
}
