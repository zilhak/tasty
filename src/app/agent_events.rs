//! GUI와 헤드리스 루프에서 에이전트 큐를 비우고 플러그인 이벤트로 전달한다.

use std::sync::Arc;

use crate::core::agent::event_feed::{AgentEvent, AgentEventQueue};
use crate::plugin::PluginManager;

pub(crate) fn take_from(q: &Arc<AgentEventQueue>, events: &mut Vec<AgentEvent>, dropped: &mut u64) {
    let (mut e, d) = q.take_pending();
    events.append(&mut e);
    *dropped = dropped.saturating_add(d);
}

/// 매니저가 없어도 호출자는 먼저 큐를 비운다. 수신자가 없는 실행에서 이벤트가 계속 쌓이지 않게 한다.
pub(crate) fn emit(mgr: Option<&mut PluginManager>, events: Vec<AgentEvent>, dropped: u64) {
    use tasty_plugin_protocol::EventScope;
    use tasty_plugin_protocol::events::payloads::{AgentBarrierClosed, AgentTaskFinished};

    if dropped > 0 {
        tracing::warn!(
            "agent event feed dropped {dropped} event(s) at its ceiling — the feed has a hole \
             and consumers cannot see it from the events they do receive"
        );
    }
    if events.is_empty() {
        return;
    }
    let Some(mgr) = mgr else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drain_without_a_manager_still_empties_the_queue() {
        let q = Arc::new(AgentEventQueue::new());
        q.push(AgentEvent::TaskFinished {
            workspace_id: 1,
            task_id: "t-1".to_string(),
            state: "succeeded",
        });
        q.push(AgentEvent::BarrierClosed {
            workspace_id: 1,
            name: "ready".to_string(),
            count_required: 2,
        });
        let mut events = Vec::new();
        let mut dropped = 0u64;
        take_from(&q, &mut events, &mut dropped);
        emit(None, events, dropped);
        let (left, dropped) = q.take_pending();
        assert!(left.is_empty(), "꺼낸 뒤에는 큐가 비어 있어야 한다");
        assert_eq!(dropped, 0);
    }
}
