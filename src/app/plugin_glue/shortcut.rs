//! Plugin 명령 단축키 관련 — draft 적용, snapshot, 키 입력 매칭.

use crate::app::App;
use crate::plugin::registry_state::shortcut_override_display;
use crate::{plugin, settings_ui, shortcuts};

/// `apply_plugin_shortcut_draft`의 단일 (plugin_id, command_id) 항목 적용.
/// 값이 `Some(ov)`이면 set, `None`이면 clear. 실제로 값이 바뀐 경우에만
/// `command.shortcut_changed` emit 용 튜플을 반환한다.
fn apply_single_shortcut_override(
    mgr: &mut plugin::PluginManager,
    plugin_id: String,
    command_id: String,
    value: Option<plugin::registry_state::ShortcutOverride>,
) -> Option<(String, String, Option<String>, Option<String>)> {
    let prev_display =
        shortcut_override_display(mgr.config.shortcut_override(&plugin_id, &command_id));
    let new_display = match &value {
        Some(ov) => shortcut_override_display(Some(ov)),
        None => None,
    };
    let local_changed = match value {
        Some(ov) => {
            mgr.config
                .set_shortcut_override(&plugin_id, &command_id, ov);
            true
        }
        None => mgr.config.clear_shortcut_override(&plugin_id, &command_id),
    };
    local_changed.then_some((plugin_id, command_id, new_display, prev_display))
}

/// `apply_plugin_shortcut_draft`에서 모인 변경분에 대해
/// `command.shortcut_changed` host event를 순서대로 emit.
fn emit_shortcut_changed_events(
    mgr: &mut plugin::PluginManager,
    emit_queue: Vec<(String, String, Option<String>, Option<String>)>,
) {
    for (plugin_id, command_id, shortcut, prev_shortcut) in emit_queue {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::payloads::CommandShortcutChanged;
        let payload = CommandShortcutChanged {
            plugin_id,
            command_id,
            shortcut,
            prev_shortcut,
        };
        mgr.emit_host_event("command.shortcut_changed", &payload, EventScope::System);
    }
}

