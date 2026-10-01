use serde_json::json;

use super::params::{self, p_try};
use crate::runtime::engine_access::EngineMut;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_pane_id;

fn require_tab_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params::opt_int::<u32>(params, "tab_id", id)?.ok_or_else(|| {
        JsonRpcResponse::invalid_params(id.clone(), "Missing required 'tab_id' parameter")
    })
}

pub fn handle_tab_list(
    presentation: &(impl crate::model::StructurePresentation + ?Sized),
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let pane_id = match require_pane_id(params, &id) {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    let tabs: Vec<_> = if let Some(pane) = engine.find_pane_by_id(pane_id) {
        pane.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let surface = presentation
                    .surface_id(tab)
                    .and_then(|id| engine.find_surface_by_id(id));
                let surface_type = surface.map(|s| s.type_name()).unwrap_or("Empty");
                let surface_id = surface.and_then(|s| s.surface_id());
                let sids = tab.all_surface_ids();
                let mut entry = json!({
                    "id": tab.id,
                    "name": tab.name,
                    "active": i == presentation.tab_index(pane),
                    "type": surface_type,
                    "busy_count": engine.read().busy_count(&sids),
                });
                if let Some(sid) = surface_id {
                    entry["surface_id"] = json!(sid);
                }
                entry
            })
            .collect()
    } else {
        return JsonRpcResponse::invalid_params(id, format!("Pane {} not found", pane_id));
    };
    JsonRpcResponse::success(id, json!({ "pane_id": pane_id, "tabs": tabs }))
}
