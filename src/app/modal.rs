//! 모달 윈도우 관리.
//!
//! 모달은 일반 윈도우와 같은 `windows` 맵에 저장되고 `view.active_modal_id`
//! 로 식별된다. 한 번에 최대 1개만 활성 (App 전역 불변식).

pub(crate) mod plugins;
pub(crate) mod preset;
pub(crate) mod quit;
pub(crate) mod settings;
pub(crate) mod shake;

use winit::window::WindowId;

use crate::adapters::ui::window;
use crate::adapters::ui::window::Window as _;
use crate::app::App;

impl App {
    /// Open a modal, registering it in the unified window map.
    /// 모달도 일반 윈도우와 같은 `windows` 맵에 저장되며, `active_modal_id`로 식별된다.
    pub(crate) fn open_modal(&mut self, modal: Box<dyn window::Window>, window_id: WindowId) {
        self.windows.insert(window_id, modal);
        self.view.active_modal_id = Some(window_id);
    }

    /// Close the active modal and handle modal-specific cleanup.
    pub(crate) fn close_active_modal(&mut self) {
        let Some(modal_id) = self.view.active_modal_id.take() else {
            return;
        };
        let Some(mut modal) = self.windows.remove(&modal_id) else {
            return;
        };
        // If it was a settings modal, apply settings to all main windows
        if let Some(settings_modal) = modal.as_any_mut().downcast_mut::<window::SettingsWindow>() {
            let new_settings = settings_modal.settings.clone();
            // Plugin shortcut override draft 회수 — modal-specific (settings 와 별 경로).
            let plugin_draft = settings_modal.take_plugin_shortcut_draft();

            // Settings cascade 는 Core 발행 → handle_core_event 통해 처리
            // (main/parked 갱신 + save + theme install + plugin event).
            if let Err(e) = self.dispatch_core_intent(
                crate::core::intent::CoreIntent::UpdateSettings(new_settings),
            ) {
                tracing::warn!("dispatch UpdateSettings failed: {e}");
            }

            // modal-specific close 처리 — settings_open=false + plugin shortcut 반영.
            for main in self.main_windows_iter_mut() {
                main.state.settings_open = false;
            }
            self.apply_plugin_shortcut_draft(plugin_draft);
        } else if modal.as_any().is::<window::PluginsWindow>() {
            for main in self.main_windows_iter_mut() {
                main.state.plugins_open = false;
                main.mark_dirty();
            }
        }
    }
}
