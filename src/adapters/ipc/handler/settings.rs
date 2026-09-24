//! 요청의 plugin_id 대신 CallerContext의 owner로 플러그인 설정을 조회한다.

use super::params::{self, p_try};
use serde_json::{Value, json};

use crate::core::CoreState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

/// caller.owner()와 storage_key에 해당하는 값을 반환한다. 없으면 null이다.
/// Local은 _host, Plugin은 plugin_id, Agent는 agent_id를 키로 쓰며 별도 타입 거절은 없다.
pub fn handle_get_plugin_setting(
    engine: &CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(storage_key) = params.get("storage_key").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing 'storage_key'");
    };
    let plugin_id = caller.owner();
    let value = engine
        .settings
        .plugin_setting(plugin_id, storage_key)
        .and_then(|v| serde_json::to_value(v).ok());
    JsonRpcResponse::success(id, json!({ "value": value }))
}

/// `settings.get_remote_transfer {}` → `RemoteTransferSettings` 직렬화
/// (`{ "dir": <string>, "max_mb": <u64> }`). 원격 전송 저장 정책 조회.
pub fn handle_get_remote_transfer(engine: &CoreState, id: Value) -> JsonRpcResponse {
    match serde_json::to_value(&engine.settings.remote_transfer) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32603, format!("failed to serialize settings: {e}")),
    }
}

/// 현재 설정의 사본에 지정 필드만 합쳐 UpdateSettings로 적용한다.
/// 원본을 미리 바꾸면 이전 값과의 차이를 잃으므로 후속 처리에서 저장까지 맡긴다.
pub fn handle_set_remote_transfer(
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mut new_settings = engine.settings.clone();
    let mut changed = false;

    if let Some(dir) = params.get("dir") {
        let Some(dir) = dir.as_str() else {
            return JsonRpcResponse::invalid_params(id, "'dir' must be a string");
        };
        new_settings.remote_transfer.dir = dir.to_string();
        changed = true;
    }
    if params.get("max_mb").is_some_and(|v| !v.is_null()) {
        let Some(max_mb) = p_try!(params::opt_int::<u64>(params, "max_mb", &id)) else {
            return JsonRpcResponse::invalid_params(id, "'max_mb' must be a non-negative integer");
        };
        new_settings.remote_transfer.max_mb = max_mb;
        changed = true;
    }
    if !changed {
        return JsonRpcResponse::invalid_params(id, "expected at least one of 'dir', 'max_mb'");
    }

    let applied = serde_json::to_value(&new_settings.remote_transfer).unwrap_or(Value::Null);
    out.push(crate::core::intent::DomainIntent::UpdateSettings(new_settings).from_agent_ipc());
    JsonRpcResponse::success(id, json!({ "applied": true, "remote_transfer": applied }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tasty_settings::PluginSettingValue;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn plugin_caller(plugin_id: &str) -> CallerContext {
        CallerContext::Plugin {
            plugin_id: plugin_id.to_string(),
            permissions: Arc::new(HashSet::new()),
        }
    }

    #[test]
    fn plugin_can_read_back_its_own_stored_setting() {
        let mut e = engine();
        e.settings.set_plugin_setting(
            "com.tasty.claude",
            "spawn_child_warn_threshold",
            PluginSettingValue::Number(8.0),
        );
        let caller = plugin_caller("com.tasty.claude");
        let resp = handle_get_plugin_setting(
            &e,
            &caller,
            json!(1),
            &json!({ "storage_key": "spawn_child_warn_threshold" }),
        );
        assert_eq!(resp.result.unwrap()["value"], json!(8.0));
    }

    #[test]
    fn unset_setting_returns_null_not_error() {
        let e = engine();
        let caller = plugin_caller("com.tasty.claude");
        let resp = handle_get_plugin_setting(
            &e,
            &caller,
            json!(1),
            &json!({ "storage_key": "never_stored" }),
        );
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["value"], Value::Null);
    }

    #[test]
    fn get_remote_transfer_returns_defaults() {
        let e = engine();
        let resp = handle_get_remote_transfer(&e, json!(1));
        assert!(resp.error.is_none());
        let v = resp.result.unwrap();
        assert_eq!(v["dir"], json!(""));
        assert_eq!(v["max_mb"], json!(500));
    }

    #[test]
    fn get_remote_transfer_reflects_live_settings() {
        let mut e = engine();
        e.settings.remote_transfer.dir = "/tmp/xfer".to_string();
        e.settings.remote_transfer.max_mb = 42;
        let resp = handle_get_remote_transfer(&e, json!(1));
        let v = resp.result.unwrap();
        assert_eq!(v["dir"], json!("/tmp/xfer"));
        assert_eq!(v["max_mb"], json!(42));
    }

    #[test]
    fn plugin_cannot_read_another_plugins_setting() {
        let mut e = engine();
        e.settings.set_plugin_setting(
            "com.tasty.codex",
            "spawn_child_warn_threshold",
            PluginSettingValue::Number(3.0),
        );
        let caller = plugin_caller("com.tasty.claude");
        // 요청의 plugin_id로 다른 플러그인 설정을 읽을 수 없어야 한다.
        let resp = handle_get_plugin_setting(
            &e,
            &caller,
            json!(1),
            &json!({ "storage_key": "spawn_child_warn_threshold", "plugin_id": "com.tasty.codex" }),
        );
        assert_eq!(resp.result.unwrap()["value"], Value::Null);
    }
}
