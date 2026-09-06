//! Debug 빌드 전용 IPC 핸들러 중 **gui 심볼을 하나도 안 쓰는 것** — `ui.state` 조회와
//! `debug.settings.apply` 패치 적용.
//!
//! `debug.rs` 와 갈라져 있는 이유는 `debug_terminal.rs` 와 같다 — 그쪽 모듈이
//! `#[cfg(all(debug_assertions, feature = "gui"))]` 라 헤드리스에서 통째로 사라지는데,
//! 여기 둘은 헤드리스에서도 유효해야 한다. `ui.state` 는 워크스페이스/pane/tab 수를 세고
//! (popup 상태만 gui 게이트로 갈라 읽는다), settings cascade 는 gui 없이도 도는
//! 파이프라인이다.
//!
//! 이 파일이 생기기 전에는 그 둘이 부모 `handler.rs` 에 직접 있었다 — 옮길 곳이 없어서였다.
//! 유일한 debug 모듈이 gui 게이트였기 때문이다. 원칙 1 의 집행 형태는 debug 전용 코드를
//! **모듈 선언에 cfg 가 붙은 별도 파일**로 모으라고 요구하므로, 그 자리를 만들어 옮겼다.
//! 판정은 `crates/tasty-doc-guards/tests/debug_handlers_live_in_cfg_declared_modules.rs`.

use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

pub(super) fn handle_ui_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let ws = state.active_workspace(engine);
    let pane_count = ws.pane_layout().all_pane_ids().len();
    let focused_pane_id = ws.focused_pane;
    let tab_count = ws
        .pane_layout()
        .find_pane(focused_pane_id)
        .map(|p| p.tabs.len())
        .unwrap_or(0);
    #[cfg(feature = "gui")]
    let notification_panel_open = state.popups.is_open("notifications");
    #[cfg(not(feature = "gui"))]
    let notification_panel_open = false;
    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "notification_panel_open": notification_panel_open,
            "active_workspace": state.active_workspace,
            "workspace_count": engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
        }),
    )
}

/// `debug.settings.apply` — `{ settings }` 의 부분 JSON patch 를 라이브 settings
/// 직렬화 **복사본** 위에 재귀 deep-merge 한 뒤, 완성된 전체 `Settings` 로
/// `UpdateSettings` intent 를 dispatch 한다. 이후는 기존 파이프라인
/// (dispatch_pending_intents → Core::apply → SettingsUpdated → cascade)이
/// collapse / theme / config.toml save 까지 처리한다 — 모달 / proxy 불요.
///
/// 사용자의 "설정 모달에서 값 변경 후 저장" 을 재현하는 디버그 동작이므로 release 에
/// 노출되지 않는다. gui feature 와 무관하게 동작하므로(settings cascade 는 headless 에서도
/// 유효) 이 모듈에 산다 — 모듈 doc 참조.
///
/// 주의:
/// - 라이브 `engine.settings` 를 dispatch 전에 직접 mutate 하지 않는다. cascade 가
///   prev(라이브)와 new 를 비교해 collapse 분기를 결정하므로, pre-mutate 시
///   prev==new 가 되어 collapse 가 죽는다. merge 는 직렬화 복사본 위에서만 한다.
/// - `Settings` 는 `deny_unknown_fields` 가 아니므로(`#[serde(default)]`) patch 의
///   오타/미지정 키는 조용히 무시된다(no-op). 타입 불일치는 `from_value` Err →
///   `invalid_params` 로 거부되고 라이브는 불변.
pub(super) fn handle_debug_settings_apply(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(patch) = params.get("settings") else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'settings' parameter");
    };
    if !patch.is_object() {
        return JsonRpcResponse::invalid_params(id, "'settings' must be a JSON object");
    }

    // base = 라이브 settings 직렬화 복사본 (라이브 자체는 건드리지 않는다).
    let mut base = match serde_json::to_value(&engine.settings) {
        Ok(v) => v,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                -32603,
                format!("failed to serialize live settings: {e}"),
            );
        }
    };
    json_deep_merge(&mut base, patch);

    // base 가 이미 완전한 Settings 직렬화이므로 serde(default) 함정을 피한다.
    let new_settings: tasty_settings::Settings = match serde_json::from_value(base) {
        Ok(s) => s,
        Err(e) => {
            return JsonRpcResponse::invalid_params(id, format!("invalid settings patch: {e}"));
        }
    };

    state.dispatch_intent(
        crate::core::intent::DomainIntent::UpdateSettings(new_settings).from_agent_ipc(),
    );
    JsonRpcResponse::success(id, json!({ "applied": true }))
}

/// 표준 재귀 deep-merge. 양쪽이 object 면 키별로 재귀 병합하고, 그 외에는
/// `patch` 값으로 `target` 을 치환한다. 얕은 치환이 아니므로 nested 필드가
/// 유실되지 않는다 (예: `appearance` 의 일부 키만 patch 해도 나머지 보존).
fn json_deep_merge(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                json_deep_merge(
                    target_map
                        .entry(k.clone())
                        .or_insert(serde_json::Value::Null),
                    v,
                );
            }
        }
        (target_slot, patch_val) => {
            *target_slot = patch_val.clone();
        }
    }
}
