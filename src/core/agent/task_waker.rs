//! Task await waker hub — `agent.task_await` 의 진짜 blocking 지원.
//!
//! 패턴: `tasty-approval` 의 `await_response` (sync_channel + waiters HashMap +
//! `recv_timeout`) 를 그대로 차용. fire 측은 `Core::task_set_state` wrapper +
//! runner_thread 의 set_state 클로저에서 호출 (R-5 회피).
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
}

impl TaskWakerHub {
    pub fn new() -> Self {
        Self::default()
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
    pub fn fire(&self, workspace_id: u32, task_id: &TaskId, snapshot: TerminalSnapshot) {
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
    /// `docs/adr/0181-a-latency-assertion-must-carry-a-control-that-load-moves-and-code-does-not.md`).
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
        let hub = TaskWakerHub::new();
        let start = Instant::now();
        let out = hub.await_terminal(1, &"t-1".to_string(), Some(50), snap(TaskState::Running));
        let elapsed = start.elapsed();
        assert!(matches!(out, AwaitOutcome::TimedOut));
        // 50ms ± 100ms 허용 (CI jitter).
        assert!(
            elapsed >= Duration::from_millis(40) && elapsed < Duration::from_millis(500),
            "elapsed={elapsed:?}"
        );
    }

    #[test]
    fn fire_without_waiters_is_noop() {
        let hub = TaskWakerHub::new();
        hub.fire(1, &"t-none".to_string(), snap(TaskState::Succeeded));
        // assertion: no panic
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
