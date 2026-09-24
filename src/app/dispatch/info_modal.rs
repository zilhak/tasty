//! 안내 모달 버튼이 남긴 요청을 프레임 시작에 처리한다.
//!
//! 팝업을 그리는 함수는 `AppState`만 가지고 있어 winit 이벤트 루프에 접근할 수 없다.
//! 그래서 [권한 설정 열기]는 `state.dialogs.permission_settings_requested`를 표시만
//! 해 두고, 설정 창을 실제로 여는 일은 여기서 한다. 파일 핸들러 피커의 결과를 처리하는
//! `super::picker`와 같은 방식이다.
//!
//! 피커 쪽 처리에 함께 얹지 않고 별도 함수로 둔 것은, 그 함수의 이름과 설명이 피커
//! 전용이어서 안내 모달의 요청까지 넣으면 내용과 맞지 않기 때문이다.

use crate::app::App;
use crate::view::ui::View;

impl App {
    /// 안내 모달이 요청한 권한 화면 열기를 처리한다.
    ///
    /// 설정 창이 이미 열려 있으면 새로 열지 않고 그 창의 탭만 바꾼 뒤 포커스를 준다.
    /// `open_settings_modal`은 모달이 이미 있으면 바로 반환하므로, 이 경우에
    /// `AppEvent::OpenSettings`만 보내면 아무 일도 일어나지 않는다. 그러면 탭 지정
    /// 플래그가 쓰이지 않은 채 남아서 다음에 설정 창을 열 때 엉뚱하게 권한 탭이 열린다.
    /// 그래서 이미 열려 있는 경우에는 플래그를 세우지 않는다.
    pub(crate) fn dispatch_pending_info_modal_requests(&mut self) {
        let mut requested = false;
        for view in self.view.views.values_mut() {
            if let Some(main) = view.as_main_mut()
                && std::mem::take(&mut main.state.dialogs.permission_settings_requested)
            {
                requested = true;
            }
        }
        if !requested {
            return;
        }

        if let Some(modal_id) = self.view.active_modal_id {
            // 열려 있는 모달이 설정 창이면 그 자리에서 탭을 바꾼다. Plugins처럼 다른
            // 모달이면 설정 창을 열 수 없으므로 아무것도 하지 않는다.
            if let Some(view) = self.view.views.get_mut(&modal_id) {
                if let Some(settings) = view
                    .as_any_mut()
                    .downcast_mut::<crate::view::settings::SettingsView>()
                {
                    settings.focus_macos_permissions_tab();
                    settings.mark_dirty();
                    settings.base().winit.focus_window();
                } else {
                    tracing::debug!("권한 화면 요청: 다른 모달이 열려 있어 넘어간다");
                }
            }
            return;
        }

        self.pending_settings_macos_permissions_tab = true;
        crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
    }
}
