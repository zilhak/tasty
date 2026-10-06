use serde_json::json;

use crate::app::services::AppServices;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 열린 surface에만 쓴다. 라우터도 대상 surface가 없는 요청을 거절하지만, 이 핸들러가 다른
/// 경로로 불려도 닫힌 surface에 metadata가 남지 않도록 여기서 한 번 더 확인한다.
pub fn handle_surface_meta_set(
    core: &AppServices,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if !engine.has_surface(surface_id) {
        return JsonRpcResponse::invalid_params(id, format!("Surface {surface_id} not found"));
    }
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let value = match params.get("value").and_then(|v| v.as_str()) {
        Some(v) => v,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'value' parameter"),
    };
    let result =
        core.with_memory(|m| crate::surface_meta::SurfaceMetaStore::set(m, surface_id, key, value));
    if let Err(e) = result {
        return JsonRpcResponse::internal_error(id, format!("surface meta set failed: {e}"));
    }
    crate::adapters::ipc::handler::memory::written(
        core,
        id,
        json!({ "ok": true, "surface_id": surface_id }),
    )
}

pub fn handle_surface_meta_get(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let value =
        core.with_memory(|m| crate::surface_meta::SurfaceMetaStore::get(m, surface_id, key));
    JsonRpcResponse::success(id, json!({ "value": value, "surface_id": surface_id }))
}

pub fn handle_surface_meta_unset(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let result =
        core.with_memory(|m| crate::surface_meta::SurfaceMetaStore::unset(m, surface_id, key));
    if let Err(e) = result {
        return JsonRpcResponse::internal_error(id, format!("surface meta unset failed: {e}"));
    }
    crate::adapters::ipc::handler::memory::written(
        core,
        id,
        json!({ "ok": true, "surface_id": surface_id }),
    )
}

pub fn handle_surface_meta_list(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let data = core.with_memory(|m| crate::surface_meta::SurfaceMetaStore::list(m, surface_id));
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "data": data }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tasty_ipc::caller::CallerContext;
    use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
    use tasty_memory::Scope;

    use crate::app::services::AppServices;

    fn set(
        core: &mut AppServices,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
        state: &mut crate::state::RequestContext,
        surface_id: u32,
    ) -> JsonRpcResponse {
        let req = JsonRpcRequest {
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: None,
            session_token: None,
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "surface.meta.set".into(),
            params: json!({ "surface_id": surface_id, "key": "role", "value": "x" }),
        };
        super::super::handle_with_caller(core, state, engine, &req, &CallerContext::local())
    }

    /// 라우터를 거치지 않고 핸들러를 직접 불러도 없는 surface에는 쓰지 않는다.
    #[test]
    fn only_an_open_surface_takes_metadata() {
        let store = tasty_memory::MemoryStore::open_in_memory().expect("store");
        let mut core = super::super::cli_entry_tests::test_core_builder()
            .with_memory(std::sync::Arc::new(std::sync::Mutex::new(store)))
            .build()
            .expect("core");
        let (mut state, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let live: Vec<u32> = engine
            .workspaces()
            .into_iter()
            .flat_map(|ws| ws.all_surface_ids())
            .collect();
        let open = *live.first().expect("fixture 에 열린 surface 가 있다");
        let gone = live.iter().max().copied().unwrap_or(open) + 1;

        let refused = set(&mut core, &mut engine, &mut state, gone);
        let error = refused.error.expect("없는 surface 에 쓰기가 거절돼야 한다");
        assert_eq!(error.code, -32602, "{}", error.message);
        assert_eq!(error.message, format!("Surface {gone} not found"));
        let left = core.with_memory(|m| m.get(&Scope::Surface(gone), "role"));
        assert!(
            matches!(left, Ok(None)),
            "거절한 쓰기가 저장소에 남았다: {left:?}"
        );

        let written = set(&mut core, &mut engine, &mut state, open);
        assert!(
            written.error.is_none(),
            "열린 surface 쓰기: {:?}",
            written.error
        );
        let stored = core.with_memory(|m| m.get(&Scope::Surface(open), "role"));
        assert!(
            matches!(stored, Ok(Some(_))),
            "열린 surface 의 값이 저장되지 않았다: {stored:?}"
        );
    }
}
