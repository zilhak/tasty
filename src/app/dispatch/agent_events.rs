//! agent 도메인 사건을 Event Bus 로 broadcast.
//!
//! 발화점은 `CoreState::agent_event_queue` 에 사실만 적는다 — 그 자리가 러너 스레드일
//! 수 있고 Event Bus 는 `PluginManager` 가 소유해 메인 스레드에서 fan-out 되기
//! 때문이다. 여기가 프레임마다 그것을 꺼내 내보낸다. 같은 모양이 옆에 하나 더 있다
//! (`memory_changes.rs`).

use crate::app::App;

impl App {
    /// `agent` 사건 큐를 drain 해 `agent.*` host event 로 broadcast.
    ///
    /// **plugin manager 가 없어도 먼저 꺼낸다.** 큐를 두고 돌아가면 다음 프레임에
    /// 그만큼 더 쌓이고, manager 가 끝내 안 생기는 실행(헤드리스)에서는 상한까지
    /// 자란다. 꺼낸 뒤 버리는 쪽이 맞다 — 받을 구독자가 없다는 뜻이므로.
    pub(crate) fn dispatch_pending_agent_events(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::payloads::{AgentBarrierClosed, AgentTaskFinished};

        use crate::core::agent::event_feed::AgentEvent;

        // **엔진 사본이 여럿이다.** `CoreState` 는 부팅 직후 `App.core_state` 에 있다가
        // 첫 MainView 로 옮겨 가고, 창마다 하나씩 있으며 parked 짝도 자기 것을 든다.
        // 한 사본만 꺼내면 다른 창에서 종결된 task 의 사건이 그 큐에 영원히 남는다 —
        // 옆의 `host_events.rs` 가 같은 이유로 전 사본을 순회한다.
        let mut events: Vec<AgentEvent> = Vec::new();
        let mut dropped: u64 = 0;
        {
            let mut drain_one =
                |q: &std::sync::Arc<crate::core::agent::event_feed::AgentEventQueue>| {
                    let (mut e, d) = q.take_pending();
                    events.append(&mut e);
                    dropped = dropped.saturating_add(d);
                };
            if let Some(engine) = self.core_state.as_ref() {
                drain_one(&engine.agent_event_queue);
            }
            for (_win_id, w) in self.view.views.iter() {
                if let Some(main) = w.as_main() {
                    drain_one(&main.core_state.agent_event_queue);
                }
            }
            for (_s, engine) in self.parked_states.iter() {
                drain_one(&engine.agent_event_queue);
            }
        }
        if dropped > 0 {
            tracing::warn!(
                "agent event feed dropped {dropped} event(s) at its ceiling — the feed has a \
                 hole and consumers cannot see it from the events they do receive"
            );
        }
        if events.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ev in events {
            match ev {
                AgentEvent::TaskFinished {
                    workspace_id,
                    task_id,
                    state,
                } => {
                    let payload = AgentTaskFinished {
                        workspace_id,
                        task_id,
                        state: state.to_string(),
                    };
                    mgr.emit_host_event("agent.task_finished", &payload, EventScope::System);
                }
                AgentEvent::BarrierClosed {
                    workspace_id,
                    name,
                    count_required,
                } => {
                    let payload = AgentBarrierClosed {
                        workspace_id,
                        name,
                        count_required,
                    };
                    mgr.emit_host_event("agent.barrier_closed", &payload, EventScope::System);
                }
            }
        }
    }
}
