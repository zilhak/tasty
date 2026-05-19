//! 도메인별로 묶기 애매한 잡종 — process / notification / hook / raw.

use tasty_plugin_protocol::EventScope;
use tasty_plugin_protocol::events::payloads::{HookFired, NotificationCreated, ProcessExited};

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
        surface_id: if surface_id != 0 { Some(surface_id) } else { None },
        payload: serde_json::Value::Null,
    };
    mgr.emit_host_event("hook.fired", &payload, scope);
}

pub(super) fn emit_raw(mgr: &mut PluginManager, key: String, payload: serde_json::Value) {
    mgr.emit_host_event(&key, &payload, EventScope::System);
}
