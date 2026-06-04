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

use tasty_agent::runner::{DispatchHandle, RunnerLoop};
use tasty_agent::{LeaseStore, SemaphoreStore, TaskId, TaskResult, TaskState, TaskStore};
use tasty_memory::{HOST_OWNER, ListOpts, MemoryValue, Scope};

use super::runner_host::{
    HANDLE_KEY_PREFIX, HostExecutor, RunnerContext, evict_run_result, handle_key, load_run_result,
};
use tasty_agent::runner::PollOutcome;

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

/// S3: 호스트 재시작 후 영속된 DispatchHandle 을 reload.
///
/// 각 영속 entry 에 대해:
/// - `task state == Running` 이 아닌 항목은 stale → 영속만 제거 (state 변경 X).
/// - `ShellProcess { pid }` : `process_alive::is_alive(pid)` 검사. alive → 복원,
///   dead → K.A-1 `run_result.<task_id>` 영속이 있으면 정확한 exit_code 로 Succeeded /
///   Failed 마감 (precise), 없으면 `Failed("host restart: pid {pid} died (exit_code unknown)")`.
/// - `ClaudeChild` / `BarrierPoll` : 그대로 복원 (insert only). 다음 정상 tick 에서 poll.
///   ClaudeChild 의 첫 poll 이 injector 미준비여도 K.A-2 grace (30s) 안에서는 Active 유지.
/// - `Immediate*` / `ImmediateFail` : 영속 대상 아니므로 도달 안 됨 (방어적 evict).
///
/// 반환: 복원할 (task_id, handle) 쌍 — RunnerLoop.running 에 insert.
fn reload_persistent_handles(
    ctx: &RunnerContext,
    workspace_id: u32,
) -> Vec<(TaskId, DispatchHandle)> {
    let now = now_ms();
    let scope = Scope::Workspace(workspace_id);
    let entries = ctx.with_memory(|mem| {
        let opts = ListOpts {
            prefix: Some(HANDLE_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        mem.list(&scope, &opts).unwrap_or_default()
    });
    let mut alive: Vec<(TaskId, DispatchHandle)> = Vec::new();
    let mut dead: Vec<(TaskId, String)> = Vec::new();
    let mut stale: Vec<TaskId> = Vec::new();
    // K.A-1: 직전 host 의 watcher 가 영속한 정확한 종료 결과 — exit_code 포함 마감.
    let mut precise: Vec<(TaskId, PollOutcome)> = Vec::new();

    for e in entries {
        let task_id = e
            .key
            .strip_prefix(HANDLE_KEY_PREFIX)
            .unwrap_or(&e.key)
            .to_string();
        let MemoryValue::Json(v) = e.value else {
            stale.push(task_id);
            continue;
        };
        let handle: DispatchHandle = match serde_json::from_value(v) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("reload handle {task_id} deserialize: {e}");
                stale.push(task_id);
                continue;
            }
        };

        // task state 가 Running 이 아니면 영속만 정리. R-1: Running 이 아닌데 handle 영속이
        // 남아 있으면 다음 tick 의 Ready 분기가 *재* dispatch 할 수 있다.
        let state_opt: Option<TaskState> = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store
                .get(workspace_id, &task_id)
                .ok()
                .flatten()
                .map(|t| t.state)
        });
        if !matches!(state_opt, Some(TaskState::Running)) {
            stale.push(task_id);
            continue;
        }

        match &handle {
            DispatchHandle::ShellProcess { pid } => {
                if tasty_agent::platform::process_alive::is_alive(*pid) {
                    alive.push((task_id, handle));
                } else if let Some(outcome) = load_run_result(ctx, workspace_id, &task_id) {
                    // K.A-1: 직전 host watcher 가 exit_code 까지 영속해 둠 → 정확 마감.
                    precise.push((task_id, outcome));
                } else {
                    dead.push((
                        task_id,
                        format!("host restart: pid {pid} died (exit_code unknown)"),
                    ));
                }
            }
            // Claude/Barrier: 그대로 복원 — 다음 정상 tick 에서 poll.
            // ClaudeChild 의 첫 poll 이 injector 미준비로 실패하면 task=Failed (R3 정책).
            DispatchHandle::ClaudeChild { .. } | DispatchHandle::BarrierPoll { .. } => {
                alive.push((task_id, handle));
            }
            // Immediate* / ImmediateFail 은 영속 대상 아님 — 도달 시 방어적 evict.
            DispatchHandle::ReduceImmediate(_)
            | DispatchHandle::CustomImmediate(_)
            | DispatchHandle::ImmediateFail(_) => {
                stale.push(task_id);
            }
        }
    }

    // stale: 영속만 제거 (task state 는 건드리지 않음).
    if !stale.is_empty() {
        ctx.with_memory(|mem| {
            for tid in &stale {
                let _ = mem.delete(HOST_OWNER, &scope, &handle_key(tid), None); // best-effort
            }
        });
    }

    // dead: task=Failed + evict.
    if !dead.is_empty() {
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            {
                let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                for (task_id, err) in &dead {
                    if let Err(e) = store.set_result(
                        workspace_id,
                        task_id,
                        TaskResult {
                            exit_code: None,
                            output: None,
                            error: Some(err.clone()),
                        },
                    ) {
                        tracing::warn!("reload mark failed set_result {task_id}: {e}");
                    }
                    if let Err(e) = store.set_state(
                        workspace_id,
                        task_id,
                        TaskState::Failed { error: err.clone() },
                        now,
                    ) {
                        tracing::warn!("reload mark failed set_state {task_id}: {e}");
                    }
                }
            }
            for (task_id, _) in &dead {
                let _ = mem.delete(HOST_OWNER, &scope, &handle_key(task_id), None); // best-effort evict — 실패 시 다음 reload 가 stale 처리
            }
        });
    }

    // K.A-1 precise: 영속된 exit_code 로 정확히 마감 (Succeeded / Failed 분류).
    if !precise.is_empty() {
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            {
                let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                for (task_id, outcome) in &precise {
                    let (result, next_state) = match outcome {
                        PollOutcome::Done(r) => (r.clone(), TaskState::Succeeded),
                        PollOutcome::Failed(err) => (
                            TaskResult {
                                exit_code: None,
                                output: None,
                                error: Some(err.clone()),
                            },
                            TaskState::Failed { error: err.clone() },
                        ),
                        PollOutcome::Active => continue, // 도달 안 함 — watcher 는 종결 시점만 영속.
                    };
                    if let Err(e) = store.set_result(workspace_id, task_id, result) {
                        tracing::warn!("reload precise set_result {task_id}: {e}");
                    }
                    if let Err(e) = store.set_state(workspace_id, task_id, next_state, now) {
                        tracing::warn!("reload precise set_state {task_id}: {e}");
                    }
                }
            }
            // handle + run_result 둘 다 evict (정확히 마감된 task 의 잔재 제거).
            // best-effort — 실패 시 다음 reload 가 stale 분기로 정리.
            for (task_id, _) in &precise {
                if let Err(e) = mem.delete(HOST_OWNER, &scope, &handle_key(task_id), None) {
                    tracing::warn!("reload precise evict handle {task_id}: {e}");
                }
            }
        });
        for (task_id, _) in &precise {
            evict_run_result(ctx, workspace_id, task_id);
        }
    }

    if !alive.is_empty() || !dead.is_empty() || !stale.is_empty() || !precise.is_empty() {
        tracing::info!(
            "agent runner ws{workspace_id}: reload handles — alive={}, dead={}, stale={}, precise={}",
            alive.len(),
            dead.len(),
            stale.len(),
            precise.len()
        );
    }
    alive
}

