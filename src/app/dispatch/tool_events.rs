//! 도구 메뉴 클릭으로 enqueue 된 tool 이벤트 publish.

use crate::app::App;

impl App {
    /// 도구 메뉴 클릭으로 enqueue된 이벤트 큐(`pending_tool_events`)를 모든 AppState
    /// 에서 drain해 PluginManager로 publish한다. payload는 plugin 작성자가 정의한 임의
    /// JSON value를 그대로 전달 (현재 `{ "tool_id": "<plugin_id>/<tool_id>" }`).
    pub(crate) fn dispatch_pending_tool_events(&mut self) {
        let mut drained: Vec<(String, serde_json::Value)> = Vec::new();
        for w in self.windows.values_mut() {
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
            // tool 트리거 이벤트는 system scope. 매니페스트 events_emitted에 등록되지 않은
            // 임의 키도 호스트 발화는 허용 (publish 권한 검사는 plugin 발화 경로에만 적용).
            mgr.emit_host_event(&key, &payload, tasty_plugin_protocol::EventScope::System);
        }
    }
}
