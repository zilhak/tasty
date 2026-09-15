//! `ToolAction::OpenPopup` 큐 drain → `PluginManager::open_popup_instance`.

use crate::app::App;

impl App {
    /// `ToolAction::OpenPopup` 클릭으로 enqueue된 popup 큐를 모든 AppState에서 drain해
    /// `PluginManager::open_popup_instance`로 dispatch한다. plugin이 실행 중이 아니면
    /// `open_popup_instance`가 자체적으로 warn 후 무시.
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
            // 소속 surface 는 매니페스트가 `scope = "surface"` 를 선언했을 때만 의미가 있다.
            // 바인딩 자체는 선언과 무관하게 해 두고, 렌더(`popup_scope::popup_scope`)가
            // 선언을 보고 쓸지 정한다 — 판정 자리를 하나로 둔다.
            if let Some(sid) = open.target_surface {
                mgr.bind_popup_instance_surface(instance_id, sid);
            }
        }
    }
}
