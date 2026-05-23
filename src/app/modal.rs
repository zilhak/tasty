//! 모달 윈도우 관리.
//!
//! 모달은 일반 윈도우와 같은 `windows` 맵에 저장되고 `engine.active_modal_id`
//! 로 식별된다. 한 번에 최대 1개만 활성 (engine 전역 불변식).

pub(crate) mod plugins;
pub(crate) mod preset;
pub(crate) mod quit;
pub(crate) mod settings;
pub(crate) mod shake;

use winit::window::WindowId;

use crate::app::App;
use crate::window;
use crate::window::Window as _;

impl App {
    /// Open a modal, registering it in the unified window map.
    /// 모달도 일반 윈도우와 같은 `windows` 맵에 저장되며, `active_modal_id`로 식별된다.
    pub(crate) fn open_modal(&mut self, modal: Box<dyn window::Window>, window_id: WindowId) {
        self.windows.insert(window_id, modal);
        self.engine.active_modal_id = Some(window_id);
    }

    /// Close the active modal and handle modal-specific cleanup.
    pub(crate) fn close_active_modal(&mut self) {
        let Some(modal_id) = self.engine.active_modal_id.take() else {
            return;
        };
        let Some(mut modal) = self.windows.remove(&modal_id) else {
            return;
        };
        // If it was a settings modal, apply settings to all main windows
        if let Some(settings_modal) = modal.as_any_mut().downcast_mut::<window::SettingsWindow>() {
            let new_settings = settings_modal.settings.clone();
            // Plugin shortcut override draft 회수 — 변경된 키만 plugins.toml에 반영.
            let plugin_draft = settings_modal.take_plugin_shortcut_draft();
            // theme/language 변경 감지용 prev 값 — 첫 main window의 현재 설정 기준.
            // SettingsWindow는 단일 SoT라 prev/new는 글로벌 비교로 충분.
            let prev_theme = self
                .main_windows_iter_mut()
                .next()
                .map(|w| w.engine_state.settings.appearance.theme.clone());
            let prev_language = self
                .main_windows_iter_mut()
                .next()
                .map(|w| w.engine_state.settings.general.language.clone());
            for main in self.main_windows_iter_mut() {
                main.engine_state.settings = new_settings.clone();
                main.state.settings_open = false;
                main.mark_dirty();
            }
            if let Err(e) = new_settings.save() {
                tracing::warn!("failed to save settings: {e}");
            }
            self.apply_plugin_shortcut_draft(plugin_draft);
            // Event Bus 1.0: theme/language 변경 발화.
            if let Some(mgr) = self.plugin_manager.as_mut() {
                use tasty_plugin_protocol::EventScope;
                use tasty_plugin_protocol::events::payloads::{LanguageChanged, ThemeChanged};
                if prev_theme.as_deref() != Some(new_settings.appearance.theme.as_str()) {
                    mgr.emit_host_event(
                        "theme.changed",
                        &ThemeChanged {
                            theme_id: new_settings.appearance.theme.clone(),
                        },
                        EventScope::System,
                    );
                }
                if prev_language.as_deref() != Some(new_settings.general.language.as_str()) {
                    mgr.emit_host_event(
                        "language.changed",
                        &LanguageChanged {
                            language_code: new_settings.general.language.clone(),
                        },
                        EventScope::System,
                    );
                }
            }
        } else if modal.as_any().is::<window::PluginsWindow>() {
            for main in self.main_windows_iter_mut() {
                main.state.plugins_open = false;
                main.mark_dirty();
            }
        }
    }
}
