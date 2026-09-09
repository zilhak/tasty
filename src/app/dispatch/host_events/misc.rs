//! 도메인별로 묶기 애매한 잡종 — process / notification / hook / raw + plugin lifecycle.

use serde_json::json;
use tasty_plugin_protocol::EventScope;
use tasty_plugin_protocol::events::LifecycleReason;
use tasty_plugin_protocol::events::payloads::{
    HookFired, NotificationCreated, PluginError, PluginLoaded, ProcessExited,
};

use crate::plugin::PluginManager;

pub(super) fn emit_process_exited(mgr: &mut PluginManager, surface_id: u32) {
    let payload = ProcessExited {
        surface_id,
        exit_code: None,
    };
    mgr.emit_host_event("process.exited", &payload, EventScope::Surface);
}

pub(super) fn emit_notification_created(
    mgr: &mut PluginManager,
    id: u64,
    title: String,
    body: String,
    source: String,
) {
    let payload = NotificationCreated {
        id: id.to_string(),
        title,
        body,
        source,
    };
    mgr.emit_host_event("notification.created", &payload, EventScope::System);
}

pub(super) fn emit_hook_fired(
    mgr: &mut PluginManager,
    hook_id: u64,
    event_kind: String,
    surface_id: u32,
) {
    let scope = if surface_id != 0 {
        EventScope::Surface
    } else {
        EventScope::System
    };
    let payload = HookFired {
        hook_id: hook_id.to_string(),
        event_kind,
        surface_id: if surface_id != 0 {
            Some(surface_id)
        } else {
            None
        },
        payload: serde_json::Value::Null,
    };
    mgr.emit_host_event("hook.fired", &payload, scope);
}

// ─── Plugin lifecycle (D.3.C.G.2.b) ───

pub(super) fn emit_plugin_loaded(mgr: &mut PluginManager, plugin_id: String, version: String) {
    let payload = PluginLoaded { plugin_id, version };
    mgr.emit_host_event("plugin.loaded", &payload, EventScope::System);
}

/// 발화 자체는 `crate::ipc::handler::plugin` 에 있다 — 헤드리스가 창 큐를 안 거치고
/// 같은 이벤트를 내야 해서, 키와 payload 는 그쪽 한 벌만 존재한다. 여기 남는 것은
/// 큐가 실어 온 값을 그 함수의 인자 모양으로 맞추는 일뿐이다.
pub(super) fn emit_plugin_enable_toggled(
    mgr: &mut PluginManager,
    plugin_id: String,
    enabled: bool,
) {
    crate::ipc::handler::plugin::emit_enable_toggled(mgr, plugin_id, enabled);
}

/// 위와 같다. `reason` 은 큐에 문자열로 실려 오므로(`PendingHostEvent` 가 protocol
/// 타입을 안 들고 다닌다) 되읽는 것이 이 함수의 일이다.
pub(super) fn emit_plugin_unloaded(mgr: &mut PluginManager, plugin_id: String, reason: String) {
    let lr = match reason.as_str() {
        "ipc" => LifecycleReason::Ipc,
        "crash" => LifecycleReason::Crash,
        _ => LifecycleReason::User,
    };
    crate::ipc::handler::plugin::emit_unloaded(mgr, plugin_id, lr);
}

pub(super) fn emit_plugin_error(
    mgr: &mut PluginManager,
    plugin_id: String,
    error_kind: String,
    message: String,
) {
    let payload = PluginError {
        plugin_id,
        error_kind,
        message,
    };
    mgr.emit_host_event("plugin.error", &payload, EventScope::System);
}

/// install / remove / grant / revoke 4종을 단일 helper 로. `change_kind` 가
/// raw event key 결정. 옛 lifecycle.rs 에는 대응 호출 없었음 — 본 substep 의
/// 신규 가시성.
pub(super) fn emit_plugin_registry_changed(
    mgr: &mut PluginManager,
    plugin_id: String,
    change_kind: String,
    detail: serde_json::Value,
) {
    let key = match change_kind.as_str() {
        "installed" => "plugin.installed",
        "removed" => "plugin.removed",
        "permission_granted" => "plugin.permission_granted",
        "permission_revoked" => "plugin.permission_revoked",
        other => {
            tracing::warn!("emit_plugin_registry_changed: unknown change_kind '{other}'");
            return;
        }
    };
    let payload = json!({
        "plugin_id": plugin_id,
        "detail": detail,
    });
    mgr.emit_host_event(key, &payload, EventScope::System);
}

pub(super) fn emit_plugin_surface_kind_registered(
    mgr: &mut PluginManager,
    plugin_id: String,
    kind: String,
    rendering: String,
) {
    let payload = json!({
        "plugin_id": plugin_id,
        "kind": kind,
        "rendering": rendering,
    });
    mgr.emit_host_event(
        "plugin.surface_kind_registered",
        &payload,
        EventScope::System,
    );
}

/// `[[contributes.window]]` 항목이 hello 시점에 등록되었음을 알리는 stub
/// 이벤트. 1.0 에서는 실제 spawn 동작이 없고 가시성만 제공한다.
pub(super) fn emit_plugin_window_declared(
    mgr: &mut PluginManager,
    plugin_id: String,
    window_id: String,
) {
    let payload = json!({
        "plugin_id": plugin_id,
        "window_id": window_id,
    });
    mgr.emit_host_event("plugin.window_declared", &payload, EventScope::System);
}
