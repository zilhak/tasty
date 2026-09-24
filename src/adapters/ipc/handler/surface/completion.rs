use serde_json::json;

use crate::core::AttentionKind;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 명시한 surface에 작업 완료 또는 응답 필요 알림을 만든다.
/// kind가 needs_input이면 응답 필요, 그 외 값과 생략은 호환 동작으로 completion을 쓴다.
pub(crate) fn handle_completion(
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let kind = match params.get("kind").and_then(|v| v.as_str()) {
        Some("needs_input") => AttentionKind::NeedsInput,
        _ => AttentionKind::Completion,
    };
    // 헤드리스에서도 반영하도록 직접 상태를 바꾼다. 존재하지 않는 대상은 기존대로 ok를
    // 반환하되, 라우터가 다른 engine으로 넘긴 경우 잘못된 attention 기록은 만들지 않는다.
    if engine.has_surface(surface_id) {
        engine.raise_attention(surface_id, kind);
    }
    // GUI 후속 처리는 표시를 갱신한다.
    out.push(
        crate::core::intent::DomainIntent::SurfaceCompletion { surface_id, kind }.from_agent_ipc(),
    );
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}
