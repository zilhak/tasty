//! 플러그인 enable/disable/unload 호스트 이벤트 발행. GUI와 헤드리스가 같은 키와 payload를 쓴다.
//!
//! GUI는 창 큐(`PendingHostEvent`)를 거쳐 발행 시점을 정하고, 헤드리스는
//! `cascade_toggle_events_headless`에서 즉시 발행한다.

use tasty_plugin_protocol::EventScope;
use tasty_plugin_protocol::events::LifecycleReason;
use tasty_plugin_protocol::events::payloads::{PluginEnableToggled, PluginUnloaded};

use crate::plugin::PluginManager;

pub(crate) fn emit_enable_toggled(mgr: &mut PluginManager, plugin_id: String, enabled: bool) {
    let payload = PluginEnableToggled { plugin_id };
    let key = if enabled {
        "plugin.enabled"
    } else {
        "plugin.disabled"
    };
    mgr.emit_host_event(key, &payload, EventScope::System);
}

pub(crate) fn emit_unloaded(mgr: &mut PluginManager, plugin_id: String, reason: LifecycleReason) {
    let payload = PluginUnloaded { plugin_id, reason };
    mgr.emit_host_event("plugin.unloaded", &payload, EventScope::System);
}

/// 창 큐가 없는 헤드리스에서는 이벤트를 즉시 발행한다.
/// 종료된 플러그인의 훅 선언도 해제해 더 이상 등록할 수 없게 한다.
#[cfg(not(feature = "gui"))]
pub(crate) fn cascade_toggle_events_headless(
    mgr: &mut PluginManager,
    hook_events: &crate::core::hook_event_registry::PluginHookEventRegistry,
    events: Vec<crate::app::command::CoreEvent>,
) {
    use crate::app::command::CoreEvent;
    for ev in events {
        match ev {
            CoreEvent::PluginEnableToggled { plugin_id, enabled } => {
                emit_enable_toggled(mgr, plugin_id, enabled);
            }
            CoreEvent::PluginUnloaded { plugin_id, reason } => {
                hook_events.unregister(&plugin_id);
                emit_unloaded(mgr, plugin_id, reason);
            }
            other => tracing::warn!(
                "cascade_toggle_events_headless: 토글이 낼 수 없는 CoreEvent: {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }
}
