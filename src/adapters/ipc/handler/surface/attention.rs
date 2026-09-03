use serde_json::json;
use tasty_ipc::stream::AttentionKindWire;

use crate::core::AttentionKind;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// `surface.attention.get` — 특정 surface 에 기록된 attention kind 를 조회한다.
/// `completion.rs` 가 발동만 할 수 있던 상태에 대한 read 표면 — 해제(아래
/// [`handle_attention_clear`])가 실제로 먹혔는지 확인할 수단이 headless 에는
/// 아예 없었다(렌더도 알림 패널도 없다). surface_id 필수(포커스 독립 —
/// 불가침 원칙 1).
///
/// 응답 `kind` 는 `"completion"` / `"needs_input"` / `null`(attention 없음) —
/// `surface.completion` 파라미터 및 attach 스트림([`AttentionKindWire`])과 같은
/// 어휘를 쓴다.
pub(crate) fn handle_attention_get(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if let Err(e) = require_existing_surface(engine, surface_id, &id) {
        return e;
    }
    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "kind": engine.attention_kind(surface_id).map(AttentionKind::to_wire),
        }),
    )
}

/// `surface.attention.clear` — 특정 surface 의 attention 을 해제한다.
/// `surface.completion` 의 역방향으로, raise-only 였던 IPC/CLI 표면을 대칭으로
/// 만든다. 기존 clear producer 두 개(실 렌더 포커스 `gpu.rs`, 알림 읽음)는 전부
/// GUI 로컬 사건이라 headless 인스턴스에는 해제 수단이 하나도 없었다.
///
/// - `surface_id` **필수** — 대상은 항상 ID 로 명시한다(포커스 독립).
/// - `kind` **선택** — 주면 현재 기록된 kind 가 그 값일 때만 지운다(그 사이 다른
///   producer 가 더 급한 kind 로 재발동한 것을 늦게 도착한 해제가 덮지 않도록).
///   생략하면 kind 무관 해제. 알 수 없는 값은 조용히 무시하지 않고 거절한다 —
///   `surface.completion` 은 하위 호환 때문에 미상 kind 를 `completion` 으로
///   떨어뜨리지만, 여기서 같은 관용은 "지정한 kind 만 지운다" 는 계약을 조용히
///   깨뜨린다.
/// - attention 이 없던 surface 에 대한 호출도 성공한다(idempotent) — 응답의
///   `cleared` 가 실제로 지웠는지를 알린다.
/// - **하드 점유(원격 attach) 중인 surface 는 거절**한다. 점유 중에는 그 surface 의
///   상태를 holder 세션이 소유하므로, 로컬 IPC 해제를 허용하면 서버 값만 지워져
///   holder 미러와 갈라진다.
/// - **mirror surface 도 거절**한다(ADR-0104 가 이 IPC 를 도입하는 트랙에 맡긴 집행).
///   미러의 attention 은 서버 push 만을 소스로 갖고(ADR-0098), 해제 forward 자격은
///   "그 화면을 실제로 본 주체"(실 렌더 포커스 · 미러 로컬 알림 읽음)에게만 있다 —
///   미러 인스턴스의 에이전트는 원격 surface 를 소유하지도, 그것을 보고 있지도 않다.
///   발동 축의 억제(ADR-0098)와 대칭이다.
pub(crate) fn handle_attention_clear(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if let Err(e) = require_existing_surface(engine, surface_id, &id) {
        return e;
    }
    let kind = match parse_optional_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    if engine.is_mirror_surface(surface_id) {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "Surface {surface_id} is a mirror of a remote attach session — its attention is \
                 pushed by the instance that owns the surface and cannot be cleared from here. \
                 Clear it on the owning instance (addressing the remote surface id there); a \
                 mirror user who actually looks at the surface clears it by focusing it, which \
                 is forwarded to the owner."
            ),
        );
    }
    if engine.attach.is_hard_occupied(surface_id) {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "Surface {surface_id} is occupied by a remote attach session (hard-occupied) — \
                 its attention state is owned by that session. Clear it from the attaching \
                 instance instead."
            ),
        );
    }
    let previous = engine.attention_kind(surface_id);
    let cleared = previous.is_some() && kind.is_none_or(|k| previous == Some(k));
    // 상태 변경은 **여기서** 적용한다. 라우터가 `surface_id` 로 owner engine 을 찾아
    // 넘겨줬고 위에서 소속을 재확인했으므로 대상이 확정적이며, 무엇보다 Intent 큐를
    // drain 하는 `App::dispatch_pending_intents` 는 gui 전용이라(`src/app.rs` 의
    // `#[cfg(feature = "gui")] mod dispatch;`) headless 인스턴스에서는 아래 cascade 가
    // 아예 존재하지 않는다 — enqueue 만 하면 headless 에는 해제 수단이 계속 0 개다.
    if cleared {
        engine.clear_attention(surface_id);
    }
    // cascade 는 gui 에서 소비처(테두리·탭·개수 배지) redraw 를 얹는다. 위에서 이미
    // 지웠으므로 cascade 의 재적용은 no-op 이고, cascade 는 IPC 를 타지 않는 호출자
    // (도메인 내부 producer)를 위해 자기 완결적으로 남는다.
    state.dispatch_intent(
        crate::core::intent::DomainIntent::SurfaceAttentionClear { surface_id, kind }
            .from_agent_ipc(),
    );
    JsonRpcResponse::success(
        id,
        json!({
            "ok": true,
            "surface_id": surface_id,
            "cleared": cleared,
            "previous_kind": previous.map(AttentionKind::to_wire),
        }),
    )
}

