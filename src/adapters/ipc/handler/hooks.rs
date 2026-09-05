use super::params::require_u32;
use super::params::{self, p_try};
use serde_json::json;
use tasty_hooks::HookEvent;

use crate::global_hooks::HookCondition;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// 내장 surface hook 이벤트 안내 문자열 (검증 실패 메시지용).
const BUILTIN_HOOK_EVENTS: &str = "process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS, command-completed, command-completed:EXIT_CODE";

/// `HookEvent::parse` 는 미인식 문자열을 `Custom(_)` 으로 무조건 수용하므로,
/// 여기서 (내장 ∪ 활성 plugin 선언) 집합으로 검증한다. 내장 이벤트는 parse 단계에서
/// 이미 비-Custom 변형으로 해석되므로 항상 허용된다. `Custom(key)` 만 plugin 선언
/// 카탈로그 멤버십을 확인하고, 미선언이면 동적 안내 메시지와 함께 거부한다.
fn validate_hook_event(
    engine: &crate::core::CoreState,
    event: &HookEvent,
    id: &serde_json::Value,
) -> Result<(), JsonRpcResponse> {
    let HookEvent::Custom(key) = event else {
        return Ok(());
    };
    if engine.plugin_hook_events.contains(key) {
        return Ok(());
    }
    let declared = engine.plugin_hook_events.all_keys();
    let declared_str = if declared.is_empty() {
        "(none — no active plugin declares hook events)".to_string()
    } else {
        declared.join(", ")
    };
    Err(JsonRpcResponse::invalid_params(
        id.clone(),
        format!(
            "Unknown hook event '{key}': not a built-in event and not declared by any active plugin. \
             Built-in events: {BUILTIN_HOOK_EVENTS}. Active plugin-declared events: {declared_str}"
        ),
    ))
}

pub(crate) fn handle_hook_set(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Unknown event type: '{event_str}'. Use: {BUILTIN_HOOK_EVENTS}, or a plugin-defined event name"
                ),
            );
        }
    };

    if let Err(resp) = validate_hook_event(engine, &event, &id) {
        return resp;
    }

    // `command-completed` 는 OSC 133 셸 통합이 로드된 셸에서만
    // 발화한다(전제). 그 surface 가 boundary 를 한 번도 못 받았다면 이 훅은
    // 영원히 발사되지 않을 수 있다 — 거부는 아니다(시간 기반 추정이라 이제 막
    // 뜬 surface 를 오탐할 수 있음, `shell_integration_hint.rs` 참고), 경고만
    // 남겨 가시화한다. 등록 주체(사용자 `hook.set` 든 push 완료 전략의 내부
    // dispatch 든) 무관하게 이 지점 하나에서 검사한다.
    if matches!(event, HookEvent::CommandCompleted(_))
        && !engine.shell_integration_boundary_seen.contains(&surface_id)
    {
        tracing::warn!(
            surface_id,
            "hook.set: 'command-completed' hook registered on a surface that has never shown \
             an OSC 133 prompt boundary — this hook may never fire if shell integration is not \
             loaded"
        );
    }

    // S9: hook 은 공유 훅 핸들러 레지스트리를 참조한다. `handler` 파라미터가 있으면
    // 핸들러 id 참조(레지스트리 조회 + hook 트리거 source 게이트 검증), 없으면 옛
    // `command` 를 인라인 셸(익명 hook 핸들러)로 감싼다(하위호환 어댑터).
    let binding = if let Some(handler_id) = params.get("handler").and_then(|v| v.as_str()) {
        let hid = crate::hook_handler::HookHandlerId::new(handler_id);
        match crate::hook_handler::registry::global().get(&hid) {
            Some(h) => {
                if let Err(e) = crate::hook_handler::validate_binding(
                    &h,
                    crate::hook_handler::TriggerSource::Hook,
                ) {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("handler '{handler_id}' cannot bind to a hook trigger: {e}"),
                    );
                }
            }
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("Unknown hook handler '{handler_id}'"),
                );
            }
        }
        tasty_hooks::HookBinding::Handler(handler_id.to_string())
    } else if let Some(command) = params.get("command").and_then(|v| v.as_str()) {
        tasty_hooks::HookBinding::InlineShell(command.to_string())
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing 'command' or 'handler' parameter");
    };

    let once = params
        .get("once")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let hook_id = core.register_surface_hook(engine, surface_id, event, binding, once);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

pub(crate) fn handle_hook_list(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match super::params::optional_u32(params, "surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let hooks: Vec<_> = engine
        .hook_manager
        .list_hooks(surface_id)
        .iter()
        .map(|h| {
            // `binding` = 구조적 표현(`handler:<id>` 또는 인라인 셸 명령).
            // `command` 는 하위호환 별칭 — 인라인 셸이면 옛 응답과 동일한 명령 문자열.
            json!({
                "id": h.id,
                "surface_id": h.surface_id,
                "event": h.event.to_display_string(),
                "binding": h.binding.to_display_string(),
                "command": h.binding.to_display_string(),
                "once": h.once,
            })
        })
        .collect();

    JsonRpcResponse::success(id, json!(hooks))
}

pub(crate) fn handle_hook_unset(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match p_try!(params::opt_int::<u64>(params, "hook_id", &id)) {
        Some(h) => h,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
    };

    let removed = core.unregister_surface_hook(engine, hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

pub(crate) fn handle_global_hook_set(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let condition_str = match params.get("condition").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'condition' parameter"),
    };

    let condition = match HookCondition::parse(condition_str) {
        Some(c) => c,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Invalid condition '{}'. Use: interval:SECS, once:SECS, file:/path",
                    condition_str
                ),
            );
        }
    };

    let command = match params.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'command' parameter"),
    };

    let label = params
        .get("label")
        .and_then(|v| v.as_str())
        .map(String::from);

    let hook_id = core.register_global_hook(engine, condition, command, label);
    JsonRpcResponse::success(id, json!({ "hook_id": hook_id }))
}

pub(crate) fn handle_global_hook_list(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let hooks: Vec<_> = engine
        .global_hook_manager
        .list()
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "condition": h.condition.to_display_string(),
                "command": h.command,
                "label": h.label,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(hooks))
}

pub(crate) fn handle_global_hook_unset(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let hook_id = match require_u32(params, "hook_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let removed = core.unregister_global_hook(engine, hook_id);
    JsonRpcResponse::success(id, json!({ "removed": removed }))
}

pub(crate) fn handle_surface_fire_hook(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match super::require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let event_str = match params.get("event").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'event' parameter"),
    };

    let event = match HookEvent::parse(event_str) {
        Some(e) => e,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Unknown event type: '{event_str}'"),
            );
        }
    };

    if let Err(resp) = validate_hook_event(engine, &event, &id) {
        return resp;
    }

    let fired = core.fire_surface_hooks(engine, surface_id, std::slice::from_ref(&event));
    let event_kind = event.to_display_string();
    // 수동 발화도 CommandCompleted 라면 실제 exit code 를 실어
    // 보낸다 — `tasty surface fire-hook --event command-completed:1` 로 push
    // 전략 대기 task 를 테스트/시뮬레이션할 수 있어야 한다.
    let exit_code = match &event {
        HookEvent::CommandCompleted(code) => *code,
        _ => None,
    };
    for hook_id in &fired {
        state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
            hook_id: *hook_id,
            event_kind: event_kind.clone(),
            surface_id,
            exit_code,
        });
    }
    JsonRpcResponse::success(id, json!({ "fired": fired.len() }))
}
