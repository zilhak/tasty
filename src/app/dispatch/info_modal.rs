//! 안내 모달(InfoModal) 버튼의 요청 슬롯 드레인.
//!
//! popup draw_fn 은 `AppState` 만 들고 있어 winit `EventLoopProxy` 에 닿지 않는다.
//! 그래서 [권한 설정 열기] 는 `state.dialogs.permission_settings_requested` 를 세우기만
//! 하고, 설정 창을 실제로 여는 것은 프레임 시작의 이 드레인이 한다 — file handler
//! picker 의 result 슬롯(`super::picker`)과 같은 형태다.
//!
//! 별도 함수로 둔 이유: 기존 드레인은 이름도 주석도 picker 전용이라, 안내 모달의
//! 요청을 거기 얹으면 이름과 내용이 어긋난다.

use crate::app::App;
use crate::view::ui::View;

impl App {
    /// 안내 모달이 요청한 "Tasty 권한 화면 열기" 를 처리한다.
    ///
    /// 설정 창이 **이미 열려 있으면** 그 창의 탭만 바꾸고 포커스를 준다. 그 갈래를
    /// 따로 두는 이유는 `open_settings_modal` 이 모달이 이미 있으면 즉시 return 하기
    /// 때문이다 — `AppEvent::OpenSettings` 만 보내면 아무 일도 안 일어난 것처럼 보이고,
    /// 세워 둔 1 회성 플래그가 소비되지 않은 채 남아 **다음번에 설정 창을 열 때** 엉뚱하게
    /// 권한 탭으로 튄다. 그래서 이 갈래에서는 플래그를 세우지 않는다.
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
            // 열려 있는 모달이 설정 창이면 그 자리에서 탭을 바꾼다. Plugins 처럼 다른
            // 모달이면 설정 창을 열 수 없으므로(위 early return) 아무것도 하지 않는다.
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
