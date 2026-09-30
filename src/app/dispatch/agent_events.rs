//! GUI가 가진 모든 엔진의 에이전트 이벤트를 모아 공통 전달 함수로 보낸다.

use crate::app::App;

impl App {
    /// 창뿐 아니라 아직 App에 있거나 parked 상태인 CoreState도 포함한다.
    pub(crate) fn dispatch_pending_agent_events(&mut self) {
        use crate::app::agent_events::{emit, take_from};

        let mut events = Vec::new();
        let mut dropped = 0u64;
        let engines = self.engines();
        // 임시 engine을 창·parked보다 먼저 본다.
        for engine in engines
            .pending()
            .into_iter()
            .chain(engines.windowed_and_parked())
        {
            take_from(engine.task_scope.event_queue(), &mut events, &mut dropped);
        }
        emit(self.plugin_manager.as_mut(), events, dropped);
    }
}
