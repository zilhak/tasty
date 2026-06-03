//! Debug 빌드 전용 plugin IPC 핸들러 — `debug.event_bus.*` + `debug.extension.invoke_hook`.
//!
//! PluginManager 의 EventBus 직접 조작 + extension hook 직접 fire. release 빌드에는
//! 컴파일되지 않는다 (handler.rs 의 mod 선언에 `#[cfg(debug_assertions)]`).

use crate::ipc::server::send_response;
use crate::plugin;
use tasty_ipc::protocol::JsonRpcResponse;

/// `debug.event_bus.*` IPC 처리.
pub(crate) fn handle_event_bus(
    mgr: Option<&mut plugin::PluginManager>,
    method: &str,
    params: &serde_json::Value,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    match method {
        "debug.event_bus.list_subscribers" => {
            let key = match params.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => return JsonRpcResponse::invalid_params(id, "missing 'key'"),
            };
            let subs = mgr.event_bus.debug_list_subscribers(&key);
            let result: Vec<_> = subs
                .into_iter()
                .map(|(plugin_id, sub_id, pattern)| {
                    serde_json::json!({
                        "plugin_id": plugin_id,
                        "sub_id": sub_id,
                        "pattern": pattern,
                    })
                })
                .collect();
            JsonRpcResponse::success(id, serde_json::json!({ "subscribers": result }))
        }
        "debug.event_bus.publish" => {
            let key = match params.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => return JsonRpcResponse::invalid_params(id, "missing 'key'"),
            };
            let payload_str = params
                .get("payload")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let payload: serde_json::Value = match serde_json::from_str(payload_str) {
                Ok(v) => v,
                Err(e) => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("invalid JSON payload: {e}"),
                    );
                }
            };
            let scope_str = params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("system");
            let scope = match scope_str {
                "system" => tasty_plugin_protocol::EventScope::System,
                "surface" => tasty_plugin_protocol::EventScope::Surface,
                other => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("unknown scope '{other}' (expected 'system' or 'surface')"),
                    );
                }
            };
            let envelope = mgr.build_host_envelope(&key, &payload, scope);
            let trace_id = envelope.meta.trace_id.clone();
            mgr.publish_host_event(envelope);
            JsonRpcResponse::success(
                id,
                serde_json::json!({ "published": true, "trace_id": trace_id }),
            )
        }
        "debug.event_bus.trace" => {
            let trace_id = match params.get("trace_id").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return JsonRpcResponse::invalid_params(id, "missing 'trace_id'"),
            };
            let envelopes = mgr.event_bus.debug_trace(trace_id);
            let result: Vec<_> = envelopes
                .into_iter()
                .map(|e| serde_json::to_value(&e).unwrap_or(serde_json::Value::Null))
                .collect();
            JsonRpcResponse::success(id, serde_json::json!({ "envelopes": result }))
        }
        _ => JsonRpcResponse::method_not_found(id, method),
    }
}

/// `debug.extension.invoke_hook` IPC. extension 에 hook 을 직접 fire 하고
/// 응답을 그대로 caller 에 회신한다. 비동기: response_tx 로 회신
/// (main loop 의 handle_plugin_response 가 처리).
pub(crate) fn handle_extension_invoke_hook(
    mgr: Option<&mut plugin::PluginManager>,
    params: &serde_json::Value,
    id: serde_json::Value,
    response_tx: std::sync::mpsc::SyncSender<JsonRpcResponse>,
) {
    let mgr = match mgr {
        Some(m) => m,
        None => {
            send_response(
                &response_tx,
                JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
            );
            return;
        }
    };
    let extension_id = match params.get("extension_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(id, "missing 'extension_id'"),
            );
            return;
        }
    };
    let kind = match params.get("kind").and_then(|v| v.as_str()) {
        Some("event") => tasty_plugin_protocol::ExtensionHookKind::Event,
        Some("ipc") => tasty_plugin_protocol::ExtensionHookKind::Ipc,
        _ => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(
                    id,
                    "missing/invalid 'kind' (expected 'event' or 'ipc')",
                ),
            );
            return;
        }
    };
    let phase = match params.get("phase").and_then(|v| v.as_str()) {
        Some("pre") => tasty_plugin_protocol::ExtensionHookPhase::Pre,
        Some("post") => tasty_plugin_protocol::ExtensionHookPhase::Post,
        _ => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(
                    id,
                    "missing/invalid 'phase' (expected 'pre' or 'post')",
                ),
            );
            return;
        }
    };
    let mode = match params.get("mode").and_then(|v| v.as_str()) {
        Some("transform") => crate::plugin::manifest::HookMode::Transform,
        Some("filter") => crate::plugin::manifest::HookMode::Filter,
        Some("observe") => crate::plugin::manifest::HookMode::Observe,
        _ => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(
                    id,
                    "missing/invalid 'mode' (expected 'transform', 'filter', or 'observe')",
                ),
            );
            return;
        }
    };
    let target = match params.get("target").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_response(
                &response_tx,
                JsonRpcResponse::invalid_params(id, "missing 'target'"),
            );
            return;
        }
    };
    let payload = params
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    mgr.debug_invoke_extension_hook(
        &extension_id,
        kind,
        phase,
        mode,
        &target,
        payload,
        id,
        response_tx,
    );
}
