use serde_json::json;
use tasty_ipc::stream::AttentionKindWire;

use crate::core::AttentionKind;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 명시한 surface의 attention을 completion/needs_input/null로 반환한다.
pub(crate) fn handle_attention_get(
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

/// 명시한 surface의 attention을 지운다. kind가 있으면 같은 종류만 지우고 잘못된 kind는 거절한다.
/// 더 높은 우선순위의 새 알림을 늦게 도착한 해제 요청이 지우지 않게 하기 위해서다.
/// 원래 알림이 없어도 성공하며 cleared로 실제 삭제 여부를 알린다.
/// hard 점유와 mirror surface는 거절한다. 원격 알림은 소유 인스턴스가 관리하며,
/// mirror의 해제 전달은 실제 포커스나 로컬 알림 읽음 같은 사용자 확인에서만 허용한다(ADR-0024).
pub(crate) fn handle_attention_clear(
    out: &mut crate::ipc::window_port::IntentOutbox,
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
    // 헤드리스에도 즉시 반영한다. GUI 전용 intent 후속 처리만 기다리면 해제되지 않는다.
    if cleared {
        engine.clear_attention(surface_id);
    }
    // GUI는 후속 처리에서 화면을 갱신한다. 이미 지운 상태를 다시 지워도 결과는 같다.
    out.push(
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

/// 라우터가 선택한 engine에 대상이 있는지 확인한다.
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

    #[test]
    fn clear_is_rejected_for_a_mirror_surface() {
        let (state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        state.active_workspace_mut(&mut engine).mirror = true;

        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let resp = handle_attention_clear(
            &mut out,
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
        assert!(out.is_empty());
    }

    // mirror 조회는 서버가 보낸 로컬 기록을 읽을 뿐이므로 허용한다.
    #[test]
    fn get_is_allowed_for_a_mirror_surface() {
        let (state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        state.active_workspace_mut(&mut engine).mirror = true;

        let resp = handle_attention_get(&engine, json!(1), &json!({ "surface_id": sid }));
        assert!(resp.error.is_none(), "{:?}", resp.error);
        assert_eq!(
            resp.result,
            Some(json!({ "surface_id": sid, "kind": null }))
        );
    }

    #[test]
    fn clear_still_works_for_a_local_surface() {
        let (state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        engine.raise_attention(sid, AttentionKind::NeedsInput);

        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let resp = handle_attention_clear(
            &mut out,
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
