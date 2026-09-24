//! Task 종결 전이의 단일 깔때기.
//!
//! 이 hub 가 답하는 사실은 하나다 — *이 task 가 종결 상태에 들어갔다.* 그 사실을
//! 받는 쪽이 **둘**이다:
//!
//! 1. `agent.task_await` 로 블로킹 중인 호출자 (원래 용도)
//! 2. agent 사건 피드 (`event_feed::AgentEventQueue`)
//!
//! **둘을 한 자리에 둔 것이 이 모듈의 요지다.** 종결 전이는 세 경로에서 일어나고
//! (`Core::task_set_state`/`task_cancel` wrapper · runner_thread 의 set_state 클로저 ·
//! hook wait timeout), 발화점을 따로 심으면 그 셋과 어긋날 수 있다. 여기서 같이
//! 내보내면 **피드의 덮는 범위가 곧 `task_await` 의 덮는 범위**가 되고, 그 불변식은
//! 이미 유지되고 있다(어긋나면 `task_await` 가 먼저 멈춘다 — 훨씬 시끄러운 실패다).
//!
//! 패턴: `tasty-approval` 의 `await_response` (sync_channel + waiters HashMap +
//! `recv_timeout`) 를 그대로 차용.
//!
//! - `await_terminal`: current state 가 이미 종결이면 즉시 반환. 아니면 channel
//!   등록 후 `recv_timeout`.
//! - `fire`: 같은 (workspace_id, task_id) 의 모든 sender 에 try_send.
//! - timeout 0 = 무한 (record-level timeout 없음 — approval 과 다른 정책).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::time::Duration;

use tasty_agent::{TaskId, TaskResult, TaskState};

/// 종결 시 wake 받는 스냅샷 — 호출자에게 돌려줄 최종 상태.
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

/// waiter 맵 락의 poison 복구 공용 보고 좌표(첫-1 회). hub 는 프로세스에 하나다.
static TASK_WAKER_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct TaskWakerHub {
    waiters: Mutex<HashMap<WaiterKey, Vec<SyncSender<TerminalSnapshot>>>>,
    /// 종결 사실을 함께 적을 피드. 부팅 경로가 `CoreState` 의 것과 같은 큐를 꽂아
    /// 준다. 안 꽂으면 자기 것을 들고 아무도 안 꺼내 가는데, 그 조합은 시험과
    /// 러너 하네스뿐이다(`with_feed` 를 안 부르는 생성자).
    feed: std::sync::Arc<crate::core::agent::event_feed::AgentEventQueue>,
}

impl TaskWakerHub {
    /// 피드 없이 만든다 — 종결 사실이 자기 큐에 쌓이고 아무도 안 꺼내 간다.
    /// 부팅 경로는 [`Self::with_feed`] 를 쓴다. 남은 호출자는 시험과 러너 하네스뿐이다.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// 종결 사실을 밖에서 꺼내 갈 수 있게 피드를 공유해 만든다.
    pub fn with_feed(
        feed: std::sync::Arc<crate::core::agent::event_feed::AgentEventQueue>,
    ) -> Self {
        Self {
            waiters: Mutex::new(HashMap::new()),
            feed,
        }
    }