/// 대상 surface 가 라우팅된 engine 에 실제로 존재하는지 확인한다. IPC 라우터가
/// `surface_id` 로 owner engine(main → parked)을 먼저 찾아 넘겨주므로, 여기서
/// 없다면 어느 engine 에도 없다는 뜻이다.
fn require_existing_surface(
    engine: &crate::core::CoreState,
    surface_id: u32,
    id: &serde_json::Value,
) -> Result<(), JsonRpcResponse> {
    if engine.has_surface(surface_id) {
        return Ok(());
    }
    Err(JsonRpcResponse::invalid_params(
        id.clone(),
        format!("Surface {surface_id} not found"),
    ))
}

/// `kind` 파라미터(선택)를 파싱한다. 없거나 `null` 이면 `None`(필터 없음).
/// 문자열 어휘는 attach 스트림·`surface.completion` 과 공유한다.
fn parse_optional_kind(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<Option<AttentionKind>, JsonRpcResponse> {
    let raw = match params.get("kind") {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(v) => v,
    };
    serde_json::from_value::<AttentionKindWire>(raw.clone())
        .map(|w| Some(AttentionKind::from_wire(w)))
        .map_err(|_| {
            JsonRpcResponse::invalid_params(
                id.clone(),
                "'kind' must be \"completion\" or \"needs_input\"".to_string(),
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// mirror 워크스페이스의 surface 에 대한 IPC 해제는 거절된다(ADR-0104 가 이 IPC 를
    /// 도입하는 트랙에 맡긴 집행). 거절이 조용한 no-op 이 아니라 사유를 담은 에러여야
    /// 에이전트가 "지웠다" 고 오인하지 않는다.
    #[test]
    fn clear_is_rejected_for_a_mirror_surface() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        state.active_workspace_mut(&mut engine).mirror = true;

        let resp = handle_attention_clear(
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "surface_id": sid }),
        );
        let message = resp
            .error
            .expect("mirror surface 해제는 에러여야 한다")
            .message;
        assert!(
            message.contains("mirror"),
            "거절 사유가 mirror 임을 밝혀야 한다: {message}"
        );
        // 거절이므로 도메인 intent 도 발화되지 않는다 — cascade 가 뒤늦게 지우면
        // 거절의 의미가 없다.
        assert!(state.pending_intents.is_empty());
    }

    /// 조회는 mirror surface 에서도 허용된다 — 서버가 push 해 준 로컬 레코드를 읽는
    /// 것이라 소유권 문제가 없고, 미러 사용자/에이전트가 배지 상태를 확인할 수단이
    /// 사라지면 안 된다.
    #[test]
    fn get_is_allowed_for_a_mirror_surface() {
        let (state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        state.active_workspace_mut(&mut engine).mirror = true;

        let resp = handle_attention_get(&state, &engine, json!(1), &json!({ "surface_id": sid }));
        assert!(resp.error.is_none(), "{:?}", resp.error);
        assert_eq!(
            resp.result,
            Some(json!({ "surface_id": sid, "kind": null }))
        );
    }

    /// 비-mirror surface 는 그대로 해제된다(위 게이트가 일반 경로를 막지 않는지).
    #[test]
    fn clear_still_works_for_a_local_surface() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        engine.raise_attention(sid, AttentionKind::NeedsInput);

        let resp = handle_attention_clear(
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "surface_id": sid }),
        );
        assert!(resp.error.is_none(), "{:?}", resp.error);
        assert_eq!(
            resp.result,
            Some(json!({
                "ok": true,
                "surface_id": sid,
                "cleared": true,
                "previous_kind": "needs_input",
            }))
        );
        assert!(engine.attention_kind(sid).is_none());
    }
}
