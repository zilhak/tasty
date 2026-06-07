//! `attach.*` IPC — 배타 attach 점유 제어(획득/해제/강제/조회).
//!
//! `session.*`(자식 agent 신원 토큰)와 별도 네임스페이스 — 충돌 회피. decision 5:
//! 자체 token 없음(SSH + 127.0.0.1 loopback 위임). `client_id` 는 단계 1 stream
//! 핸드셰이크(`stream.open`)가 발급한 StreamClientId 로, 클라가 ack 로 받아 파라미터로
//! 전달한다.
//!
//! 원칙 3(포커스 독립): 대상 surface 를 ID 로 직접 지정 — 포커스 상태에 의존하지
//! 않는다. force-detach 는 사용자의 포커스/닫힌항목 히스토리를 건드리지 않는다
//! (원칙 1①). attach 제어는 *에이전트 행동*(ID 지정·입력 시뮬레이션 아님)이라 release
//! 빌드에 노출된다.

use serde_json::json;

use crate::core::CoreState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

fn require_client_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("client_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing required 'client_id' parameter"))
}

/// `attach.acquire` { surface_id, client_id } → 배타 lock 획득(동시 attach 거부).
pub(crate) fn handle_acquire(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let client_id = match require_client_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // 실재하는(또는 deferred) 터미널 surface 만 점유 대상. 없는 id 점유 방지.
    if !engine.terminals.contains(surface_id) && !engine.is_surface_deferred(surface_id) {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Surface {surface_id} not found or not attachable"),
        );
    }
    match engine.attach.acquire(surface_id, client_id) {
        Ok(lock) => JsonRpcResponse::success(
            id,
            json!({
                "attached": true,
                "surface_id": surface_id,
                "holder": lock.holder,
                "granted_seq": lock.granted_seq,
            }),
        ),
        Err(crate::core::attach::AttachError::AlreadyAttached { holder }) => {
            JsonRpcResponse::error(id, -32020, format!("already attached by client {holder}"))
        }
        Err(e) => JsonRpcResponse::error(id, -32020, format!("{e:?}")),
    }
}

/// `attach.release` { surface_id, client_id } → 정상 해제(holder 본인).
pub(crate) fn handle_release(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let client_id = match require_client_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match engine.attach.release(surface_id, client_id) {
        Ok(()) => {
            JsonRpcResponse::success(id, json!({ "released": true, "surface_id": surface_id }))
        }
        Err(e) => JsonRpcResponse::error(id, -32021, format!("{e:?}")),
    }
}

/// `attach.force_detach` { surface_id } → 서버 권한 강제 해제 + holder 종료 통지.
pub(crate) fn handle_force_detach(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let holder = engine.attach.force_detach(surface_id);
    JsonRpcResponse::success(
        id,
        json!({
            "force_detached": holder.is_some(),
            "surface_id": surface_id,
            "holder": holder,
        }),
    )
}

/// `attach.list` → 전 점유 목록(포커스 독립). free/점유 디스커버리(design §5.3).
pub(crate) fn handle_list(engine: &CoreState, id: serde_json::Value) -> JsonRpcResponse {
    let arr: Vec<_> = engine
        .attach
        .locks_snapshot()
        .into_iter()
        .map(|(sid, l)| {
            json!({
                "surface_id": sid,
                "holder": l.holder,
                "granted_seq": l.granted_seq,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "attached": arr }))
}
