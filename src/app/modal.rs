//! 모달 윈도우 관리.
//!
//! 모달은 일반 윈도우와 같은 `windows` 맵에 저장되고 `view.active_modal_id`
//! 로 식별된다. 한 번에 최대 1개만 활성 (App 전역 불변식).
//!
//! # 이 디렉토리의 여는 함수 셋이 닮은 것에 대해 — 재 보고 하나만 묶었다
//!
//! `settings` · `plugins` · `quit` 의 여는 본문은 각각 70 · 61 · 54 줄(공백 제외)이고
//! 그 안에서 **글자 단위로 같은 줄이 18** 이다. 그중 6 은 `{` · `}` · `);` 같은 구두점이라
//! 실질은 12 다. 식별자 단위로 다시 세면 12 표지 중 **10 이 셋 다에** 있다 —
//! `WindowAttributes::default()` · 아이콘 · `with_visible(false)` · `create_window` ·
//! `create_gpu_state` · `window.id()` · Windows 첫 프레임 짝 · `open_modal(..)`.
//! `preset` 은 모달로 등록하지는 않지만 창 만드는 부분은 같은 모양이라 사실상 **넷째**이고,
//! 이미 두 조각을 자기 헬퍼로 뽑아 놨다(`preset_window_attributes` · `create_window_or_warn`) —
//! 그런데 형제 셋이 안 따라갔다. 반복이 우연이 아니라는 증거로 이것이 가장 세다.
//!
//! **그런데 셋 다에 있는 것과 묶어도 되는 것은 다르다.** 갈라지는 두 표지가 하필
//! 정책이다: `quit` 만 `is_modal_active()` 로 안 막고(막으면 사용자가 앱을 끌 수 없는
//! 상태에 갇힌다), `quit` 만 실패 시 `notify_window_creation_failed` 대신 **확인 없이
//! 종료로 폴백**한다(ADR-0117). 창 속성도 자리마다 다르다. 이 셋을 한 헬퍼로 묶으려면
//! 실패 정책을 콜백이나 열거로 받아야 하고, 그러면 다음 사람이 "이 창은 실패하면 어떻게
//! 되나" 를 **두 파일을 오가며** 읽게 된다 — 지금은 한 화면에서 끝난다.
//!
//! 그래서 묶은 것은 **정책이 0 인 조각 하나**뿐이다: Windows 에서 첫 프레임을 강제하는
//! `#[cfg]` 짝. 넷 다 수신자 이름만 빼고 **글자까지 같았고**, 빠뜨리면 창이 Windows
//! 에서만 안 뜨고 다른 모든 곳에서는 초록이라 그 플랫폼에 닿기 전까지 신호가 없다.
//! [`crate::view::ui::present_first_frame`] 이 그것이다.
//!
//! ⇒ **나머지를 묶자는 제안이 다시 오면 이 문단이 답이다.** 되짚을 값은 위 두 개(10/12,
//! 그리고 갈라지는 둘이 정책이라는 것)이고, 값이 달라졌으면 — 예컨대 `quit` 의 예외가
//! 사라졌으면 — 그때 다시 판단해라.

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
    /// Open a modal, registering it in the unified window map.
    /// 모달도 일반 윈도우와 같은 `windows` 맵에 저장되며, `active_modal_id`로 식별된다.
    pub(crate) fn open_modal(
        &mut self,
        modal: Box<dyn crate::view::ui::View>,
        window_id: WindowId,
        kind: ModalKind,
    ) {
        self.view.views.insert(window_id, modal);
        self.view.active_modal_id = Some(window_id);
        // `AppState` 쪽 거울 — `ui.state` 가 `View` 에 안 닿아서 둔다. 원본을 세우는 **바로
        // 그 자리에서** 함께 갱신하는 것이 이 사본이 어긋나지 않는 근거다.
        //
        // main window 마다 `AppState` 가 따로라 전부에 쓴다 — 아래 `settings_open_requested = false`
        // 와 같은 형태다. main window 가 없는 상태(parked-only)면 아무 데도 안 쓰이는데,
        // 그때는 읽을 `AppState` 도 없으므로 비대칭이 생기지 않는다.
        //
        // 종류(`kind`)는 **여는 쪽이 준다.** 열린 `View` 를 downcast 해서 되짚지 않는
        // 이유는, 되짚으면 새 모달을 추가한 사람이 열거를 안 늘려도 조용히 `None` 이
        // 되어 "모달이 없다" 와 같은 모양이 되기 때문이다. 인자로 받으면 컴파일이 막는다.
        let raw = u64::from(window_id);
        for main in self.main_windows_iter_mut() {
            main.state.active_modal_id = Some(raw);
            main.state.active_modal_kind = Some(kind);
        }
    }

    /// Close the active modal and handle modal-specific cleanup.
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
        // If it was a settings modal, apply settings to all main windows
        if let Some(settings_modal) = modal.as_any_mut().downcast_mut::<view::SettingsView>() {
            let new_settings = settings_modal.settings.clone();
            // Plugin shortcut override draft 회수 — modal-specific (settings 와 별 경로).
            let plugin_draft = settings_modal.take_plugin_shortcut_draft();
            // bashrc 저장 실패 사유 — 모달이 닫히므로 여기서 회수해 main window 로 올린다.
            let bashrc_error = settings_modal.take_bashrc_save_error();

            // Settings cascade 는 Core 발행 → handle_core_event 통해 처리
            // (main/parked 갱신 + save + theme install + plugin event).
            // cascade 는 동일 frame 의 dispatch_pending_intents 의 domain_batch 단계에서
            // 적용 — modal 닫힌 직후 후속 코드 (settings_open_requested=false / plugin
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

            // modal-specific close 처리 — settings_open_requested=false + plugin shortcut 반영.
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
