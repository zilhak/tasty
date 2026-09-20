//! agent 사건 큐를 버스로 옮기는 **조합 공용** 부분.
//!
//! 큐(`core::agent::event_feed`)는 발화 자리가 러너 스레드일 수 있어 사실만 적고,
//! 실제 fan-out 은 `PluginManager` 를 든 루프가 한다. 그 루프가 조합마다 다르다 —
//! gui 는 `about_to_wait`, 헤드리스는 데몬 루프다. **꺼내서 내보내는 규칙은 하나여야
//! 하므로** 여기 모은다.
//!
//! 두 벌로 두면 한쪽만 고쳐지고, 그 갈림은 컴파일로 안 드러난다 — 실제로 이
//! 모듈이 생기기 전에는 드레인이 gui 에만 있어서 헤드리스 데몬이 `events.fetch` 에
//! 정상 응답하면서 **영원히 빈 링**을 내줬다(읽는 쪽은 두 조합 모두에 배선돼 있다).

use std::sync::Arc;

use crate::core::agent::event_feed::{AgentEvent, AgentEventQueue};
use crate::plugin::PluginManager;

/// 큐 하나를 비워 넘긴 자리에 붙인다. 엔진 사본이 여럿인 조합(gui)은 이것을
/// 사본마다 부른다.
pub(crate) fn take_from(q: &Arc<AgentEventQueue>, events: &mut Vec<AgentEvent>, dropped: &mut u64) {
    let (mut e, d) = q.take_pending();
    events.append(&mut e);
    *dropped = dropped.saturating_add(d);
}

/// 꺼낸 것을 `agent.*` host event 로 broadcast 한다.
///
/// **manager 가 없어도 호출자는 먼저 꺼낸다.** 큐를 두고 돌아가면 다음 바퀴에 그만큼
/// 더 쌓이고, manager 가 끝내 안 생기는 실행에서는 상한까지 자란다. 꺼낸 뒤 버리는
/// 쪽이 맞다 — 받을 구독자가 없다는 뜻이므로.
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

    /// manager 가 없어도 **먼저 꺼낸다.** 큐를 두고 돌아가면 그 실행에서는 상한까지
    /// 자라고, 상한에 닿는 순간부터는 새 사건이 버려진다 — 받을 구독자가 없는 것과
    /// 피드에 구멍이 나는 것은 다른 일이다.
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
