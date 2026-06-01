//! plugin popup 렌더 중 수집된 UiEvent / close 사유 forward.

use crate::app::App;

impl App {
    /// plugin popup 렌더 중 수집된 사용자 입력 / close 사유를 모든 AppState에서 drain해
    /// `PluginManager`로 forward한다. (`send_popup_event` / `close_popup_instance`)
    pub(crate) fn dispatch_plugin_popup_events(&mut self) {
        let mut drained_events: Vec<(u64, tasty_plugin_protocol::ui_tree::UiEvent)> = Vec::new();
        let mut drained_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)> = Vec::new();
        for w in self.view.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained_events.append(&mut main.state.plugin_popup_events);
                drained_closes.append(&mut main.state.plugin_popup_closes);
            }
        }
        for (s, _engine) in &mut self.parked_states {
            drained_events.append(&mut s.plugin_popup_events);
            drained_closes.append(&mut s.plugin_popup_closes);
        }
        if drained_events.is_empty() && drained_closes.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for (instance_id, event) in drained_events {
            mgr.send_popup_event(instance_id, &event);
        }
        // 같은 인스턴스에 대해 close 사유가 여러 번 쌓일 수 있다 (Escape 매 프레임 등).
        // 첫 사유로 close하고 나머지는 무시 — close_popup_instance가 알아서 멱등 처리.
        let mut seen = std::collections::HashSet::new();
        for (instance_id, reason) in drained_closes {
            if seen.insert(instance_id) {
                mgr.close_popup_instance(instance_id, reason);
            }
        }
    }
}
