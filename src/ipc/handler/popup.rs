//! Debug 전용 plugin popup IPC. 사용자 클릭 자동화 및 popup 디버깅용이라
//! release 빌드에는 노출되지 않는다.

#![cfg(debug_assertions)]

use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::plugin::PluginManager;

/// `debug.popup.list` — 매니페스트로 contribute된 popup 목록 + 현재 열린 인스턴스.
pub fn handle_list(mgr: Option<&PluginManager>, id: serde_json::Value) -> JsonRpcResponse {
    let Some(mgr) = mgr else {
        return JsonRpcResponse::success(
            id,
            json!({ "contributes": [], "instances": [] }),
        );
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
                "has_tree": inst.tree.is_some(),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "contributes": contributes, "instances": instances }))
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
            &format!("popup '{plugin_id}/{popup_id}' not found or plugin not running"),
        ),
    }
}

/// `debug.popup.close` — `{ instance_id }`로 popup 인스턴스 강제 close.
pub fn handle_close(
    mgr: Option<&mut PluginManager>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(instance_id) = params.get("instance_id").and_then(|v| v.as_u64()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'instance_id' parameter");
    };
    let Some(mgr) = mgr else {
        return JsonRpcResponse::error(id, -32002, "plugin manager not initialized");
    };
    mgr.close_popup_instance(
        instance_id,
        tasty_plugin_protocol::PopupCloseReason::PluginRequest,
    );
    JsonRpcResponse::success(id, json!({ "closed": instance_id }))
}
