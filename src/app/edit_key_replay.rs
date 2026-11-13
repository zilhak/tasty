//! 창별 편집 키 판정의 App 쪽 연결. 보조 창과 부팅 오류 화면에 적용된 편집 바인딩을 넣고, 메인 창에서
//! 보류가 풀린 편집 키를 그 창의 평소 이벤트 경로로 재생한다. 원래 키는 이미 판정한 편집으로 한 번만
//! 넘기고, 보류 중에 온 이벤트는 받은 순서대로 다시 처리한다.
//! 계약: `docs/design/policies/key-mapping.md`의 "편집 키 판정" 절.

use winit::window::WindowId;

use crate::adapters::ui::input::edit_key::EditBindings;
use crate::app::App;

impl App {
    /// 최신 적용 설정의 편집 바인딩. 설정 창의 편집 초안이 아니라 적용한 값이다.
    pub(crate) fn applied_edit_bindings(&self) -> Option<EditBindings> {
        self.services
            .latest_settings()
            .map(|s| EditBindings::from_settings(&s.keybindings))
    }

    /// 메인 창이 아닌 창(설정·Preset·Plugins·종료 확인)에 적용된 편집 바인딩을 넣는다. 열린 동안 바인딩을
    /// 바꿔도 다음 키부터 따라간다. 메인 창은 자기 engine 설정으로 판정한다.
    pub(crate) fn sync_auxiliary_edit_bindings(&mut self, id: WindowId) {
        let bindings = self.applied_edit_bindings();
        if let Some(view) = self.view.views.get_mut(&id)
            && view.as_main().is_none()
        {
            view.base_mut().gpu.set_edit_bindings(bindings);
        }
    }

    pub(crate) fn replay_main_edit_keys(&mut self, id: WindowId) {
        let Some(release) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_main_mut())
            .and_then(|main| main.base.gpu.take_edit_release())
        else {
            return;
        };
        if let Some((event, edit)) = release.replay {
            self.set_edit_replay(id, Some(edit));
            self.dispatch_window_event_to_view(id, event);
            // 창이 키를 받지 않고 돌아갔으면 남은 표시가 다음 키에 붙지 않게 지운다.
            self.set_edit_replay(id, None);
        }
        for event in release.queued {
            self.dispatch_window_event_to_view(id, event);
        }
    }

    fn set_edit_replay(
        &mut self,
        id: WindowId,
        edit: Option<crate::adapters::ui::input::edit_key::EditKey>,
    ) {
        if let Some(main) = self.view.views.get_mut(&id).and_then(|v| v.as_main_mut()) {
            main.edit_replay = edit;
        }
    }
}
