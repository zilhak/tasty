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

/// `settings.get_remote_transfer {}` → `RemoteTransferSettings` 직렬화
/// (`{ "dir": <string>, "max_mb": <u64> }`). 07 원격 전송 저장 정책 조회.
pub fn handle_get_remote_transfer(engine: &CoreState, id: Value) -> JsonRpcResponse {
    match serde_json::to_value(&engine.settings.remote_transfer) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32603, format!("failed to serialize settings: {e}")),
    }
}

/// `settings.set_remote_transfer { dir?, max_mb? }` — 제공된 필드만 현재 설정 위에
/// 덮어쓴 뒤 `UpdateSettings` intent 로 dispatch 한다(라이브 `engine.settings` 직접
/// mutate 금지 — collapse 가 prev/new 를 비교하므로 pre-mutate 시 분기가 죽는다.
/// clone 위에서만 수정). 이후 기존 파이프라인이 config.toml save 까지 처리한다.
pub fn handle_set_remote_transfer(
    state: &mut crate::state::AppState,
    engine: &CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    // 라이브 설정의 clone 위에서만 수정(직접 mutate 금지).
    let mut new_settings = engine.settings.clone();
    let mut changed = false;

    if let Some(dir) = params.get("dir") {
        let Some(dir) = dir.as_str() else {
            return JsonRpcResponse::invalid_params(id, "'dir' must be a string");
        };
        new_settings.remote_transfer.dir = dir.to_string();
        changed = true;
    }
    if let Some(max_mb) = params.get("max_mb") {
        let Some(max_mb) = max_mb.as_u64() else {
            return JsonRpcResponse::invalid_params(id, "'max_mb' must be a non-negative integer");
        };
        new_settings.remote_transfer.max_mb = max_mb;
        changed = true;
    }
    if !changed {
        return JsonRpcResponse::invalid_params(id, "expected at least one of 'dir', 'max_mb'");
    }

    let applied = serde_json::to_value(&new_settings.remote_transfer).unwrap_or(Value::Null);
    state.dispatch_intent(
        crate::core::intent::DomainIntent::UpdateSettings(new_settings).from_agent_ipc(),
    );
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
        // 라이브 설정을 직접 바꿔도(테스트 편의) get 이 그 값을 반영하는지 확인.
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
