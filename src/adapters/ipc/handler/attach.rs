//! attach 점유의 획득·해제·조회. 대상은 ID로 지정하며 사용자 포커스를 바꾸지 않는다.
//! 별도 토큰 대신 SSH와 loopback 연결을 신뢰한다. client_id는 stream.open에서 발급한다.

use super::params::{self, p_try};
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

/// workspace의 점유와 멤버 제한을 해제하고 holder에게 알린다.
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

/// 지정한 port와 workspace의 mirror 생성을 GUI 큐에 요청한다.
/// 실제 연결·생성은 App이 처리하며 헤드리스는 이 큐를 처리하지 않는다.
pub(crate) fn handle_into_gui(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let port = match p_try!(params::opt_int::<u64>(params, "port", &id)) {
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

/// surface와 workspace 점유 목록을 함께 반환한다.
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
