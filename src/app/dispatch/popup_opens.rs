//! `ToolAction::OpenPopup` 큐 drain → `PluginManager::open_popup_instance`.

use crate::app::App;

impl App {
    /// `ToolAction::OpenPopup` 클릭으로 enqueue된 popup 큐를 모든 AppState에서 drain해
    /// `PluginManager::open_popup_instance`로 dispatch한다. plugin이 실행 중이 아니면
    /// `open_popup_instance`가 자체적으로 warn 후 무시.
    pub(crate) fn dispatch_pending_popup_opens(&mut self) {
        let mut drained: Vec<(String, String, serde_json::Value)> = Vec::new();
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
        for (plugin_id, popup_id, context) in drained {
            mgr.open_popup_instance(&plugin_id, &popup_id, context);
        }
    }
}
