//! `surface.kinds` — **등록된** surface kind 를 묻는 읽기 전용 조회.
//!
//! 이 조회가 있기 전에는 "이 kind 로 surface 를 만들 수 있는가" 를 묻는 방법이
//! **만들어 보는 것**뿐이었다 — 실패는 `unknown surface kind: <kind>` 로만 오고,
//! 성공하면 탭이나 workspace 가 생긴다. 관측하려고 부른 명령이 관측 대상을 바꾸는
//! 형태다. `plugin.show` 로는 답이 안 나온다: 그쪽이 내는 것은 매니페스트 **선언**
//! 이고, 등록 경로가 그 선언을 받아들였는지는 거기 안 실린다. host 내장 kind
//! (`terminal`/`empty`/`explorer`/`dag_graph`)는 plugin 이 아니라 어느 `plugin.*`
//! 조회에도 아예 안 나온다.
//!
//! 그래서 이 핸들러가 읽는 것은 매니페스트가 아니라
//! [`crate::core::surface_registry::SurfaceKindRegistry`] — 런타임의 사실이다.

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
    // `kinds_snapshot` 은 `HashMap` 순회라 순서가 실행마다 다르다. 조회 결과를
    // 사람이 읽고 스크립트가 diff 하므로 kind 이름으로 고정한다.
    kinds.sort_by(|a, b| {
        a.get("kind")
            .and_then(|v| v.as_str())
            .cmp(&b.get("kind").and_then(|v| v.as_str()))
    });
    JsonRpcResponse::success(id, json!({ "kinds": kinds }))
}
