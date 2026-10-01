//! engine의 작업 종료·barrier 이벤트를 메인 루프에 전달한다.
//! 러너도 기록할 수 있어 큐를 공유하고 PluginManager가 나중에 방송한다.
//! 실패 사유·작업 결과·명령 출력은 payload에 싣지 않는다.

use std::sync::Mutex;

/// 소비가 늦어져도 큐가 계속 커지지 않도록 새 항목을 버리고 개수를 기록한다.
const MAX_PENDING: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// 종료 상태만 알린다. 중간 상태의 모든 변경을 추적하는 피드는 아니다.
    TaskFinished {
        workspace_id: u32,
        task_id: String,
        state: &'static str,
    },
    BarrierClosed {
        workspace_id: u32,
        name: String,
        count_required: u32,
    },
}

static AGENT_FEED_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct AgentEventQueue {
    pending: Mutex<Vec<AgentEvent>>,
    /// take_pending이 회수하고 0으로 돌려놓는 손실 누계.
    dropped: Mutex<u64>,
}

impl AgentEventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// poison은 로그로 알리고 큐를 계속 사용해 이벤트 기록 실패가 작업 종료를 중단하지 않게 한다.
    pub fn push(&self, event: AgentEvent) {
        let mut g = tasty_utils::poison::recover_mutex(
            self.pending.lock(),
            "agent event feed",
            &AGENT_FEED_POISONED,
        );
        if g.len() >= MAX_PENDING {
            drop(g);
            let mut d = tasty_utils::poison::recover_mutex(
                self.dropped.lock(),
                "agent event feed",
                &AGENT_FEED_POISONED,
            );
            *d = d.saturating_add(1);
            return;
        }
        g.push(event);
    }

    /// 대기 항목과 손실 누계를 각각 회수한다. 락이 별개여서 두 값은 같은 순간의 스냅샷은 아니다.
    pub fn take_pending(&self) -> (Vec<AgentEvent>, u64) {
        let taken = {
            let mut g = tasty_utils::poison::recover_mutex(
                self.pending.lock(),
                "agent event feed",
                &AGENT_FEED_POISONED,
            );
            std::mem::take(&mut *g)
        };
        let dropped = {
            let mut d = tasty_utils::poison::recover_mutex(
                self.dropped.lock(),
                "agent event feed",
                &AGENT_FEED_POISONED,
            );
            std::mem::replace(&mut *d, 0)
        };
        (taken, dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(id: &str) -> AgentEvent {
        AgentEvent::TaskFinished {
            workspace_id: 1,
            task_id: id.to_string(),
            state: "succeeded",
        }
    }

    #[test]
    fn taking_the_queue_empties_it() {
        let q = AgentEventQueue::new();
        q.push(finished("t-1"));
        q.push(finished("t-2"));
        let (first, dropped) = q.take_pending();
        assert_eq!(first.len(), 2);
        assert_eq!(dropped, 0);
        let (second, _) = q.take_pending();
        assert!(second.is_empty(), "두 번째 꺼냄은 비어야 한다");
    }

    #[test]
    fn events_come_back_in_the_order_they_were_recorded() {
        let q = AgentEventQueue::new();
        q.push(finished("t-1"));
        q.push(AgentEvent::BarrierClosed {
            workspace_id: 1,
            name: "wave".into(),
            count_required: 2,
        });
        q.push(finished("t-2"));
        let (out, _) = q.take_pending();
        assert_eq!(out[0], finished("t-1"));
        assert!(matches!(out[1], AgentEvent::BarrierClosed { .. }));
        assert_eq!(out[2], finished("t-2"));
    }

    #[test]
    fn overflowing_the_queue_reports_how_many_it_threw_away() {
        let q = AgentEventQueue::new();
        for i in 0..(MAX_PENDING + 3) {
            q.push(finished(&format!("t-{i}")));
        }
        let (out, dropped) = q.take_pending();
        assert_eq!(out.len(), MAX_PENDING);
        assert_eq!(dropped, 3);
        let (_, again) = q.take_pending();
        assert_eq!(again, 0);
    }
}
