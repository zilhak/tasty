//! 도구 메뉴가 요청한 이벤트를 플러그인에 전달한다.

use crate::app::App;

impl App {
    /// 이벤트 payload는 플러그인이 정의한 JSON 그대로 전달한다.
    pub(crate) fn dispatch_pending_tool_events(&mut self) {
        let mut drained: Vec<(String, serde_json::Value)> = Vec::new();
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.append(&mut main.state.pending_tool_events);
            }
        }
        for (s, _engine) in &mut self.parked_states {
            drained.append(&mut s.pending_tool_events);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for (key, payload) in drained {
            // 호스트 이벤트는 플러그인 발행 경로의 publish 권한·선언 검사 대상이 아니다.
            mgr.emit_host_event(&key, &payload, tasty_plugin_protocol::EventScope::System);
        }
    }
}
