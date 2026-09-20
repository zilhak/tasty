//! agent 도메인 사건의 발화 큐.
//!
//! 이 큐가 있는 이유는 **발화하는 쪽과 버스가 다른 스레드에 살기 때문**이다. task 의
//! 종결 전이는 러너 스레드에서도 일어나는데 Event Bus 는 `PluginManager` 가 소유해
//! 메인 스레드에서 fan-out 된다. 그래서 발화점은 사실만 여기 적고, 프레임 루프가
//! 그것을 꺼내 `emit_host_event` 로 내보낸다 — `memory.changed` 가 같은 모양이다
//! (`src/app/dispatch/memory_changes.rs`).
//!
//! **무엇을 싣지 않는가**: 실패 사유·task 결과·명령 출력. 근거는 payload 타입
//! (`tasty_plugin_protocol::events::payloads::AgentTaskFinished`) 의 주석에 있다.

use std::sync::Mutex;

/// 한 프레임 안에서 밀릴 수 있는 사건 수의 상한.
///
/// 이 값이 하는 일은 **성능 조절이 아니라 메모리 상한**이다. 정상 경로에서는 프레임마다
/// 비므로 길이가 한 자리를 넘지 않는다. 길어지는 경우는 하나뿐이다 — 꺼내 갈 프레임
/// 루프가 없는 실행(헤드리스 도구 실행 등). 그때 조용히 무한히 자라는 것보다 상한에서
/// 멈추고 **버린 수를 세는** 편이 낫다. 버린 사실은 `take_pending` 이 함께 돌려준다.
const MAX_PENDING: usize = 4096;

/// 꺼내 가기를 기다리는 agent 사건 하나.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// task 가 종결 상태에 들어갔다. **종결만 싣는다** — 비종결 전이에는 모든 진입
    /// 경로가 지나는 단일 깔때기가 없다.
    TaskFinished {
        workspace_id: u32,
        task_id: String,
        /// `TaskState::name()` 이 주는 이름. 종결 넷 중 하나다.
        state: &'static str,
    },
    /// barrier 가 요구 수를 채워 닫혔다.
    BarrierClosed {
        workspace_id: u32,
        name: String,
        count_required: u32,
    },
}

/// poison 복구 공용 보고 좌표(첫 1 회). 이 큐는 프로세스에 하나다.
static AGENT_FEED_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 발화점과 프레임 루프 사이의 손바꿈 자리.
#[derive(Default)]
pub struct AgentEventQueue {
    pending: Mutex<Vec<AgentEvent>>,
    /// 상한에 걸려 버린 수. `take_pending` 이 함께 돌려주고 0 으로 되돌린다.
    dropped: Mutex<u64>,
}

impl AgentEventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// 사건 하나를 기록한다.
    ///
    /// 임계구역이 `push` 하나뿐이라 패닉할 곳이 없고, 그래도 poison 이 왔다면 복구가
    /// 맞다 — 여기서 패닉하면 **task 를 종결시키던 경로가 함께 죽는다**
    /// (`docs/dev-guide/error-handling.md` "락 poison").
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

    /// 쌓인 사건을 통째로 가져간다. 두 번째 값은 **그 사이 상한에 걸려 버린 수**다 —
    /// 0 이 아니면 피드에 구멍이 있다는 뜻이고, 부르는 쪽이 그것을 로그로 남긴다.
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

    /// 상한을 넘겨도 **조용히 넘어가지 않는다.** 버린 수가 꺼낼 때 함께 나온다 —
    /// 그것이 없으면 꺼내 갈 루프가 없는 실행에서 피드가 구멍 난 것을 알 방법이 없다.
    #[test]
    fn overflowing_the_queue_reports_how_many_it_threw_away() {
        let q = AgentEventQueue::new();
        for i in 0..(MAX_PENDING + 3) {
            q.push(finished(&format!("t-{i}")));
        }
        let (out, dropped) = q.take_pending();
        assert_eq!(out.len(), MAX_PENDING);
        assert_eq!(dropped, 3);
        // 버린 수는 한 번만 보고된다.
        let (_, again) = q.take_pending();
        assert_eq!(again, 0);
    }
}
