//! `surface.set_cwd` IPC 핸들러 — plugin 이 자기 RemoteSurface 의 cwd 를 host 에 통보.
//!
//! explorer 가 root 변경 시 carry 후보 cwd 갱신, 이후 새 surface 생성 시
//! resolve_inherit_cwd_from_surface 경로에서 자동 활용.
//! webview::handle_set_url 과 동일 패턴 — `&CoreState` (immutable) 로 충분.

use serde_json::Value;
use std::path::PathBuf;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

pub fn handle_set_cwd(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match params.get("surface_id").and_then(|v| v.as_u64()) {
        Some(s) => s as u32,
        None => return JsonRpcResponse::invalid_params(id, "missing 'surface_id'"),
    };
    let cwd = match params.get("cwd") {
        Some(Value::Null) | None => None,
        Some(Value::String(s)) => Some(PathBuf::from(s)),
        Some(_) => return JsonRpcResponse::invalid_params(id, "'cwd' must be string or null"),
    };

    for ws in &engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    let surface = tab.surface();
                    if surface.surface_id() != Some(sid) {
                        continue;
                    }
                    if let Some(rs) = surface
                        .as_any()
                        .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
                    ) {
                        rs.set_cwd(cwd);
                        return JsonRpcResponse::success(id, serde_json::json!({ "ok": true }));
                    }
                    return JsonRpcResponse::error(id, -32000, "surface is not a RemoteSurface");
                }
            }
        }
    }
    JsonRpcResponse::error(id, -32000, "surface_id not found")
}