    /// Poison 된 waiter 맵을 복구한다.
    ///
    /// 맵이 담는 것은 `SyncSender` 목록뿐이고 임계구역은 `entry`/`push`/`remove` 밖에
    /// 하지 않는다 — 패닉이 나도 맵은 유효하다. 반면 [`Self::fire`] 가 패닉하면 그
    /// task 를 기다리는 `agent.task_await` 호출자 전원이 **영원히** 깨어나지 못한다
    /// (timeout 을 안 준 호출자는 무한 대기다). 복구가 맞다
    /// ([`error-handling.md`](../../../docs/dev-guide/error-handling.md) "락 poison").
    ///
    /// 예전엔 이 자리가 매 발생을 로그로 남겼다(task 종결 단위라 빈도가 낮아 감당된다는
    /// 판단). 이제는 공용 헬퍼의 첫-1 회 보고로 통일한다 — poison 은 sticky 라 첫 만남이
    /// 곧 원인이고, 방침(error-handling.md)이 모든 poison 복구를 한 헬퍼·첫-1 회로 모은다.
    fn lock_recovering(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<WaiterKey, Vec<SyncSender<TerminalSnapshot>>>> {
        crate::poison::recover_mutex(self.waiters.lock(), "task waker hub", &TASK_WAKER_POISONED)
    }

    /// `current` 가 이미 종결이면 즉시 반환. 아니면 등록 후 timeout 동안 대기.
    /// `timeout_ms == None` 또는 `Some(0)` = 무한 대기.
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
                // Best-effort cleanup — timeout 시 자기 sender 를 hub 에서 제거.
                // sync_channel 의 sender 가 hash 비교 불가 → 같은 key 의 dead sender 는
                // 다음 fire 의 try_send 가 SendError 로 흡수 (받는쪽 drop).
                AwaitOutcome::TimedOut
            }
        }
    }

    /// `(workspace_id, task_id)` 의 모든 waiter 에 snapshot 전달 + map 에서 제거.
    /// 같은 사실을 사건 피드에도 적는다 — **대기자가 없어도 적는다.** 피드의 소비자는
    /// 그 task 를 기다리던 쪽이 아니라 나중에 붙는 쪽이다.
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
            let _ = tx.try_send(snapshot.clone()); // 의도적 무시 — 수신측이 drop 했으면 SendError 흡수
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

    /// 상한을 넘었을 때 **대조군**을 함께 실어 부하가 만든 값과 코드가 만든 값을 가른다
    /// (근거·선택 규칙은
    /// `docs/adr/0046-verification-evidence-and-diagnostics.md`).
    /// 이 자리가 기다리는 자원은 락과 CPU 뿐이라 스케줄러 계열 대조군이 맞다 — 디스크 뒤에
    /// 줄 서는 값이 아니다.
    #[test]
    fn await_returns_immediately_if_already_terminal() {
        const LIMIT: Duration = Duration::from_millis(50);
        let hub = TaskWakerHub::new();
        // 측정 **전에** 기준선을 잡는다. 이 탐침은 어느 경로로 빠져나가든 값을 남긴다.
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
        // 짧은 sleep 으로 waiter 등록 완료를 보장.
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
        let mut control = ControlProbe::start("50ms 대기의 상한");
        let start = Instant::now();
        let out = hub.await_terminal(1, &"t-1".to_string(), Some(50), snap(TaskState::Running));
        let elapsed = start.elapsed();
        assert!(matches!(out, AwaitOutcome::TimedOut));
        // 아래쪽(40ms)은 부하에 안 흔들린다 — 굶은 러너는 타이머를 **일찍** 깨우지
        // 못하므로 이 방향의 빨강은 언제나 코드다. 위쪽만 대조군을 싣는다.
        assert!(elapsed >= Duration::from_millis(40), "elapsed={elapsed:?}");
        assert!(elapsed < CEILING, "{}", control.verdict(elapsed, CEILING));
    }

    #[test]
    fn fire_without_waiters_is_noop() {
        let hub = TaskWakerHub::new();
        hub.fire(1, &"t-none".to_string(), snap(TaskState::Succeeded));
        // assertion: no panic
    }

    /// 대기자가 **없어도** 피드에는 적힌다. 피드의 소비자는 그 task 를 기다리던 쪽이
    /// 아니라 나중에 붙는 쪽이라, 대기자 유무로 갈리면 그 소비자는 자기가 무엇을
    /// 못 받았는지 알 수 없다.
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

    /// 비종결 상태로 hub 를 때려도 피드에는 안 적힌다. 이 키가 약속한 것은 **종결**
    /// 이고, 한 번이라도 비종결이 섞이면 소비자가 종결로 읽는다.
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

    /// waiter 맵이 poison 돼도 `fire` 는 대기자를 깨운다.
    ///
    /// `.expect()` 이던 시절에는 여기서 패닉해, 그 task 를 기다리던
    /// `agent.task_await` 호출자가 **영원히** 깨어나지 못했다 — timeout 없이 부른
    /// 호출자에게는 무한 대기다.
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

        // 등록이 보이도록 잠깐 양보한 뒤 fire.
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
