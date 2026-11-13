//! 메인 창에서 보류가 풀린 편집 키를 그 창의 평소 이벤트 경로로 재생한다. 원래 키는 이미 판정한
//! 편집으로 한 번만 넘기고, 보류 중에 온 이벤트는 받은 순서대로 다시 처리한다.
//! 계약: `docs/design/policies/key-mapping.md`의 "편집 키 판정" 절.

use winit::window::WindowId;

use crate::app::App;

impl App {
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
