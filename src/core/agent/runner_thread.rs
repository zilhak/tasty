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
use tasty_agent::{LeaseStore, SemaphoreStore, TaskResult, TaskState, TaskStore};
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
        if let Some(ctrl) = threads.get(&workspace_id)
            && !ctrl.crashed.load(Ordering::Relaxed)
        {
            return false;
        }
        // crashed 인 경우 정리 후 재시작 허용.
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

/// S3 보강 ①: 호스트 재시작 후 *영속된* semaphore holder 정화.
///
/// in-memory `held_permits` 는 재시작 시 비어 있으므로 직전에 점유 중이던
/// permit 이 영구 leak 된다. workspace runner 시작 직전 1 회 수행:
///
/// 1. 해당 workspace 의 모든 Running task 를 load.
/// 2. `metadata.semaphore.holder == task.id` (컨벤션 강제) 인 항목만 정화 대상.
/// 3. 해당 holder 를 store 에서 release.
/// 4. task 자체를 `Failed("host restart")` 로 마감 — handle 유실 시나리오의
///    R3 정책과 일치 (사용자 retry).
fn purge_stale_semaphore_holders(ctx: &RunnerContext, workspace_id: u32) {
    let now = now_ms();
    let candidates: Vec<(String, String, String)> = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let Ok(tasks) = store.list(workspace_id) else {
            return Vec::new();
        };
        tasks
            .into_iter()
            .filter(|t| matches!(t.state, TaskState::Running))
            .filter_map(|t| {
                let meta = t.metadata.get("semaphore")?.as_object()?;
                let name = meta.get("name")?.as_str()?.to_string();
                let holder = meta
                    .get("holder")
                    .and_then(|v| v.as_str())
                    .unwrap_or(t.id.as_str())
                    .to_string();
                // task.id == holder 컨벤션을 강제: 외부 도구가 임의 holder 로
                // acquire 한 항목은 runner 회수 대상 아님.
                if holder != *t.id.as_str() {
                    return None;
                }
                Some((t.id, name, holder))
            })
            .collect()
    });
    if candidates.is_empty() {
        return;
    }
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        {
            let mut sem = SemaphoreStore::new(mem, HOST_OWNER);
            for (_task_id, name, holder) in &candidates {
                let _ = sem.release(workspace_id, name, holder); // idempotent
            }
        }
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        for (task_id, _, _) in &candidates {
            // 정화는 best-effort — task 가 이미 다른 상태로 갔거나 store 가 일시
            // 오류 상태면 다음 사용자 retry 가 처리한다.
            if let Err(e) = store.set_result(
                workspace_id,
                task_id,
                TaskResult {
                    exit_code: None,
                    output: None,
                    error: Some("host restart".to_string()),
                },
            ) {
                tracing::warn!("purge set_result for {task_id} failed: {e}");
            }
            if let Err(e) = store.set_state(
                workspace_id,
                task_id,
                TaskState::Failed {
                    error: "host restart".to_string(),
                },
                now,
            ) {
                tracing::warn!("purge set_state for {task_id} failed: {e}");
            }
        }
    });
    tracing::info!(
        "agent runner ws{workspace_id}: purged {} stale semaphore holder(s) on restart",
        candidates.len()
    );
}

/// S1 보강: 호스트 재시작 후 *영속된* lease holder 정화.
///
/// semaphore purge 와 같은 정책 — `metadata.lease.holder == task.id` 인 Running task 만
/// 정화 대상. 그 외 (외부 도구 또는 사용자 명시 holder) 는 그대로 보존.
fn purge_stale_lease_holders(ctx: &RunnerContext, workspace_id: u32) {
    let now = now_ms();
    let candidates: Vec<(String, String, String)> = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let Ok(tasks) = store.list(workspace_id) else {
            return Vec::new();
        };
        tasks
            .into_iter()
            .filter(|t| matches!(t.state, TaskState::Running))
            .filter_map(|t| {
                let meta = t.metadata.get("lease")?.as_object()?;
                let resource = meta.get("resource")?.as_str()?.to_string();
                let holder = meta
                    .get("holder")
                    .and_then(|v| v.as_str())
                    .unwrap_or(t.id.as_str())
                    .to_string();
                if holder != *t.id.as_str() {
                    return None;
                }
                Some((t.id, resource, holder))
            })
            .collect()
    });
    if candidates.is_empty() {
        return;
    }
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        {
            let mut lstore = LeaseStore::new(mem, HOST_OWNER);
            for (_task_id, resource, holder) in &candidates {
                let _ = lstore.release(workspace_id, resource, holder); // idempotent
            }
        }
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        for (task_id, _, _) in &candidates {
            if let Err(e) = store.set_result(
                workspace_id,
                task_id,
                TaskResult {
                    exit_code: None,
                    output: None,
                    error: Some("host restart".to_string()),
                },
            ) {
                tracing::warn!("purge(lease) set_result for {task_id} failed: {e}");
            }
            if let Err(e) = store.set_state(
                workspace_id,
                task_id,
                TaskState::Failed {
                    error: "host restart".to_string(),
                },
                now,
            ) {
                tracing::warn!("purge(lease) set_state for {task_id} failed: {e}");
            }
        }
    });
    tracing::info!(
        "agent runner ws{workspace_id}: purged {} stale lease holder(s) on restart",
        candidates.len()
    );
}

fn run_loop(ctx: RunnerContext, workspace_id: u32, stop_rx: mpsc::Receiver<()>) {
    purge_stale_semaphore_holders(&ctx, workspace_id);
    purge_stale_lease_holders(&ctx, workspace_id);
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
