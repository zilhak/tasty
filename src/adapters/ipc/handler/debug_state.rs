//! GUI feature 없이도 쓰는 디버그 UI 상태 조회와 설정 변경.
//! 팝업 상태만 GUI에서 읽고 나머지는 헤드리스에서도 처리한다.

use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

pub(super) fn handle_ui_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    // 창이 없는 parked engine도 조회된다. workspace가 없으면 관련 값은 0 대신 null이다.
    // active_workspace에는 system.info와 마찬가지로 저장된 인덱스를 반환한다.
    let ws = (!engine.workspaces.is_empty()).then(|| state.active_workspace(engine));
    let pane_count = ws.map(|ws| ws.pane_layout().all_pane_ids().len());
    let focused_pane = ws.and_then(|ws| ws.pane_layout().find_pane(ws.focused_pane));
    let tab_count = ws.map(|_| focused_pane.map(|p| p.tabs.len()).unwrap_or(0));
    let active_tab = ws.map(|_| focused_pane.map(|p| p.active_tab).unwrap_or(0));
    #[cfg(feature = "gui")]
    let notification_panel_open = state.popups.is_open("notifications");
    #[cfg(not(feature = "gui"))]
    let notification_panel_open = false;
    // 실제 단축키 처리와 같은 판정을 사용한다. 전체화면 무대는 오버레이보다 먼저 입력을 소비한다.
    let keyboard_shortcuts_gated = state.fullscreen_stage_active() || state.keyboard_overlay_open();

    // 차단 사유를 구별하도록 각 조건의 true/false도 반환한다.
    // 실제 입력 판정이 읽은 값을 그대로 보고해야 잘못된 상태도 진단할 수 있다.
    // keyboard_overlay_open의 인자와 일치하는지는 fullscreen_stage_input_gate가 검사한다.
    // 전체화면 무대는 해당 함수 밖의 조건이므로 따로 보고한다.
    #[cfg(feature = "gui")]
    let host_popup_focused = state.popups.has_focused();
    #[cfg(not(feature = "gui"))]
    let host_popup_focused = false;

    #[cfg(feature = "gui")]
    let tutorial = json!({
        "active": state.tutorial.active.map(|a| json!({"topic": a.topic, "step": a.step})),
        "ready": state.tutorial.ready(),
        "keyboard_focus": state.tutorial.keyboard_focus,
        "catalog_open": state.popups.is_open("tutorial_topics"),
        "rect": state.tutorial.callout_rect.map(|r| [r.min.x, r.min.y, r.max.x, r.max.y]),
        "save_error": state.tutorial.save_error,
    });
    #[cfg(not(feature = "gui"))]
    let tutorial = serde_json::Value::Null;
    JsonRpcResponse::success(
        id,
        json!({
            "tutorial": tutorial,
            // 열기 요청과 실제 모달 표시는 다르다. 표시 여부는 modal_open으로 확인한다.
            "settings_open_requested": state.settings_open_requested,
            "keyboard_shortcuts_gated": keyboard_shortcuts_gated,
            "gate_fullscreen_stage_active": state.fullscreen_stage_active(),
            "gate_settings_open_requested": state.settings_open_requested,
            "gate_input_dialog_open": state.has_input_dialog_open(),
            "gate_host_popup_focused": host_popup_focused,
            "gate_plugin_popup_open": state.plugin_popup_open,
            "modal_open": state.active_modal_id.is_some(),
            "active_modal_id": state.active_modal_id,
            // 창 ID만으로는 설정 모달과 다른 모달을 구분할 수 없다.
            "active_modal_kind": state.active_modal_kind.map(|k| k.as_str()),
            "notification_panel_open": notification_panel_open,
            "active_workspace": state.active_workspace,
            "workspace_count": engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
            "active_tab": active_tab,
        }),
    )
}

/// 현재 설정의 직렬화 사본에 부분 patch를 합친 뒤 UpdateSettings로 적용한다.
/// 실제 설정을 미리 바꾸면 후속 처리에서 이전 값과의 차이를 알 수 없으므로 사본만 수정한다.
/// 알 수 없는 키는 무시하고 잘못된 타입은 거절한다. GUI 없이도 같은 설정 변경 경로를 쓴다.
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

    // 전체 설정에 합쳤으므로 patch에서 빠진 필드가 기본값으로 바뀌지 않는다.
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

/// object는 키별로 재귀 병합하고 그 외 값은 대체한다. 지정하지 않은 중첩 필드는 유지한다.
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

#[cfg(test)]
mod tests {
    use super::handle_ui_state;

    #[test]
    fn ui_state_answers_for_a_parked_engine_without_workspaces() {
        let (state, mut engine) = crate::state::tests::test_state();
        engine.workspaces.clear();
        let resp = handle_ui_state(&state, &engine, serde_json::json!(1));
        let result = resp.result.expect("성공 응답이어야 한다");
        assert_eq!(result["workspace_count"], 0);
        assert!(result["pane_count"].is_null(), "{result}");
        assert!(result["tab_count"].is_null(), "{result}");
        assert!(result["active_tab"].is_null(), "{result}");
        assert_eq!(result["active_workspace"], state.active_workspace);
        assert!(result["keyboard_shortcuts_gated"].is_boolean(), "{result}");
    }

    #[test]
    fn ui_state_keeps_the_counts_when_a_workspace_exists() {
        let (state, engine) = crate::state::tests::test_state();
        let resp = handle_ui_state(&state, &engine, serde_json::json!(1));
        let result = resp.result.expect("성공 응답이어야 한다");
        assert_eq!(result["workspace_count"], 1);
        assert!(
            result["pane_count"].as_u64().is_some_and(|n| n >= 1),
            "{result}"
        );
        assert!(
            result["tab_count"].as_u64().is_some_and(|n| n >= 1),
            "{result}"
        );
        assert_eq!(result["active_tab"], 0);
    }
}
