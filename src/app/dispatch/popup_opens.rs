//! 도구 메뉴의 popup 열기 요청을 플러그인 매니저로 전달한다.

use crate::app::App;

impl App {
    pub(crate) fn dispatch_pending_popup_opens(&mut self) {
        let mut drained: Vec<crate::state::PendingPopupOpen> = Vec::new();
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.append(&mut main.state.pending_popup_opens);
            }
        }
        for (s, _engine) in &mut self.parked_states {
            drained.append(&mut s.pending_popup_opens);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for open in drained {
            let Some(instance_id) =
                mgr.open_popup_instance(&open.plugin_id, &open.popup_id, open.context)
            else {
                continue;
            };
            // surface 바인딩은 기록하되 실제 범위 적용은 매니페스트를 읽는 렌더 경로에 맡긴다.
            if let Some(sid) = open.target_surface {
                mgr.bind_popup_instance_surface(instance_id, sid);
            }
        }
    }
}
