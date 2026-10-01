use serde_json::json;

use crate::runtime::engine_access::EngineMut;
use crate::model::SplitDirection;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_pane_id;

pub fn handle_pane_list(
    presentation: &(impl crate::model::StructurePresentation + ?Sized),
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mut panes = Vec::new();
    for ws in &engine.workspaces() {
        let pane_ids = ws.pane_layout().all_pane_ids();
        let focused = presentation.pane_id(ws).unwrap_or(0);
        for &pid in &pane_ids {
            let tab_count = ws
                .pane_layout()
                .find_pane(pid)
                .map(|p| p.tabs.len())
                .unwrap_or(0);
            panes.push(json!({
                "id": pid,
                "workspace_id": ws.id,
                "workspace_name": ws.name,
                "focused": pid == focused,
                "tab_count": tab_count,
            }));
        }
    }
    JsonRpcResponse::success(id, json!(panes))
}

/// 숫자 ID나 별칭으로 surface를 찾는다. 점유 검사도 같은 해석을 사용한다.
pub(crate) fn resolve_surface_target(
    core: &crate::app::services::AppServices,
    params: &serde_json::Value,
) -> Option<u32> {
    let val = params.get("target_surface");
    let val = val?;
    if val.is_null() {
        return None;
    }
    // 범위 초과 값을 자르면 다른 surface를 가리킬 수 있으므로 거절한다.
    if val.is_number() {
        return crate::adapters::ipc::handler::params::read_int::<u32>(params, "target_surface")
            .ok()
            .flatten();
    }
    if let Some(s) = val.as_str() {
        if s.is_empty() {
            return None;
        }
        if let Ok(n) = s.parse::<u32>() {
            return Some(n);
        }
        return core.with_memory(|m| {
            crate::surface_meta::SurfaceMetaStore::find_by_value(m, "nickname", s)
        });
    }
    None
}

