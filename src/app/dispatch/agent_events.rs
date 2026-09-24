//! GUI가 가진 모든 엔진의 에이전트 이벤트를 모아 공통 전달 함수로 보낸다.

use crate::app::App;

impl App {
    /// 창뿐 아니라 아직 App에 있거나 parked 상태인 CoreState도 포함한다.
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
