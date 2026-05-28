//! Plugin 단축키 매칭 + dispatch 헬퍼 (단계 F).
//!
//! event_handler.rs가 winit 키 이벤트를 normal window dispatch로 보내기 전에,
//! focused surface가 plugin RemoteSurface일 경우 이 모듈을 통해 plugin command
//! 매칭을 시도한다. 매칭 시 호스트 단축키 dispatch는 trigger되지 않고 이벤트가
//! 소모된다.
//!
//! 실제 IPC 송신(plugin 프로세스로 command_id 전달)은 단계 G에서 protocol/SDK
//! 변경과 함께 추가된다. 현재는 dispatch가 호출됐다는 것만 trace 로그로 남긴다.

use winit::keyboard::{Key, ModifiersState};

use tasty_settings::KeybindingSettings;

use crate::plugin::PluginManager;
use crate::plugin::command_registry::{EffectiveBinding, effective_binding};
use crate::shortcuts::matches_any_binding;

/// Focused surface가 RemoteSurface인 경우 (plugin_id, surface_id) 튜플 반환.
pub fn focused_plugin_surface(
    state: &crate::state::AppState,
    engine: &crate::engine_state::CoreState,
) -> Option<(String, u32)> {
    let pane = state.focused_pane(engine)?;
    let tab = pane.tabs.get(pane.active_tab)?;
    let focused = tab.focused_surface;
    let surface = tab.layout().find_surface(focused)?;
    let remote = surface
        .as_any()
        .downcast_ref::<crate::plugin::remote_surface::RemoteSurface>()?;
    Some((remote.plugin_id.clone(), remote.id))
}

/// 주어진 plugin이 contribute한 command 중 현재 키 + modifiers에 매칭되는 것이
/// 있으면 command_id를 반환. 사용자 override + 매니페스트 default + 호스트
/// keybindings를 모두 합성한 effective binding을 사용.
pub fn match_plugin_shortcut(
    mgr: &PluginManager,
    plugin_id: &str,
    key: &Key,
    mods: ModifiersState,
    host_kb: &KeybindingSettings,
) -> Option<String> {
    for entry in mgr.command_registry.commands_for(plugin_id) {
        let ov = mgr.config.shortcut_override(plugin_id, &entry.command_id);
        let bindings: Vec<String> = match effective_binding(entry, ov, host_kb) {
            EffectiveBinding::Keys(k) => k,
            EffectiveBinding::Inherit { keys, .. } => keys,
            EffectiveBinding::None => continue,
        };
        if bindings.is_empty() {
            continue;
        }
        if matches_any_binding(&bindings, key, mods) {
            return Some(entry.command_id.clone());
        }
    }
    None
}

/// Plugin command를 plugin 프로세스에 전달.
///
/// 두 경로로 발화한다:
/// - Event Bus 1.0 `command.invoked` owner-unicast (PR 5 — Option D 기본 경로).
///   sub_id=0 sentinel. 다른 plugin이 `command.invoked` 구독해도 보이지 않는다.
/// - 옛 `command.invoke` IPC. plugin이 `SurfaceResult`로 tree/display_name을 갱신할 수
///   있게 응답 기반 형태. 두 경로는 당분간 병행 — plugin이 새 경로로 자연 마이그레이션
///   하면 옛 IPC는 후속 PR에서 정리된다.
pub fn dispatch_plugin_command(
    mgr: &mut PluginManager,
    plugin_id: &str,
    command_id: &str,
    surface_id: u32,
) {
    tracing::debug!(
        "plugin shortcut matched: plugin='{}' command='{}' surface={}",
        plugin_id,
        command_id,
        surface_id
    );
    let manifest_scope = mgr
        .command_registry
        .commands_for(plugin_id)
        .iter()
        .find(|e| e.command_id == command_id)
        .map(|e| e.scope)
        .unwrap_or_default();
    {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::payloads::{
            CommandInvoked, CommandScope, CommandTrigger,
        };
        let scope = match manifest_scope {
            crate::plugin::manifest::CommandScope::Global => CommandScope::Global,
            crate::plugin::manifest::CommandScope::Surface => CommandScope::Surface,
        };
        let payload = CommandInvoked {
            plugin_id: plugin_id.to_string(),
            command_id: command_id.to_string(),
            scope,
            source_surface_id: Some(surface_id),
            trigger: CommandTrigger::Shortcut,
        };
        mgr.emit_host_event_to_plugin(plugin_id, "command.invoked", &payload, EventScope::System);
    }
    mgr.send_command_invoke(plugin_id, surface_id, command_id);
}