fn run_loop(ctx: RunnerContext, workspace_id: u32, stop_rx: mpsc::Receiver<()>) {
    purge_stale_semaphore_holders(&ctx, workspace_id);
    purge_stale_lease_holders(&ctx, workspace_id);
    let reloaded = reload_persistent_handles(&ctx, workspace_id);
    let executor = HostExecutor::new(ctx.clone());
    let mut runner = RunnerLoop::new(executor);
    for (task_id, handle) in reloaded {
        runner.running.insert(task_id, handle);
    }
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
                let (res, fire_target) = ctx_for_set.with_memory(|mem| {
                    let seq = ctx_for_set.agent_seq.clone();
                    let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                    match store.set_state(ws, id, st, n) {
                        Ok((task, _downstream)) => {
                            // S5: 종결 전이 시 hub fire 를 위한 snapshot 채취.
                            let fire = if task.state.is_terminal() {
                                Some((task.state.clone(), task.result.clone()))
                            } else {
                                None
                            };
                            (Ok(()), fire)
                        }
                        Err(e) => (Err(e), None),
                    }
                });
                if let Some((state, result)) = fire_target {
                    ctx_for_set.task_waker_hub.fire(
                        ws,
                        id,
                        crate::core::agent::task_waker::TerminalSnapshot { state, result },
                    );
                }
                res
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;
    use std::sync::atomic::AtomicU64;
    use tasty_agent::task::TaskCreateOpts;
    use tasty_agent::{OnFailure, TaskCommand};
    use tasty_memory::{MemoryStorage, MemoryStore, PutOpts};

    fn fresh_ctx() -> (tempfile::TempDir, RunnerContext) {
        let td = tempfile::tempdir().unwrap();
        let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
        let ctx = RunnerContext {
            memory: Arc::new(Mutex::new(mem)),
            agent_seq: Arc::new(AtomicU64::new(0)),
            host_ipc: Arc::new(OnceLock::new()),
            task_waker_hub: Arc::new(crate::core::agent::task_waker::TaskWakerHub::new()),
        };
        (td, ctx)
    }

    fn put_handle(ctx: &RunnerContext, ws: u32, task_id: &str, handle: &DispatchHandle) {
        ctx.with_memory(|mem| {
            let value = MemoryValue::Json(serde_json::to_value(handle).unwrap());
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(ws),
                &handle_key(task_id),
                &value,
                &PutOpts::default(),
            )
            .unwrap();
        });
    }

    /// J.A.S3: 현 프로세스 pid 로 ShellProcess handle 영속 + reload → 복원.
    #[test]
    fn reload_persistent_handles_restores_alive_shell_process() {
        let (_td, ctx) = fresh_ctx();
        // task 가 Running 상태여야 reload 대상이 됨.
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Run {
                        command: vec!["true".into()],
                        workspace_id: 1,
                        cwd: None,
                    },
                    depends_on: vec![],
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::Value::Null,
                    now_ms: 1000,
                })
                .unwrap();
            store.set_state(1, &t.id, TaskState::Running, 1100).unwrap();
            t.id
        });
        let my_pid = std::process::id();
        let handle = DispatchHandle::ShellProcess { pid: my_pid };
        put_handle(&ctx, 1, &task_id, &handle);

        let alive = reload_persistent_handles(&ctx, 1);
        assert_eq!(alive.len(), 1, "live pid should restore");
        assert_eq!(alive[0].0, task_id);
    }

    /// J.A.S3: dead pid → task=Failed("host restart: pid X died") + handle evict.
    #[test]
    fn reload_persistent_handles_marks_dead_pid_as_failed() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Run {
                        command: vec!["true".into()],
                        workspace_id: 1,
                        cwd: None,
                    },
                    depends_on: vec![],
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::Value::Null,
                    now_ms: 1000,
                })
                .unwrap();
            store.set_state(1, &t.id, TaskState::Running, 1100).unwrap();
            t.id
        });
        // 0xFFFF_FFFE 은 거의 확실히 살아있지 않은 pid.
        let dead_pid: u32 = 0xFFFF_FFFE;
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::ShellProcess { pid: dead_pid },
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(alive.is_empty(), "dead pid should not restore");

        // task state 확인.
        let final_state: TaskState = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap().state
        });
        match final_state {
            TaskState::Failed { error } => assert!(
                error.contains("host restart") && error.contains("died"),
                "unexpected error: {error}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }

        // handle 도 evict 됐는지.
        let still_there: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!still_there, "dead pid handle should be evicted");
    }

    /// J.A.S3: task state != Running 인 handle → stale 처리 (영속만 evict, state X).
    #[test]
    fn reload_persistent_handles_evicts_stale_when_task_not_running() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            // Ready state 유지 (Running 으로 전이 안 함).
            store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Run {
                        command: vec!["true".into()],
                        workspace_id: 1,
                        cwd: None,
                    },
                    depends_on: vec![],
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::Value::Null,
                    now_ms: 1000,
                })
                .unwrap()
                .id
        });
        let handle = DispatchHandle::ShellProcess {
            pid: std::process::id(),
        };
        put_handle(&ctx, 1, &task_id, &handle);

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(alive.is_empty(), "non-Running task handle is stale");

        let still_there: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!still_there, "stale handle should be evicted");

        // task state 는 그대로 (Ready) — 변경하면 안 됨.
        let state: TaskState = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap().state
        });
        assert!(matches!(state, TaskState::Ready), "got {state:?}");
    }
}
