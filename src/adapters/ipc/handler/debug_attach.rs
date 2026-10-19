//! 로컬 사용자의 attach 강제 끊기를 재현하는 debug 전용 처리기.
//! release에는 원격 점유를 강제로 푸는 공개 메서드가 없고, 로컬 사용자는 GUI로 끊는다
//! (docs/principles/control-and-data.md). 헤드리스 debug 시험에서도 같은 해제를 재현한다.

#![cfg(debug_assertions)]

use crate::runtime::engine_access::EngineMut;
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// `debug.attach.force_detach` { surface_id } → 점유 테두리의 강제 끊기 버튼과 같은
/// 서버 쪽 해제와 holder 종료 통지.
pub(super) fn handle_force_detach(
    engine: &mut EngineMut<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let holder = engine.force_detach(surface_id);
    JsonRpcResponse::success(
        id,
        json!({
            "force_detached": holder.is_some(),
            "surface_id": surface_id,
            "holder": holder,
        }),
    )
}

/// `debug.attach.force_detach_workspace` { workspace_id } → 사이드바 확인 팝업과 같은
/// workspace 점유·멤버 제한 해제와 holder 종료 통지.
pub(super) fn handle_force_detach_workspace(
    engine: &mut EngineMut<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let workspace_id = match super::params::require_u32(params, "workspace_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let holder = engine.force_detach_workspace(workspace_id);
    JsonRpcResponse::success(
        id,
        json!({
            "force_detached": holder.is_some(),
            "workspace_id": workspace_id,
            "holder": holder,
        }),
    )
}
