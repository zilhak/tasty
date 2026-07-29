//! Plugin 단축키 매칭 + dispatch 헬퍼 (단계 F).
//!
//! event_handler.rs가 winit 키 이벤트를 normal window dispatch로 보내기 전에,
//! focused surface가 plugin RemoteSurface일 경우 이 모듈을 통해 plugin command
//! 매칭을 시도한다. 매칭 시 호스트 단축키 dispatch는 trigger되지 않고 이벤트가
//! 소모된다.
//!
//! 포커스된 plugin surface가 없을 때는 `match_plugin_shortcut`(단일 plugin
//! 대상)이 아니라 `match_global_shortcut`(등록된 모든 plugin의 `CommandScope::Global`
//! command 대상)을 쓴다 — 호출 순서는 `App::try_plugin_shortcut`
//! (`src/app/plugin_glue/shortcut.rs`)이 결정.

use winit::keyboard::{Key, ModifiersState};

use tasty_settings::KeybindingSettings;

use crate::plugin::PluginManager;
use crate::plugin::command_registry::{EffectiveBinding, effective_binding};
use crate::shortcuts::matches_any_binding;

/// Focused surface가 RemoteSurface인 경우 (plugin_id, surface_id) 튜플 반환.
pub fn focused_plugin_surface(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<(String, u32)> {
    let pane = state.focused_pane(engine)?;
    let tab = pane.tabs.get(pane.active_tab)?;
    let focused = tab.focused_surface;
    let surface = tab.layout().find_surface(focused)?;
    let remote = surface
        .as_any()
        .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()?;
    Some((remote.plugin_id.clone(), remote.id))
}

/// 주어진 plugin이 contribute한 command 중 현재 키 + modifiers에 매칭되는 것이
/// 있으면 command_id를 반환. 사용자 override + 매니페스트 default + 호스트
/// keybindings를 모두 합성한 effective binding을 사용.
///
/// 포커스된 plugin surface가 있을 때(그 plugin이 우선권을 갖는 경로)만 호출한다 —
/// scope(`Global`/`Surface`)와 무관하게 그 plugin의 커맨드를 모두 후보로 본다,
/// 왜냐하면 "그 plugin의 surface가 이미 포커스되어 있다"는 조건 자체가 `Surface`
/// scope의 발화 조건을 이미 만족하기 때문이다.
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

/// 포커스된 plugin surface가 없을 때: 등록된 **모든** plugin의
/// `CommandScope::Global` command를 대상으로 키 매칭. 매칭되면
/// `(plugin_id, command_id)`를 반환.
///
/// `Surface` scope command는 여기서 대상이 되지 않는다 — 그 owner plugin의
/// surface가 실제로 포커스되어 있을 때만 `match_plugin_shortcut`으로 매칭된다.
pub fn match_global_shortcut(
    mgr: &PluginManager,
    key: &Key,
    mods: ModifiersState,
    host_kb: &KeybindingSettings,
) -> Option<(String, String)> {
    for entry in mgr.command_registry.iter_global() {
        let ov = mgr
            .config
            .shortcut_override(&entry.plugin_id, &entry.command_id);
        let bindings: Vec<String> = match effective_binding(entry, ov, host_kb) {
            EffectiveBinding::Keys(k) => k,
            EffectiveBinding::Inherit { keys, .. } => keys,
            EffectiveBinding::None => continue,
        };
        if bindings.is_empty() {
            continue;
        }
        if matches_any_binding(&bindings, key, mods) {
            return Some((entry.plugin_id.clone(), entry.command_id.clone()));
        }
    }
    None
}

/// Event Bus 1.0 `command.invoked` owner-unicast 발사 (sub_id=0 sentinel — 다른
/// plugin이 `command.invoked`를 구독해도 보이지 않는다).
///
/// `action`(선언적 처리) 유무·대상 surface 유무와 무관하게 매칭될 때마다 항상
/// 실행되는 informational 통지 — plugin이 관찰 목적으로만 구독해도 안전하다.
pub fn emit_command_invoked(
    mgr: &mut PluginManager,
    plugin_id: &str,
    command_id: &str,
    source_surface_id: Option<u32>,
) {
    let manifest_scope = mgr
        .command_registry
        .find(plugin_id, command_id)
        .map(|e| e.scope)
        .unwrap_or_default();
    use tasty_plugin_protocol::EventScope;
    use tasty_plugin_protocol::events::payloads::{CommandInvoked, CommandScope, CommandTrigger};
    let scope = match manifest_scope {
        crate::plugin::manifest::CommandScope::Global => CommandScope::Global,
        crate::plugin::manifest::CommandScope::Surface => CommandScope::Surface,
    };
    let payload = CommandInvoked {
        plugin_id: plugin_id.to_string(),
        command_id: command_id.to_string(),
        scope,
        source_surface_id,
        trigger: CommandTrigger::Shortcut,
    };
    mgr.emit_host_event_to_plugin(plugin_id, "command.invoked", &payload, EventScope::System);
}

/// `action`이 선언되지 않은 command를 plugin 프로세스에 전달(legacy 경로).
///
/// - Event Bus `command.invoked`는 항상 발사(`emit_command_invoked`).
/// - 옛 `command.invoke` IPC(plugin의 `handle_command`를 트리거하고
///   `SurfaceResult`로 tree/display_name을 갱신할 수 있게 하는 응답 기반 형태)는
///   `surface_id`가 있을 때만 발사한다. `CommandInvokeCtx`/`send_command_invoke`가
///   `surface_id: u32`를 필수로 요구해 "대상 surface 없음"을 표현할 수 없기
///   때문 — 포커스된 plugin surface 없이 매칭된 `Global` command가 이 케이스다.
///   그 경우 plugin은 Event Bus 경로만으로 command 발화를 알 수 있다(옛 IPC
///   round-trip은 애초에 `Surface` scope 전용으로 한정).
///
/// `action`이 선언된 command는 이 함수를 거치지 않는다 — 호출자(`App::try_plugin_shortcut`)가
/// 그 경우 `emit_command_invoked` + 직접 액션 실행으로 분기하고 여기로 오지 않는다.
pub fn dispatch_plugin_command(
    mgr: &mut PluginManager,
    plugin_id: &str,
    command_id: &str,
    surface_id: Option<u32>,
) {
    tracing::debug!(
        "plugin shortcut matched: plugin='{}' command='{}' surface={:?}",
        plugin_id,
        command_id,
        surface_id
    );
    emit_command_invoked(mgr, plugin_id, command_id, surface_id);
    if let Some(surface_id) = surface_id {
        mgr.send_command_invoke(plugin_id, surface_id, command_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_manifest::{CommandDecl, Contributes, Entry};
    use winit::keyboard::{NamedKey, SmolStr};

    // `PluginManager::new`는 `tasty-host-plugin` crate 자체의 `#[cfg(test)]` 전용
    // ctor라 외부 crate(본 바이너리)의 테스트 빌드에서는 보이지 않는다 — production
    // 경로와 동일한 `with_registries` + stub registry 로 대체.
    struct StubFormat;
    impl tasty_plugin_protocol::host_port::FileFormatRegistryPort for StubFormat {
        fn install_plugin_detectors(&self, _: &str, _: &[serde_json::Value]) {}
        fn uninstall_plugin(&self, _: &str) {}
    }
    struct StubHandler;
    impl tasty_plugin_protocol::host_port::FileHandlerRegistryPort for StubHandler {
        fn install_plugin_handlers(&self, _: &str, _: &[serde_json::Value]) {}
        fn uninstall_plugin(&self, _: &str) {}
    }

    fn mgr() -> PluginManager {
        PluginManager::with_registries(
            std::sync::Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
            std::sync::Arc::new(StubFormat),
            std::sync::Arc::new(StubHandler),
        )
    }

    fn k_char(s: &str) -> Key {
        Key::Character(SmolStr::new(s))
    }

    fn manifest_with_commands(id: &str, cmds: Vec<CommandDecl>) -> tasty_plugin_manifest::Manifest {
        tasty_plugin_manifest::Manifest {
            manifest_version: 1,
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1".to_string(),
            authors: vec![],
            description: String::new(),
            homepage: String::new(),
            api_version: "1".to_string(),
            entry: Entry::Process {
                command: "x".to_string(),
                args: vec![],
            },
            surface_kinds: vec![],
            permissions: vec![],
            event_subscribe: vec![],
            event_publish: vec![],
            events_emitted: vec![],
            contributes: Contributes {
                commands: cmds,
                ..Default::default()
            },
            extends: None,
            lang_dir: "lang".to_string(),
            bundle: true,
        }
    }

    fn cmd(
        id: &str,
        default_keybinding: &str,
        scope: tasty_plugin_manifest::CommandScope,
    ) -> CommandDecl {
        CommandDecl {
            id: id.to_string(),
            title_i18n_key: format!("{id}.title"),
            default_keybinding: Some(default_keybinding.to_string()),
            binding_mode: tasty_plugin_manifest::BindingMode::Independent,
            scope,
            action: None,
        }
    }

    fn ctrl_shift_r() -> (Key, ModifiersState) {
        (
            k_char("r"),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        )
    }

    // ── match_global_shortcut ─────────────────────────────────────

    #[test]
    fn match_global_shortcut_finds_across_multiple_plugins() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.noop",
                "ctrl+alt+z",
                tasty_plugin_manifest::CommandScope::Global,
            )],
        ));
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.b",
            vec![cmd(
                "b.open",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Global,
            )],
        ));
        let kb = KeybindingSettings::preset_tasty();
        let (key, mods) = ctrl_shift_r();
        let matched = match_global_shortcut(&m, &key, mods, &kb);
        assert_eq!(
            matched,
            Some(("com.example.b".to_string(), "b.open".to_string()))
        );
    }

    #[test]
    fn match_global_shortcut_ignores_surface_scope_commands() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.surface_only",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Surface,
            )],
        ));
        let kb = KeybindingSettings::preset_tasty();
        let (key, mods) = ctrl_shift_r();
        assert_eq!(match_global_shortcut(&m, &key, mods, &kb), None);
    }

    #[test]
    fn match_global_shortcut_no_match_returns_none() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.other",
                "ctrl+alt+z",
                tasty_plugin_manifest::CommandScope::Global,
            )],
        ));
        let kb = KeybindingSettings::preset_tasty();
        let (key, mods) = ctrl_shift_r();
        assert_eq!(match_global_shortcut(&m, &key, mods, &kb), None);
    }

    #[test]
    fn match_global_shortcut_respects_user_override() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.open",
                "ctrl+alt+z",
                tasty_plugin_manifest::CommandScope::Global,
            )],
        ));
        m.config.set_shortcut_override(
            "com.example.a",
            "a.open",
            crate::plugin::registry_state::ShortcutOverride::Key {
                value: vec!["ctrl+shift+r".to_string()],
            },
        );
        let kb = KeybindingSettings::preset_tasty();
        let (key, mods) = ctrl_shift_r();
        assert_eq!(
            match_global_shortcut(&m, &key, mods, &kb),
            Some(("com.example.a".to_string(), "a.open".to_string()))
        );
    }

    // ── match_plugin_shortcut (focused-surface path, 회귀) ────────

    #[test]
    fn match_plugin_shortcut_matches_within_focused_plugin_only() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.open",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Surface,
            )],
        ));
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.b",
            vec![cmd(
                "b.open",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Surface,
            )],
        ));
        let kb = KeybindingSettings::preset_tasty();
        let (key, mods) = ctrl_shift_r();
        assert_eq!(
            match_plugin_shortcut(&m, "com.example.a", &key, mods, &kb),
            Some("a.open".to_string())
        );
        // 다른 plugin 이름으로는 같은 키라도 매칭되지 않는다 (per-plugin 격리).
        assert_eq!(
            match_plugin_shortcut(&m, "com.example.ghost", &key, mods, &kb),
            None
        );
    }

    #[test]
    fn modifier_only_press_never_matches() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.open",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Global,
            )],
        ));
        let kb = KeybindingSettings::preset_tasty();
        let mods = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(
            match_global_shortcut(&m, &Key::Named(NamedKey::Control), mods, &kb),
            None
        );
    }

    // ── dispatch/emit: 플러그인 프로세스 미기동 상태에서 panic 없이 no-op ──

    #[test]
    fn dispatch_plugin_command_with_no_surface_does_not_panic() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.open",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Global,
            )],
        ));
        // plugin process가 실행 중이 아니므로 emit/legacy IPC 모두 조용히 no-op.
        dispatch_plugin_command(&mut m, "com.example.a", "a.open", None);
    }

    #[test]
    fn dispatch_plugin_command_with_surface_does_not_panic() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd(
                "a.open",
                "ctrl+shift+r",
                tasty_plugin_manifest::CommandScope::Surface,
            )],
        ));
        dispatch_plugin_command(&mut m, "com.example.a", "a.open", Some(7));
    }

    #[test]
    fn emit_command_invoked_unknown_command_defaults_scope_without_panic() {
        let mut m = mgr();
        // registry에 없는 command — scope는 CommandScope::default()(Global)로 폴백.
        emit_command_invoked(&mut m, "com.example.ghost", "ghost.cmd", None);
    }
}
