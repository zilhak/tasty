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
    super::params::require_u32(params, "client_id", id)
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

/// `attach.force_detach_workspace` { workspace_id } → workspace 단위 강제 해제(단계 6).
/// 멤버 터미널 lock + 비-터미널 숨김을 일괄 free 환원하고 holder 에게 종료 통지.
pub(crate) fn handle_force_detach_workspace(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let workspace_id = match super::params::require_u32(params, "workspace_id", &id) {
        Ok(v) => v,
        Err(e) => {
            return e;
        }
    };
    let holder = engine.attach.force_detach_workspace(workspace_id);
    JsonRpcResponse::success(
        id,
        json!({
            "force_detached": holder.is_some(),
            "workspace_id": workspace_id,
            "holder": holder,
        }),
    )
}

/// `attach.into_gui` { port, workspace } → 이 GUI 인스턴스가 *client* 로서 loopback
/// `port` 의 원격 tasty `workspace` 를 mirror 로 재구성하도록 트리거(작업 J B2).
/// 실제 연결/재구성은 App 이 `about_to_wait` 에서 큐를 drain 해 수행(스레드·뷰 접근
/// 필요 — 핸들러는 engine 만 본다). headless 는 GUI 가 없어 drain 되지 않는다.
///
/// 원칙 3(포커스 독립): 대상을 ID(port/workspace)로 직접 지정. 단계 7 자동 매핑의
/// 수동 트리거 버전 — 자동 attach 결선은 단계 7.
pub(crate) fn handle_into_gui(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let port = match params.get("port").and_then(|v| v.as_u64()) {
        Some(v) if v <= u16::MAX as u64 => v as u16,
        _ => return JsonRpcResponse::invalid_params(id, "Missing/invalid 'port' parameter"),
    };
    let workspace = match super::params::require_u32(params, "workspace", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    engine.pending_gui_attach.push((port, workspace));
    JsonRpcResponse::success(
        id,
        json!({ "queued": true, "port": port, "workspace": workspace }),
    )
}

/// `attach.list` → 전 점유 목록(포커스 독립). free/점유 디스커버리(design §5.3).
/// surface 단위(단계 4)와 workspace 단위(단계 6) 점유를 함께 보고한다.
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
    let workspaces: Vec<_> = engine
        .attach
        .workspaces_snapshot()
        .into_iter()
        .map(|(ws, l)| {
            json!({
                "workspace_id": ws,
                "holder": l.holder,
                "granted_seq": l.granted_seq,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "attached": arr, "workspaces": workspaces }))
}
