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
    let focused_pane = ws.pane_layout().find_pane(focused_pane_id);
    let tab_count = focused_pane.map(|p| p.tabs.len()).unwrap_or(0);
    // 탭 **전환**은 수로 안 보인다 — `tab_count` 는 전환해도 그대로다. 그래서 전환을
    // 재는 시험은 관측할 것이 없어 고정 sleep 으로 대신하게 되고, 그러면 전환이 아예
    // 안 일어나도 통과한다. 활성 탭 인덱스가 그 관측 축이다.
    let active_tab = focused_pane.map(|p| p.active_tab).unwrap_or(0);
    #[cfg(feature = "gui")]
    let notification_panel_open = state.popups.is_open("notifications");
    #[cfg(not(feature = "gui"))]
    let notification_panel_open = false;
    // 단축키가 **소비되는지 자체**를 노출한다. `handle_keyboard_input` 은 오버레이가 열려
    // 있으면 단축키 경로에 아예 안 들어가는데(`view/main/keyboard.rs`), 그 게이트를 여는
    // 네 조건 중 `settings_open` 하나만 여기 보였다. 나머지 셋(입력 dialog · 포커스된 host
    // popup · plugin popup)이 걸려 있으면 시험은 "단축키가 안 먹는다" 만 보고 **왜인지는
    // 못 본다** — 실측으로 그 자리에 걸렸다: 공유 인스턴스가 오염된 회차에서 도착 카나리아가
    // 죽었는데 실패 메시지가 원인을 못 담았다.
    //
    // 판정을 여기서 다시 쓰지 않고 소비처와 **같은 함수**를 부른다. 사본을 두면 게이트가
    // 바뀔 때 둘이 어긋나고, 어긋난 쪽은 조용하다.
    //
    // 무대 항이 함께 들어가는 이유: 단축키가 매처에 닿으려면 **둘 다** 열려 있어야 한다.
    // `handle_keyboard_input` 은 0 단계에서 전체화면 무대를 먼저 소비하고
    // (`try_consume_fullscreen_stage_key`) 4 단계에서 오버레이를 본다. 오버레이만 보면
    // 이 필드가 이름이 약속한 것의 절반만 답하고, **무대 중에는 거짓으로 "안 막혔다"** 를
    // 말한다 — 그러면 이 필드를 넣은 이유(왜 단축키가 안 먹었는지)가 그 경우에 사라진다.
    let keyboard_shortcuts_gated = state.fullscreen_stage_active() || state.keyboard_overlay_open();

    // ★ 그리고 다섯 항을 **각각** 찍는다. 위 합성값은 `||` 로 뭉치므로 "막혔다" 까지만
    // 말하고 **무엇이 막았는지는 말하지 않는다.** 실측으로 그 자리에 걸렸다 — GUI 스위트
    // 한 회차가 어느 지점부터 21 건 연속 `true` 였는데, 그 값만으로는 다섯 중 무엇이 열린
    // 채 남았는지 고를 수 없었다. 합성값을 넣은 이유가 "왜 안 먹었는가" 였는데, 정작 원인이
    // 하나로 안 좁혀지는 회차에서 침묵한 것이다.
    //
    // **참인 것만 나열하지 않고 거짓도 값으로 낸다.** 이름만 나열하면 "그 항이 거짓이라
    // 빠졌다" 와 "보고가 그 항을 아예 모른다" 가 같은 모양이 된다 — 안 돈 것과 통과한 것이
    // 같은 줄을 만드는 그 형태다. 다섯 칸이 항상 차 있으면 그 둘이 갈린다.
    //
    // 합성값은 그대로 둔다. 더하는 것이지 바꾸는 것이 아니다 — 기존 소비자가 있다.
    //
    // 이름은 술어 `state::keyboard_overlay_open` 의 **매개변수 이름 그대로**다. 이 다섯은
    // 판정의 사본이라 술어에 항이 늘면 조용히 낡는다 — 그러면 새 항이 막은 회차에서
    // 다섯 칸이 전부 `false` 인데 합성값만 `true` 인, 원인 없는 보고가 남는다. 그래서 정합을
    // `tests/fullscreen_stage_input_gate.rs` 가 원문 대조로 강제한다(이름이 일치해야 한다).
    //
    // 무대 항은 술어 밖이다 — `handle_keyboard_input` 이 0 단계에서 따로 소비하므로
    // 매개변수로는 안 잡힌다. 그래서 따로 센다.
    #[cfg(feature = "gui")]
    let host_popup_focused = state.popups.has_focused();
    #[cfg(not(feature = "gui"))]
    let host_popup_focused = false;

    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "keyboard_shortcuts_gated": keyboard_shortcuts_gated,
            "gate_fullscreen_stage_active": state.fullscreen_stage_active(),
            "gate_settings_open": state.settings_open,
            "gate_input_dialog_open": state.has_input_dialog_open(),
            "gate_host_popup_focused": host_popup_focused,
            "gate_plugin_popup_open": state.plugin_popup_open,
            "notification_panel_open": notification_panel_open,
            "active_workspace": state.active_workspace,
            "workspace_count": engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
            "active_tab": active_tab,
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
