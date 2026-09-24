use super::params::require_u32;
use super::params::{self, p_try};
use serde_json::json;
use tasty_hooks::HookEvent;

use crate::global_hooks::HookCondition;
use tasty_ipc::protocol::JsonRpcResponse;

/// 내장 surface hook 이벤트 안내 문자열 (검증 실패 메시지용).
const BUILTIN_HOOK_EVENTS: &str = "process-exit, bell, notification, output-match:PATTERN, idle-timeout:SECS, command-completed, command-completed:EXIT_CODE";

/// parse가 Custom으로 받은 이름은 활성 플러그인이 선언한 이벤트인지 추가 확인한다.
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

    // command-completed는 OSC 133 셸 통합이 필요하다. 아직 prompt boundary가 없으면 경고한다.
    // 막 시작한 셸일 수도 있으므로 훅 등록 자체는 거절하지 않는다.
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

    // handler는 등록된 정의를 참조하고, 기존 command 입력은 인라인 셸로 감싼다.
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
            // command는 binding의 호환 별칭이다.
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
    window: &mut dyn crate::ipc::window_port::IpcWindow,
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
    // 수동 command-completed도 종료 코드를 전달해 push 전략의 성공·실패를 시험할 수 있게 한다.
    let exit_code = match &event {
        HookEvent::CommandCompleted(code) => *code,
        _ => None,
    };
    for hook_id in &fired {
        window.push_host_event(crate::state::PendingHostEvent::HookFired {
            hook_id: *hook_id,
            event_kind: event_kind.clone(),
            surface_id,
            exit_code,
        });
    }
    JsonRpcResponse::success(id, json!({ "fired": fired.len() }))
}
