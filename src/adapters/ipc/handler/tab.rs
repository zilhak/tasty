use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use super::require_pane_id;

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
                let surface_type = surface.map(tab_surface_type).unwrap_or("Empty");
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

/// plugin kind 등록을 기다리는 자리(`EmptySurface`의 deferred)는 type_name이 `Empty`지만
/// `surface.list`·`list tree`처럼 `Pending`으로 보고한다. type_name은 탭 제목에도 쓰여 바꾸지 않는다.
fn tab_surface_type(surface: &dyn crate::model::Surface) -> &'static str {
    let plugin_wait = surface
        .as_any()
        .downcast_ref::<crate::model::EmptySurface>()
        .is_some_and(|empty| empty.is_deferred());
    if plugin_wait {
        "Pending"
    } else {
        surface.type_name()
    }
}

#[cfg(test)]
mod tests {
    use tasty_core::{DomainEvent as E, SurfaceSpec};

    fn markdown(id: u32) -> SurfaceSpec {
        SurfaceSpec {
            id,
            kind: "markdown".into(),
            data: None,
        }
    }

    /// plugin 대기 자리 탭은 Pending, 비활성 빈 surface 탭은 Empty로 보고한다.
    #[test]
    fn plugin_wait_tab_reports_pending_and_empty_tab_stays_empty() {
        let model = crate::state::tests::test_model(vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::WorkspaceCreated {
                id: 1,
                name: "w".into(),
                category: 0,
                index: 0,
                pane: 10,
            },
            E::TabCreated {
                id: 100,
                pane: 10,
                index: 0,
                name: "wait".into(),
                surface: markdown(11),
            },
            E::TabCreated {
                id: 101,
                pane: 10,
                index: 1,
                name: "empty".into(),
                surface: markdown(12),
            },
        ]);
        let (_state, mut session) = crate::state::tests::test_state_from_model(model);
        session.runtime.surfaces.insert(
            11,
            Box::new(crate::model::EmptySurface::new_deferred_plugin(
                11,
                crate::model::DeferredPlugin {
                    kind: "markdown".into(),
                    snapshot: serde_json::json!({ "file": "/a.md" }),
                },
            )),
        );
        session
            .runtime
            .surfaces
            .insert(12, Box::new(crate::model::EmptySurface::new(12)));
        let engine = session.borrow_mut();
        let presentation = crate::state::navigation::NavigationState::default();
        let response = super::handle_tab_list(
            &presentation,
            &engine.as_ref(),
            serde_json::json!(1),
            &serde_json::json!({ "pane_id": 10 }),
        );
        let result = response.result.expect("tab.list result");
        let tab = |id: u32| {
            result["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == id)
                .unwrap_or_else(|| panic!("tab {id} missing: {result}"))
                .clone()
        };
        assert_eq!(tab(100)["type"], "Pending", "{result}");
        assert_eq!(tab(101)["type"], "Empty", "{result}");
    }
}
