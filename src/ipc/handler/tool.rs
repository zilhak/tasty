//! Debug 전용 도구 메뉴 IPC. 사용자 클릭을 자동화로 재현하는 디버그 기능이므로
//! release 빌드에 노출되지 않는다. `#[cfg(debug_assertions)]`로 감싸 컴파일 자체를
//! debug 빌드에서만 한다.

#![cfg(debug_assertions)]

use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::plugin::manifest::ToolAction;
use crate::plugin::tool_registry::ToolSource;
use crate::state::AppState;

/// `debug.tool.list` — 현재 도구 메뉴에 표시되는 모든 항목을 정렬된 순서로 반환.
pub fn handle_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let items: Vec<_> = state
        .tool_registry
        .visible_items()
        .into_iter()
        .map(|item| {
            let ToolSource::Plugin {
                plugin_id,
                tool_id,
            } = &item.source;
            let source = json!({
                "kind": "plugin",
                "plugin_id": plugin_id,
                "tool_id": tool_id,
            });
            let action = match &item.action {
                ToolAction::Event { event_key } => {
                    json!({ "kind": "event", "event_key": event_key })
                }
                ToolAction::OpenSurface { surface_kind } => {
                    json!({ "kind": "open_surface", "surface_kind": surface_kind })
                }
                ToolAction::OpenPopup { popup_id } => {
                    json!({ "kind": "open_popup", "popup_id": popup_id })
                }
            };
            json!({
                "key": item.key,
                "source": source,
                "label_i18n_key": item.label_i18n_key,
                "icon": item.icon,
                "action": action,
                "order_hint": item.order_hint,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "items": items }))
}

/// `debug.tool.invoke` — `params.key`로 항목을 찾아 사용자 클릭과 동일한 동작을 수행.
/// 항목을 찾지 못하면 invalid_params로 거부.
pub fn handle_invoke(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(key) = params.get("key").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'key' parameter");
    };
    let Some(item) = state.tool_registry.find(key) else {
        return JsonRpcResponse::error(
            id,
            -32602,
            &format!("tool item '{key}' not found"),
        );
    };
    crate::ui::tools_menu::invoke_tool(state, &item);
    JsonRpcResponse::success(id, json!({ "invoked": key }))
}