impl App {
    /// SettingsView가 회수해 온 plugin shortcut override draft를 PluginsConfig에
    /// 반영하고 디스크에 저장. 값이 `Some(ov)`이면 set, `None`이면 clear.
    pub(crate) fn apply_plugin_shortcut_draft(
        &mut self,
        draft: std::collections::BTreeMap<
            (String, String),
            Option<plugin::registry_state::ShortcutOverride>,
        >,
    ) {
        if draft.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            tracing::warn!("plugin shortcut draft dropped: plugin manager not initialized");
            return;
        };
        let mut changed = false;
        let mut emit_queue: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();
        for ((plugin_id, command_id), value) in draft {
            if let Some(entry) = apply_single_shortcut_override(mgr, plugin_id, command_id, value) {
                changed = true;
                emit_queue.push(entry);
            }
        }
        if changed {
            if let Err(e) = mgr.config.save() {
                tracing::warn!("plugins.toml save failed after shortcut update: {e}");
            }
            emit_shortcut_changed_events(mgr, emit_queue);
        }
    }

    /// Plugins 키바인딩 서브탭에 표시할 snapshot.
    pub(crate) fn snapshot_plugin_shortcuts(&self) -> settings_ui::PluginShortcutSnapshot {
        let Some(mgr) = self.plugin_manager.as_ref() else {
            return settings_ui::PluginShortcutSnapshot::default();
        };
        // plugin_id → display name map (매니페스트의 name).
        let name_for: std::collections::HashMap<&str, &str> = mgr
            .packages()
            .iter()
            .map(|p| (p.manifest.id.as_str(), p.manifest.name.as_str()))
            .collect();

        let rows: Vec<settings_ui::PluginShortcutRow> = mgr
            .command_registry
            .iter_all()
            .map(|e| settings_ui::PluginShortcutRow {
                plugin_id: e.plugin_id.clone(),
                plugin_name: name_for
                    .get(e.plugin_id.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.plugin_id.clone()),
                command_id: e.command_id.clone(),
                title_i18n_key: e.title_i18n_key.clone(),
                binding_mode: e.binding_mode.clone(),
                manifest_default: e.manifest_default.clone(),
                current_override: mgr
                    .config
                    .shortcut_override(&e.plugin_id, &e.command_id)
                    .cloned(),
            })
            .collect();
        settings_ui::PluginShortcutSnapshot { rows }
    }

    /// 사용자 키 입력이 plugin 명령에 매칭되면 dispatch 한다. 호출자(event_handler)는
    /// normal window dispatch를 skip해 host action이 trigger되지 않게 한다.
    ///
    /// 우선순위: 포커스된 plugin surface가 있으면 **그 plugin의 커맨드**(scope 무관 —
    /// 이미 포커스 조건을 만족)만 후보로 본다. 없으면 등록된 **모든** plugin의
    /// `CommandScope::Global` 커맨드를 후보로 본다 — `scope = "global"`(기본값)의
    /// "어디서나 동작" 계약을 실제로 만족시키는 경로. `Surface` scope 커맨드는 이
    /// 두번째 경로에 나타나지 않으므로 회귀 없이 그대로 "owner surface 포커스 시에만
    /// 동작"을 유지한다.
    pub(crate) fn try_plugin_shortcut(
        &mut self,
        id: winit::window::WindowId,
        ke: &winit::event::KeyEvent,
    ) -> bool {
        use winit::event::ElementState;
        if ke.state != ElementState::Pressed {
            return false;
        }
        let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
            return false;
        };
        // physical key fallback (IME 영향 회피) — keyboard.rs와 동일 규칙
        let mods = main.base.modifiers;
        let shortcut_key = if mods.control_key() || mods.super_key() || mods.alt_key() {
            shortcuts::physical_key_to_logical(&ke.physical_key)
                .unwrap_or_else(|| ke.logical_key.clone())
        } else {
            ke.logical_key.clone()
        };
        self.dispatch_plugin_shortcut_key(id, &shortcut_key, mods)
    }

    /// `(key, mods)` 만으로 plugin 명령 단축키를 매칭·실행한다. winit `KeyEvent` 경로
    /// (`try_plugin_shortcut`)와 native webview 포워딩 경로가 **같은 게이트·같은
    /// 우선순위**를 쓰도록 실제 판정을 여기 한 곳에 둔다. `winit::event::KeyEvent` 는
    /// 합성 생성이 불가능하므로 포워딩 경로는 이 진입점을 쓴다.
    pub(crate) fn dispatch_plugin_shortcut_key(
        &mut self,
        id: winit::window::WindowId,
        shortcut_key: &winit::keyboard::Key,
        mods: winit::keyboard::ModifiersState,
    ) -> bool {
        // Modal이 활성화되면 plugin shortcut은 동작하지 않는다.
        if self.view.is_modal_active() {
            return false;
        }
        let Some(w) = self.view.views.get_mut(&id) else {
            return false;
        };
        let Some(main) = w.as_main_mut() else {
            return false;
        };
        // overlay/popup이 키를 가져갈 상태면 patcher. plugin popup 도 포함한다 —
        // 그 popup 이 키를 받는 동안 surface 단축키가 같은 키를 또 소비하면 이중
        // 처리다(키 게이트와 같은 단일 출처를 쓴다).
        //
        // 전체화면 무대도 같이 막는다. 이 경로는 `dispatch_window_event_to_view` **이전에**
        // 호출되므로(`app/event_handler.rs`) `keyboard.rs` 의 0단계 무대 게이트가 아예 도달하지
        // 못한다 — 무대 중 plugin 단축키 발화는 여기서 직접 막아야 한다.
        if main.state.keyboard_overlay_open() || main.state.fullscreen_stage_active() {
            return false;
        }
        let focused = crate::plugin_bridge::key_dispatch::focused_plugin_surface(
            &main.state,
            &main.core_state,
        );
        let host_kb = main.core_state.settings.keybindings.clone();

        // (plugin_id, command_id, 대상 surface — Surface 경로만 Some) 매칭 결과.
        let matched = {
            let Some(mgr) = self.plugin_manager.as_ref() else {
                return false;
            };
            match &focused {
                Some((plugin_id, surface_id)) => {
                    crate::plugin_bridge::key_dispatch::match_plugin_shortcut(
                        mgr,
                        plugin_id,
                        shortcut_key,
                        mods,
                        &host_kb,
                    )
                    .map(|cmd_id| (plugin_id.clone(), cmd_id, Some(*surface_id)))
                }
                None => crate::plugin_bridge::key_dispatch::match_global_shortcut(
                    mgr,
                    shortcut_key,
                    mods,
                    &host_kb,
                )
                .map(|(plugin_id, cmd_id)| (plugin_id, cmd_id, None)),
            }
        };
        let Some((plugin_id, cmd_id, surface_id)) = matched else {
            return false;
        };

        let action = self
            .plugin_manager
            .as_ref()
            .and_then(|mgr| mgr.command_registry.find(&plugin_id, &cmd_id))
            .and_then(|e| e.action.clone());

        if let Some(action) = action {
            // action이 선언된 command: 호스트가 직접 실행 (`[[contributes.tool]]`과 동일
            // 처리). Event Bus `command.invoked`는 informational로 여전히 발사하지만,
            // 옛 `command.invoke` IPC(`handle_command`)는 이 경로에서 아예 발사하지
            // 않는다 — action과 handle_command 동시 실행 시 popup 중복 오픈 등의
            // 부작용을 막기 위함(`CommandDecl::action` 문서 참조).
            if let Some(mgr) = self.plugin_manager.as_mut() {
                crate::plugin_bridge::key_dispatch::emit_command_invoked(
                    mgr, &plugin_id, &cmd_id, surface_id,
                );
            }
            let item = plugin::tool_registry::ToolItem {
                source: plugin::tool_registry::ToolSource::Plugin {
                    plugin_id: plugin_id.clone(),
                    tool_id: cmd_id.clone(),
                },
                key: format!("{plugin_id}/{cmd_id}"),
                label_i18n_key: String::new(),
                icon: None,
                action,
                order_hint: 0,
            };
            crate::adapters::ui::tools_menu::invoke_tool(
                &mut main.state,
                &mut main.core_state,
                &item,
            );
        } else if let Some(mgr) = self.plugin_manager.as_mut() {
            crate::plugin_bridge::key_dispatch::dispatch_plugin_command(
                mgr, &plugin_id, &cmd_id, surface_id,
            );
        }
        true
    }
}
