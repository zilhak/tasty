//! 모달은 일반 view 맵에 두고 active_modal_id로 구별하며 한 번에 하나만 활성화한다.
//! 종료 확인은 다른 모달과 달리 열기 실패 때도 종료를 계속한다.
//! 공통 첫 프레임 처리는 present_first_frame에 두고 각 창의 실패 정책은 호출부에 남긴다.

pub(crate) mod plugins;
pub(crate) mod preset;
pub(crate) mod quit;
pub(crate) mod settings;
pub(crate) mod shake;

use winit::window::WindowId;

use crate::app::App;
use crate::state::ModalKind;
use crate::view;
use crate::view::ui::View as _;

impl App {
    pub(crate) fn open_modal(
        &mut self,
        modal: Box<dyn crate::view::ui::View>,
        window_id: WindowId,
        kind: ModalKind,
    ) {
        self.view.views.insert(window_id, modal);
        self.view.active_modal_id = Some(window_id);
        // debug ui.state가 읽을 ID·종류를 각 MainView에도 기록한다. parked 상태는 여기서 갱신하지 않는다.
        let raw = u64::from(window_id);
        for main in self.main_windows_iter_mut() {
            main.state.active_modal_id = Some(raw);
            main.state.active_modal_kind = Some(kind);
        }
    }

    pub(crate) fn close_active_modal(&mut self) {
        let Some(modal_id) = self.view.active_modal_id.take() else {
            return;
        };
        for main in self.main_windows_iter_mut() {
            main.state.active_modal_id = None;
            main.state.active_modal_kind = None;
        }
        let Some(mut modal) = self.view.views.remove(&modal_id) else {
            return;
        };
        if let Some(settings_modal) = modal.as_any_mut().downcast_mut::<view::SettingsView>() {
            let new_settings = settings_modal.settings.clone();
            let plugin_draft = settings_modal.take_plugin_shortcut_draft();
            // 모달이 없어지기 전에 저장 실패 사유를 회수해 MainView에 알린다.
            let bashrc_error = settings_modal.take_bashrc_save_error();

            // 설정 변경은 다음 Intent 처리에서 적용한다. MainView가 없으면 요청을 넣지 못하고 경고만 남긴다.
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

            for main in self.main_windows_iter_mut() {
                main.state.settings_open_requested = false;
            }
            self.apply_plugin_shortcut_draft(plugin_draft);
            if let Some(reason) = bashrc_error {
                self.surface_bashrc_save_failure(&reason);
            }
        } else if modal.as_any().is::<view::PluginsView>() {
            for main in self.main_windows_iter_mut() {
                main.state.plugins_open = false;
                main.mark_dirty();
            }
        }
    }
}
