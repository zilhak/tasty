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

use crate::app::App;
use crate::view;
use crate::view::ui::View as _;

impl App {
    /// Open a modal, registering it in the unified window map.
    /// 모달도 일반 윈도우와 같은 `windows` 맵에 저장되며, `active_modal_id`로 식별된다.
    pub(crate) fn open_modal(
        &mut self,
        modal: Box<dyn crate::view::ui::View>,
        window_id: WindowId,
    ) {
        self.view.windows.insert(window_id, modal);
        self.view.active_modal_id = Some(window_id);
    }

    /// Close the active modal and handle modal-specific cleanup.
    pub(crate) fn close_active_modal(&mut self) {
        let Some(modal_id) = self.view.active_modal_id.take() else {
            return;
        };
        let Some(mut modal) = self.view.windows.remove(&modal_id) else {
            return;
        };
        // If it was a settings modal, apply settings to all main windows
        if let Some(settings_modal) = modal.as_any_mut().downcast_mut::<view::SettingsWindow>() {
            let new_settings = settings_modal.settings.clone();
            // Plugin shortcut override draft 회수 — modal-specific (settings 와 별 경로).
            let plugin_draft = settings_modal.take_plugin_shortcut_draft();

            // Settings cascade 는 Core 발행 → handle_core_event 통해 처리
            // (main/parked 갱신 + save + theme install + plugin event).
            // cascade 는 동일 frame 의 dispatch_pending_intents 의 domain_batch 단계에서
            // 적용 — modal 닫힌 직후 후속 코드 (settings_open=false / plugin
            // shortcut draft 적용) 는 cascade 결과를 보지 않으므로 지연 안전.
            //
            // 첫 main window 의 state 를 통해 발화. parked-only 상태에서도 modal
            // 은 열릴 수 있으나, parked-only 시에는 본 분기에 들어오지 않는다
            // (Settings modal 은 main window 가 있어야 열 수 있음).
            if let Some(main) = self.main_windows_iter_mut().next() {
                main.state.dispatch_intent(
                    crate::core::intent::DomainIntent::UpdateSettings(new_settings)
                        .from_user_menu("settings_save"),
                );
            } else {
                tracing::warn!(
                    "Settings modal closed but no main window to dispatch UpdateSettings"
                );
            }

            // modal-specific close 처리 — settings_open=false + plugin shortcut 반영.
            for main in self.main_windows_iter_mut() {
                main.state.settings_open = false;
            }
            self.apply_plugin_shortcut_draft(plugin_draft);
        } else if modal.as_any().is::<view::PluginsWindow>() {
            for main in self.main_windows_iter_mut() {
                main.state.plugins_open = false;
                main.mark_dirty();
            }
        }
    }
}
