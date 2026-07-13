//! `settings.*` IPC 핸들러 — plugin 이 자기 자신의 `plugin_settings` 값을
//! 런타임에 read-back. `plugin_id` 는 요청 파라미터로 받지 않고
//! `CallerContext` 에서 강제 도출한다(다른 plugin 값 조회 불가).

use serde_json::{Value, json};

use crate::core::CoreState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

/// `settings.get_plugin_setting { storage_key }` →
/// `{ "value": <PluginSettingValue as JSON> | null }`.
///
/// caller 가 `CallerContext::Plugin` 이 아니면(Local/Agent) — 이번 TODO 는
/// "plugin 자기 설정 read-back" 전용이라 Local/Agent 호출은 항상 값 없음으로
/// 취급한다. `caller.owner()` 를 그대로 재사용(memory.rs/secret.rs 와 동일
/// 관례) — Local 은 `HOST_OWNER`("_host") 로 조회되므로 plugin_settings 맵에
/// 해당 키가 존재할 수 없어 자연히 `null` 이 반환된다. 별도 거부 분기를
/// 두지 않는 편이 기존 owner() 관례와 일관되고 더 단순하다.
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
    fn plugin_cannot_read_another_plugins_setting() {
        let mut e = engine();
        e.settings.set_plugin_setting(
            "com.tasty.codex",
            "spawn_child_warn_threshold",
            PluginSettingValue::Number(3.0),
        );
        let caller = plugin_caller("com.tasty.claude");
        // 요청에 plugin_id 파라미터 자체가 없다 — caller 로 강제 스코프됨을
        // 검증하는 게 핵심이라, params 에 넣어봐도 무시돼야 한다.
        let resp = handle_get_plugin_setting(
            &e,
            &caller,
            json!(1),
            &json!({ "storage_key": "spawn_child_warn_threshold", "plugin_id": "com.tasty.codex" }),
        );
        assert_eq!(resp.result.unwrap()["value"], Value::Null);
    }
}
