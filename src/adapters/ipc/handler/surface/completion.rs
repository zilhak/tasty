use serde_json::json;

use crate::core::AttentionKind;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// `surface.completion` — 특정 surface 를 "작업 완료"(또는 "응답 필요") 신호 처리해
/// highlight(주의 환기) 를 발동한다. highlight 를 발동하는 여러 producer 중 하나
/// (release 정식). surface_id 필수(포커스 독립 — 불가침 원칙 1).
/// `mark.rs::handle_set_mark` 미러.
///
/// `kind` 는 선택 파라미터 — `"needs_input"` 이면 `AttentionKind::NeedsInput`,
/// 그 외(생략 포함)는 기존과 동일하게 `AttentionKind::Completion` 이다(하위 호환:
/// CLI/OSC 133/toast/windows-resume producer 는 kind 를 모른 채 이 IPC 를 호출한다 —
/// Claude 플러그인 훅만 명시적으로 kind 를 싣는다, `hook.rs::HostCall::SurfaceCompletion`).
pub(crate) fn handle_completion(
    state: &mut AppState,
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
    let _ = engine; // handler 는 enqueue 만. cascade 가 highlight 발동 + redraw.
    state.dispatch_intent(
        crate::core::intent::DomainIntent::SurfaceCompletion { surface_id, kind }.from_agent_ipc(),
    );
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}
