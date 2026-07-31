use serde_json::json;
use tasty_hooks::HookEvent;

use crate::global_hooks::HookCondition;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// 내장 surface hook 이벤트 안내 문자열 (검증 실패 메시지용).
const BUILTIN_HOOK_EVENTS: &str =
    "process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS";

/// `HookEvent::parse` 는 미인식 문자열을 `Custom(_)` 으로 무조건 수용하므로(TODO 15),
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
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

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
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
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
    let hook_id = match params.get("hook_id").and_then(|v| v.as_u64()) {
        Some(h) => h as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'hook_id' parameter"),
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
    for hook_id in &fired {
        state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
            hook_id: *hook_id,
            event_kind: event_kind.clone(),
            surface_id,
        });
    }
    JsonRpcResponse::success(id, json!({ "fired": fired.len() }))
}
