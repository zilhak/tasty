//! 플러그인이 RemoteSurface의 cwd를 갱신한다. 이후 새 surface의 작업 폴더 선택에 사용한다.

use crate::adapters::ipc::handler::params::require_u32;
use serde_json::Value;
use std::path::PathBuf;

use tasty_ipc::protocol::JsonRpcResponse;

pub fn handle_set_cwd(
    engine: &crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match require_u32(params, "surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
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
