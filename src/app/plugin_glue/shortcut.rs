//! Plugin 명령 단축키 관련 — draft 적용, snapshot, 키 입력 매칭.

use crate::app::App;
use crate::plugin::registry_state::shortcut_override_display;
use crate::{plugin, settings_ui, shortcuts};

impl App {
    /// SettingsWindow가 회수해 온 plugin shortcut override draft를 PluginsConfig에
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
            if local_changed {
                changed = true;
                emit_queue.push((plugin_id, command_id, new_display, prev_display));
            }
        }
        if changed {
            if let Err(e) = mgr.config.save() {
                tracing::warn!("plugins.toml save failed after shortcut update: {e}");
            }
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
    }

    /// Plugins 키바인딩 서브탭에 표시할 snapshot.
    pub(crate) fn snapshot_plugin_shortcuts(&self) -> settings_ui::PluginShortcutSnapshot {
        let Some(mgr) = self.plugin_manager.as_ref() else {
            return settings_ui::PluginShortcutSnapshot::default();
        };
        // plugin_id → display name map (매니페스트의 name).
        let name_for: std::collections::HashMap<&str, &str> = mgr
            .packages
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

    /// 사용자 키 입력이 현재 포커스 surface 의 plugin 명령에 매칭되면 dispatch 한다.
    /// 호출자(event_handler)는 normal window dispatch를 skip해 host action이
    /// trigger되지 않게 한다.
    pub(crate) fn try_plugin_shortcut(
        &mut self,
        id: winit::window::WindowId,
        ke: &winit::event::KeyEvent,
    ) -> bool {
        use winit::event::ElementState;
        if ke.state != ElementState::Pressed {
            return false;
        }
        // Modal이 활성화되면 plugin shortcut은 동작하지 않는다.
        if self.view.is_modal_active() {
            return false;
        }
        let Some(w) = self.windows.get(&id) else {
            return false;
        };
        let Some(main) = w.as_main() else {
            return false;
        };
        // overlay/popup이 키를 가져갈 상태면 patcher
        if main.state.settings_open
            || main.state.has_input_dialog_open()
            || main.state.popups.has_focused()
        {
            return false;
        }
        let Some((plugin_id, surface_id)) =
            plugin::key_dispatch::focused_plugin_surface(&main.state, &main.engine_state)
        else {
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
        let host_kb = main.engine_state.settings.keybindings.clone();
        let cmd_id = {
            let Some(mgr) = self.plugin_manager.as_ref() else {
                return false;
            };
            plugin::key_dispatch::match_plugin_shortcut(
                mgr,
                &plugin_id,
                &shortcut_key,
                mods,
                &host_kb,
            )
        };
        let Some(cmd_id) = cmd_id else {
            return false;
        };
        if let Some(mgr) = self.plugin_manager.as_mut() {
            plugin::key_dispatch::dispatch_plugin_command(mgr, &plugin_id, &cmd_id, surface_id);
        }
        true
    }
}
