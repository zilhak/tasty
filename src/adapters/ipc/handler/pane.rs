use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

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

/// 숫자 ID로 surface를 찾는다. 별칭은 게이트가 ID로 바꿔 둔다. 점유 검사도 같은 해석을 사용한다.
pub(crate) fn resolve_surface_target(params: &serde_json::Value) -> Option<u32> {
    let val = params.get("target_surface")?;
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
        // nickname은 요청 게이트가 접수할 때 찾아 surface ID로 바꿔 둔다. 남은 문자열은 찾지 못한 대상이다.
        return s.parse::<u32>().ok();
    }
    None
}
