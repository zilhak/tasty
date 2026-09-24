//! 작업 상태를 기다리는 호출자에게 snapshot을 보내고 종결 상태는 사건 큐에도 기록한다.
//! 상태를 바꾸는 호출자가 fire를 호출해야 두 소비자가 통지를 받는다.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::time::Duration;

use tasty_agent::{TaskId, TaskResult, TaskState};

#[derive(Debug, Clone)]
pub struct TerminalSnapshot {
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone)]
pub enum AwaitOutcome {
    Terminal(TerminalSnapshot),
    TimedOut,
}

type WaiterKey = (u32, TaskId);

static TASK_WAKER_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct TaskWakerHub {
    waiters: Mutex<HashMap<WaiterKey, Vec<SyncSender<TerminalSnapshot>>>>,
    /// engine과 같은 큐를 공유해야 메인 루프가 이 이벤트를 방송할 수 있다.
    feed: std::sync::Arc<crate::core::agent::event_feed::AgentEventQueue>,
}

impl TaskWakerHub {
    /// 독립 큐를 만들며 외부 소비자는 연결하지 않는다. 실제 engine은 with_feed를 사용한다.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_feed(
        feed: std::sync::Arc<crate::core::agent::event_feed::AgentEventQueue>,
    ) -> Self {
        Self {
            waiters: Mutex::new(HashMap::new()),
            feed,
        }
    }

    /// poison을 알리고 waiter 맵을 계속 사용해 상태 전이 경로에서 다시 패닉하지 않게 한다.
    fn lock_recovering(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<WaiterKey, Vec<SyncSender<TerminalSnapshot>>>> {
        crate::poison::recover_mutex(self.waiters.lock(), "task waker hub", &TASK_WAKER_POISONED)
    }

    /// 받은 current가 종결이면 즉시 반환하며 그 외에는 등록 후 기다린다. 현재 저장소 상태를 재조회하지 않는다.
    /// None·Some(0)은 무기한 대기다. 등록 전에 지나간 fire를 재생하지 않는다.
    pub fn await_terminal(
        &self,
        workspace_id: u32,
        task_id: &TaskId,
        timeout_ms: Option<u64>,
        current: TerminalSnapshot,
    ) -> AwaitOutcome {
        if current.state.is_terminal() {
            return AwaitOutcome::Terminal(current);
        }
        let rx = {
            let (tx, rx) = sync_channel::<TerminalSnapshot>(1);
            let mut g = self.lock_recovering();
            g.entry((workspace_id, task_id.clone()))
                .or_default()
                .push(tx);
            rx
        };
        let infinite = matches!(timeout_ms, None | Some(0));
        let result = if infinite {
            rx.recv().ok()
        } else {
            let dur = Duration::from_millis(timeout_ms.unwrap());
            match rx.recv_timeout(dur) {
                Ok(snap) => Some(snap),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => None,
            }
        };
        match result {
            Some(snap) => AwaitOutcome::Terminal(snap),
            None => {
                // timeout·채널 종료를 같은 결과로 반환한다. sender는 다음 fire가 맵을 제거할 때까지 남는다.
                AwaitOutcome::TimedOut
            }
        }
    }

    /// 대기자에게 snapshot을 보내고 매핑을 지운다. 종결 상태는 대기자가 없어도 사건 큐에 기록한다.
    pub fn fire(&self, workspace_id: u32, task_id: &TaskId, snapshot: TerminalSnapshot) {
        if snapshot.state.is_terminal() {
            self.feed
                .push(crate::core::agent::event_feed::AgentEvent::TaskFinished {
                    workspace_id,
                    task_id: task_id.clone(),
                    state: snapshot.state.name(),
                });
        }
        let mut g = self.lock_recovering();
        let Some(senders) = g.remove(&(workspace_id, task_id.clone())) else {
            return;
        };
        for tx in senders {
            let _ = tx.try_send(snapshot.clone()); // 대기 시간이 끝나 수신자가 없을 수 있어 실패는 무시한다.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;
    use tasty_latency_control::ControlProbe;

    fn snap(state: TaskState) -> TerminalSnapshot {
        TerminalSnapshot {
            state,
            result: None,
        }
    }

    /// 시간 초과 시 스케줄러 대조 측정을 진단에 함께 남긴다. 원인을 확정하는 검사는 아니다.
    #[test]
    fn await_returns_immediately_if_already_terminal() {
        const LIMIT: Duration = Duration::from_millis(50);
        let hub = TaskWakerHub::new();
        let mut control = ControlProbe::start("종료 상태의 await_terminal 즉시 반환");
        let start = Instant::now();
        let out = hub.await_terminal(
            1,
            &"t-1".to_string(),
            Some(5000),
            snap(TaskState::Succeeded),
        );
        let elapsed = start.elapsed();
        assert!(elapsed < LIMIT, "{}", control.verdict(elapsed, LIMIT));
        match out {
            AwaitOutcome::Terminal(s) => assert!(matches!(s.state, TaskState::Succeeded)),
            other => panic!("expected Terminal, got {other:?}"),
        }
    }

    #[test]
    fn await_wakes_on_fire() {
        let hub = Arc::new(TaskWakerHub::new());
        let hub_w = hub.clone();
        let handle = thread::spawn(move || {
            hub_w.await_terminal(1, &"t-1".to_string(), Some(5000), snap(TaskState::Running))
        });
        // 등록할 시간을 주지만 등록 완료를 동기화하는 방식은 아니다.
        thread::sleep(Duration::from_millis(50));
        hub.fire(1, &"t-1".to_string(), snap(TaskState::Succeeded));
        let out = handle.join().unwrap();
        match out {
            AwaitOutcome::Terminal(s) => assert!(matches!(s.state, TaskState::Succeeded)),
            other => panic!("expected Terminal, got {other:?}"),
        }
    }

    #[test]
    fn await_times_out_after_short_duration() {
        const CEILING: Duration = Duration::from_millis(500);
        let hub = TaskWakerHub::new();
        let mut control = ControlProbe::start("50 ms 요청의 실제 대기 시간");
        let start = Instant::now();
        let out = hub.await_terminal(1, &"t-1".to_string(), Some(50), snap(TaskState::Running));
        let elapsed = start.elapsed();
        assert!(matches!(out, AwaitOutcome::TimedOut));
        // 하한은 조기 반환을, 상한은 과도한 지연을 검사한다. 상한 실패에는 대조 측정도 표시한다.
        assert!(elapsed >= Duration::from_millis(40), "elapsed={elapsed:?}");
        assert!(elapsed < CEILING, "{}", control.verdict(elapsed, CEILING));
    }

    #[test]
    fn fire_without_waiters_is_noop() {
        let hub = TaskWakerHub::new();
        hub.fire(1, &"t-none".to_string(), snap(TaskState::Succeeded));
    }

    #[test]
    fn a_terminal_task_reaches_the_feed_even_with_nobody_waiting() {
        use crate::core::agent::event_feed::{AgentEvent, AgentEventQueue};
        let feed = Arc::new(AgentEventQueue::new());
        let hub = TaskWakerHub::with_feed(Arc::clone(&feed));
        hub.fire(7, &"t-9".to_string(), snap(TaskState::Cancelled));
        let (events, dropped) = feed.take_pending();
        assert_eq!(dropped, 0);
        assert_eq!(
            events,
            vec![AgentEvent::TaskFinished {
                workspace_id: 7,
                task_id: "t-9".to_string(),
                state: "cancelled",
            }]
        );
    }

    #[test]
    fn a_nonterminal_snapshot_leaves_the_feed_alone() {
        use crate::core::agent::event_feed::AgentEventQueue;
        let feed = Arc::new(AgentEventQueue::new());
        let hub = TaskWakerHub::with_feed(Arc::clone(&feed));
        hub.fire(7, &"t-9".to_string(), snap(TaskState::Running));
        let (events, _) = feed.take_pending();
        assert!(events.is_empty(), "비종결이 실렸다: {events:?}");
    }
}

#[cfg(test)]
mod poison_tests {
    use super::*;
    use std::sync::Arc;

    /// poison 뒤에도 등록한 대기자에게 snapshot이 전달되는지 확인한다.
    #[test]
    fn a_poisoned_hub_still_wakes_pending_waiters() {
        let hub = Arc::new(TaskWakerHub::new());
        let held = Arc::clone(&hub);
        let joined = std::thread::spawn(move || {
            let _guard = held.waiters.lock().expect("fresh mutex");
            panic!("a thread dies while holding the waker hub");
        })
        .join();
        assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
        assert!(hub.waiters.lock().is_err(), "poison 됐어야 한다");

        let waiting = Arc::clone(&hub);
        let awaiting = std::thread::spawn(move || {
            waiting.await_terminal(
                1,
                &"t1".to_string(),
                Some(5_000),
                TerminalSnapshot {
                    state: TaskState::Running,
                    result: None,
                },
            )
        });

        let snapshot = TerminalSnapshot {
            state: TaskState::Succeeded,
            result: None,
        };
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let registered = !hub.lock_recovering().is_empty();
            if registered {
                break;
            }
        }
        hub.fire(1, &"t1".to_string(), snapshot);

        let outcome = awaiting.join().expect("await thread must not panic");
        assert!(
            matches!(outcome, AwaitOutcome::Terminal(s) if s.state == TaskState::Succeeded),
            "poison 이후에도 종결이 전달돼야 한다"
        );
    }
}
