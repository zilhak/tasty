//! 런타임에 등록된 surface kind를 조회한다. 호스트 내장 종류도 포함한다.
//! 매니페스트 선언만 읽지 않고 실제 SurfaceKindRegistry를 사용한다.

use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

pub(crate) fn handle_surface_kinds(
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mut kinds: Vec<serde_json::Value> = engine
        .surface_registry
        .kinds_snapshot()
        .into_iter()
        .map(|(kind, def)| {
            let required: Vec<&str> = def.required_params().collect();
            json!({
                "kind": kind,
                "display_name_i18n_key": def.display_name_i18n_key,
                "icon": def.icon,
                "rendering": def.rendering.as_str(),
                "source": def.source.as_str(),
                "plugin_id": def.source.plugin_id(),
                "required_params": required,
            })
        })
        .collect();
    // HashMap 순서가 매번 달라지지 않도록 이름순으로 반환한다.
    kinds.sort_by(|a, b| {
        a.get("kind")
            .and_then(|v| v.as_str())
            .cmp(&b.get("kind").and_then(|v| v.as_str()))
    });
    JsonRpcResponse::success(id, json!({ "kinds": kinds }))
}
