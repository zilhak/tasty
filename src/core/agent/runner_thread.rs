//! Workspace 별 runner thread 의 시작/중단/상태를 관리하는 registry.
//!
//! `agent.task_run` IPC handler 가 본 registry 를 호출. 한 workspace 에 동시
//! runner 는 1 개만 — 중복 start 는 no-op.
//!
//! runner thread 본문: `RunnerLoop::tick` 을 500ms 간격으로 호출. tick 안의
//! task_list / set_state / set_result 는 `RunnerContext::with_memory` 의 짧은
//! lock 안에서만 수행 (executor.dispatch / poll 의 plugin IPC 호출은 lock 바깥).
//!
//! R5 완화: tick 본문은 `std::panic::catch_unwind` 로 감싸 panic 시 stop_tx 채널
//! 닫고 thread 종료 — registry 의 status 는 자동으로 false 가 된다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tasty_agent::runner::RunnerLoop;
use tasty_agent::{TaskState, TaskStore};
use tasty_memory::HOST_OWNER;

use super::runner_host::{HostExecutor, RunnerContext};

const TICK_INTERVAL: Duration = Duration::from_millis(500);

struct RunnerControl {
    stop_tx: mpsc::Sender<()>,
    crashed: Arc<AtomicBool>,
    /// `Option` — `Drop` / `stop_workspace` 에서 `take()` 후 join.
    join: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct RunnerStatus {
    pub running: bool,
    pub crashed: bool,
    pub ready_count: u32,
    pub running_count: u32,
}

pub struct RunnerRegistry {
    threads: Mutex<HashMap<u32, RunnerControl>>,
}

impl RunnerRegistry {
    pub fn new() -> Self {
        Self {
            threads: Mutex::new(HashMap::new()),
        }
    }

    /// 이미 실행 중이면 false (idempotent — 중복 start 는 no-op).
    pub fn start(&self, ctx: RunnerContext, workspace_id: u32) -> bool {
        let mut threads = self.threads.lock().expect("RunnerRegistry poisoned");
        if let Some(ctrl) = threads.get(&workspace_id) {
            if !ctrl.crashed.load(Ordering::Relaxed) {
                return false;
            }
            // crashed 인 경우 정리 후 재시작 허용.
        }
        let (tx, rx) = mpsc::channel::<()>();
        let crashed = Arc::new(AtomicBool::new(false));
        let crashed_thread = crashed.clone();
        let ctx_thread = ctx.clone();
        let join = thread::Builder::new()
            .name(format!("agent-runner-ws{workspace_id}"))
            .spawn(move || {
                let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_loop(ctx_thread, workspace_id, rx);
                }));
                if panicked.is_err() {
                    crashed_thread.store(true, Ordering::Relaxed);
                    tracing::error!(
                        "agent runner thread for workspace {workspace_id} panicked — \
                         marked crashed. Restart via agent.task_run start."
                    );
                }
            })
            .expect("spawn agent-runner thread");
        threads.insert(
            workspace_id,
            RunnerControl {
                stop_tx: tx,
                crashed,
                join: Some(join),
            },
        );
        true
    }

    /// 정지 신호를 보내고 join. 이미 멈춰있으면 false.
    pub fn stop(&self, workspace_id: u32) -> bool {
        let mut threads = self.threads.lock().expect("RunnerRegistry poisoned");
        if let Some(mut ctrl) = threads.remove(&workspace_id) {
            let _ = ctrl.stop_tx.send(()); // thread 가 panic 후 종료 시 Err — 의도적 무시
            if let Some(j) = ctrl.join.take() {
                let _ = j.join(); // catch_unwind 가 panic 흡수 — Err 일 일 거의 없음
            }
            return true;
        }
        false
    }

    /// 현재 상태 + ready/running task 카운트. task list 는 호출자가 별도 제공
    /// (Core 측에 의존성 없음).
    pub fn status(&self, ctx: &RunnerContext, workspace_id: u32) -> RunnerStatus {
        let threads = self.threads.lock().expect("RunnerRegistry poisoned");
        let (running, crashed) = match threads.get(&workspace_id) {
            Some(ctrl) => (
                !ctrl.crashed.load(Ordering::Relaxed),
                ctrl.crashed.load(Ordering::Relaxed),
            ),
            None => (false, false),
        };
        let (ready_count, running_count) = count_ready_running(ctx, workspace_id);
        RunnerStatus {
            running,
            crashed,
            ready_count,
            running_count,
        }
    }
}

impl Default for RunnerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn count_ready_running(ctx: &RunnerContext, workspace_id: u32) -> (u32, u32) {
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        match store.list(workspace_id) {
            Ok(tasks) => {
                let r = tasks
                    .iter()
                    .filter(|t| matches!(t.state, TaskState::Ready))
                    .count() as u32;
                let g = tasks
                    .iter()
                    .filter(|t| matches!(t.state, TaskState::Running))
                    .count() as u32;
                (r, g)
            }
            Err(_) => (0, 0),
        }
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn run_loop(ctx: RunnerContext, workspace_id: u32, stop_rx: mpsc::Receiver<()>) {
    let executor = HostExecutor::new(ctx.clone());
    let mut runner = RunnerLoop::new(executor);
    loop {
        // 1. tick 본문.
        let snapshot = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.list(workspace_id).unwrap_or_default()
        });

        let now = now_ms();
        let ctx_for_set = ctx.clone();
        let ctx_for_res = ctx.clone();
        runner.tick(
            workspace_id,
            now,
            &snapshot,
            move |ws, id, st, n| {
                ctx_for_set.with_memory(|mem| {
                    let seq = ctx_for_set.agent_seq.clone();
                    let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                    store.set_state(ws, id, st, n).map(|_| ())
                })
            },
            move |ws, id, r| {
                ctx_for_res.with_memory(|mem| {
                    let seq = ctx_for_res.agent_seq.clone();
                    let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                    store.set_result(ws, id, r).map(|_| ())
                })
            },
        );

        // 2. sleep + stop check.
        match stop_rx.recv_timeout(TICK_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => continue,
        }
    }
}
