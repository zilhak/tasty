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

#[derive(Default)]
pub struct TaskWakerHub {
    waiters: Mutex<HashMap<WaiterKey, Vec<SyncSender<TerminalSnapshot>>>>,
}

impl TaskWakerHub {
    pub fn new() -> Self {
        Self::default()
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
            let mut g = self.waiters.lock().expect("task waker hub mutex");
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
        let mut g = self.waiters.lock().expect("task waker hub mutex");
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

    fn snap(state: TaskState) -> TerminalSnapshot {
        TerminalSnapshot {
            state,
            result: None,
        }
    }

    #[test]
    fn await_returns_immediately_if_already_terminal() {
        let hub = TaskWakerHub::new();
        let start = Instant::now();
        let out = hub.await_terminal(
            1,
            &"t-1".to_string(),
            Some(5000),
            snap(TaskState::Succeeded),
        );
        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_millis(50), "took {elapsed:?}");
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
