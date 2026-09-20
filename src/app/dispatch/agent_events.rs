//! agent 도메인 사건을 Event Bus 로 broadcast — **gui 조합의 드레인**.
//!
//! 여기 있는 것은 "엔진 사본이 여럿이다" 는 이 조합만의 사정뿐이다. 꺼내서
//! 내보내는 규칙은 `crate::app::agent_events` 가 헤드리스와 공유한다. 같은 모양이
//! 옆에 하나 더 있다(`memory_changes.rs`).

use crate::app::App;

impl App {
    /// `agent` 사건 큐를 drain 해 `agent.*` host event 로 broadcast.
    ///
    /// **엔진 사본이 여럿이다.** `CoreState` 는 부팅 직후 `App.core_state` 에 있다가
    /// 첫 MainView 로 옮겨 가고, 창마다 하나씩 있으며 parked 짝도 자기 것을 든다.
    /// 한 사본만 꺼내면 다른 창에서 종결된 task 의 사건이 그 큐에 영원히 남는다 —
    /// 옆의 `host_events.rs` 가 같은 이유로 전 사본을 순회한다.
    pub(crate) fn dispatch_pending_agent_events(&mut self) {
        use crate::app::agent_events::{emit, take_from};

        let mut events = Vec::new();
        let mut dropped = 0u64;
        if let Some(engine) = self.core_state.as_ref() {
            take_from(&engine.agent_event_queue, &mut events, &mut dropped);
        }
        for (_win_id, w) in self.view.views.iter() {
            if let Some(main) = w.as_main() {
                take_from(
                    &main.core_state.agent_event_queue,
                    &mut events,
                    &mut dropped,
                );
            }
        }
        for (_s, engine) in self.parked_states.iter() {
            take_from(&engine.agent_event_queue, &mut events, &mut dropped);
        }
        emit(self.plugin_manager.as_mut(), events, dropped);
    }
}
