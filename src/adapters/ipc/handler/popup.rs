//! Debug 전용 plugin popup IPC. 사용자 클릭 자동화 및 popup 디버깅용이라
//! release 빌드에는 노출되지 않는다.

#![cfg(debug_assertions)]

use serde_json::json;

use crate::plugin::PluginManager;
use tasty_ipc::protocol::JsonRpcResponse;

/// `debug.popup.list` — 매니페스트로 contribute된 popup 목록 + 현재 열린 인스턴스.
pub fn handle_list(mgr: Option<&PluginManager>, id: serde_json::Value) -> JsonRpcResponse {
    let Some(mgr) = mgr else {
        return JsonRpcResponse::success(id, json!({ "contributes": [], "instances": [] }));
    };
    let contributes: Vec<_> = mgr
        .plugin_popup_contributes()
        .into_iter()
        .map(|entry| {
            let trigger = match &entry.contribute.trigger {
                crate::plugin::manifest::PopupTrigger::Event { event_key } => {
                    json!({ "kind": "event", "event_key": event_key })
                }
                crate::plugin::manifest::PopupTrigger::Ipc => json!({ "kind": "ipc" }),
            };
            json!({
                "plugin_id": entry.plugin_id,
                "popup_id": entry.contribute.id,
                "trigger": trigger,
            })
        })
        .collect();
    let instances: Vec<_> = mgr
        .popup_instances()
        .map(|(inst_id, inst)| {
            json!({
                "instance_id": inst_id,
                "plugin_id": inst.plugin_id,
                "popup_id": inst.popup_id,
                // z_seq 는 host popup 과 공유하는 전역 시퀀스라 `debug.host_popup.list`
                // 의 값과 직접 비교할 수 있다 — 겹친 popup 의 상하 관계 관찰면.
                "z_seq": inst.z_seq,
            })
        })
        .collect();
    JsonRpcResponse::success(
        id,
        json!({ "contributes": contributes, "instances": instances }),
    )
}

/// `debug.popup.open` — `{ plugin_id, popup_id, context? }`로 popup 인스턴스 강제 open.
pub fn handle_open(
    mgr: Option<&mut PluginManager>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(plugin_id) = params.get("plugin_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'plugin_id' parameter");
    };
    let Some(popup_id) = params.get("popup_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'popup_id' parameter");
    };
    let context = params
        .get("context")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let Some(mgr) = mgr else {
        return JsonRpcResponse::error(id, -32002, "plugin manager not initialized");
    };
    match mgr.open_popup_instance(plugin_id, popup_id, context) {
        Some(instance_id) => JsonRpcResponse::success(id, json!({ "instance_id": instance_id })),
        None => JsonRpcResponse::error(
            id,
            -32602,
            format!("popup '{plugin_id}/{popup_id}' not found or plugin not running"),
        ),
    }
}

// `debug.popup.close` 는 여기 없다 — 매니저 직접 close 가 아니라 렌더가 수집하는
// close 큐로 합류해야 `cancel_child_file_picker` 연쇄 정리가 돌기 때문에(ADR-0082)
// `App::enqueue_plugin_popup_close` 를 거치는 App-level glue 로 옮겼다
// (`src/app/ipc/debug_methods.rs`, `debug.plugin_banner.*` 와 같은 이유·같은 위치).
