//! workspace별 러너 스레드를 시작·정지·조회한다. 중복 시작은 생략한다.
//! 각 tick의 저장소 접근은 락 안에서, IPC 실행·poll은 락 밖에서 수행한다.
//! run_loop의 패닉을 잡아 crashed로 표시하며 다음 start가 재시작할 수 있다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tasty_agent::runner::{DispatchHandle, RunnerLoop};
use tasty_agent::{LeaseStore, SemaphoreStore, TaskId, TaskResult, TaskState, TaskStore};
use tasty_memory::{HOST_OWNER, ListOpts, MemoryValue, Scope};

use super::runner_host::{
    HANDLE_KEY_PREFIX, HostExecutor, RunnerContext, evict_run_result, evict_task_side_keys,
    handle_key, load_run_result,
};
use tasty_agent::runner::PollOutcome;

const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// 조회가 반복 실패할 때 error로 올릴 횟수. tick 소요가 달라질 수 있어 시간 상한은 아니다.
const STORE_LIST_ERROR_AFTER: u32 = 6;
/// error 승격 뒤 반복 로그를 줄이는 실패 횟수 간격.
const STORE_LIST_REPEAT_EVERY: u32 = 120;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum StoreListFailureLog {
    Silent,
    Warn,
    Error,
}

fn store_list_failure_log(consecutive: u32) -> StoreListFailureLog {
    if consecutive == 1 {
        return StoreListFailureLog::Warn;
    }
    if consecutive < STORE_LIST_ERROR_AFTER {
        return StoreListFailureLog::Silent;
    }
    if consecutive == STORE_LIST_ERROR_AFTER
        || (consecutive - STORE_LIST_ERROR_AFTER).is_multiple_of(STORE_LIST_REPEAT_EVERY)
    {
        StoreListFailureLog::Error
    } else {
        StoreListFailureLog::Silent
    }
}

struct RunnerControl {
    stop_tx: mpsc::Sender<()>,
    crashed: Arc<AtomicBool>,
    /// 러너와 상태 조회가 공유하는 연속 실패 수. 조회가 성공하면 0으로 초기화한다.
    list_failures: Arc<AtomicU32>,
    join: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct RunnerStatus {
    pub running: bool,
    pub crashed: bool,
    /// 조회 실패는 task가 없는 0과 구별해 None으로 반환한다.
    pub ready_count: Option<u32>,
    pub running_count: Option<u32>,
    /// 카운트를 읽지 못한 사유. 성공하면 None이다.
    pub store_error: Option<String>,
    /// 러너가 없으면 0이며 실제 스레드의 연속 조회 실패 수를 표시한다.
    pub list_failures: u32,
}

pub struct RunnerRegistry {
    threads: Mutex<HashMap<u32, RunnerControl>>,
    poison_reported: AtomicBool,
}

impl RunnerRegistry {
    /// poison을 한 번 알리고 스레드 맵을 계속 사용한다. 시작·정지·liveness 조회가 사용한다.
    fn lock_recovering(&self) -> std::sync::MutexGuard<'_, HashMap<u32, RunnerControl>> {
        crate::poison::recover_mutex(
            self.threads.lock(),
            "runner registry thread map",
            &self.poison_reported,
        )
    }

    pub fn new() -> Self {
        Self {
            threads: Mutex::new(HashMap::new()),
            poison_reported: AtomicBool::new(false),
        }
    }

    /// 새로 시작했으면 true. 이미 실행 중이거나 spawn이 실패했으면 false다.
    pub fn start(&self, ctx: RunnerContext, workspace_id: u32) -> bool {
        let mut threads = self.lock_recovering();
        if let Some(ctrl) = threads.get(&workspace_id)
            && !ctrl.crashed.load(Ordering::Relaxed)
        {
            return false;
        }
        let (tx, rx) = mpsc::channel::<()>();
        let crashed = Arc::new(AtomicBool::new(false));
        let crashed_thread = crashed.clone();
        let list_failures = Arc::new(AtomicU32::new(0));
        let list_failures_thread = list_failures.clone();
        let ctx_thread = ctx.clone();
        let spawned = thread::Builder::new()
            .name(format!("agent-runner-ws{workspace_id}"))
            .spawn(move || {
                let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_loop(ctx_thread, workspace_id, rx, &list_failures_thread);
                }));
                if panicked.is_err() {
                    crashed_thread.store(true, Ordering::Relaxed);
                    tracing::error!(
                        "agent runner thread for workspace {workspace_id} panicked — \
                         marked crashed. Restart via agent.task_run start."
                    );
                }
            });
        let join = match spawned {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(
                    "failed to spawn agent-runner thread for workspace {workspace_id}: {e}"
                );
                return false;
            }
        };
        threads.insert(
            workspace_id,
            RunnerControl {
                stop_tx: tx,
                crashed,
                list_failures,
                join: Some(join),
            },
        );
        true
    }

    /// 등록된 스레드에 정지를 요청하고 join한다. 진행 중인 dispatch·poll이 끝날 때까지 기다릴 수 있다.
    pub fn stop(&self, workspace_id: u32) -> bool {
        let mut threads = self.lock_recovering();
        if let Some(mut ctrl) = threads.remove(&workspace_id) {
            let _ = ctrl.stop_tx.send(()); // 수신자가 끝났으면 정지 신호 실패는 무시한다.
            if let Some(j) = ctrl.join.take() {
                let _ = j.join(); // run_loop의 패닉은 스레드 안에서 기록하며 join 오류는 여기서 무시한다.
            }
            return true;
        }
        false
    }

    /// 등록 여부와 crashed 표지로 상태를 답하며 task 수는 세지 않는다.
    pub fn liveness(&self, workspace_id: u32) -> (bool, bool) {
        let threads = self.lock_recovering();
        match threads.get(&workspace_id) {
            Some(ctrl) => (
                !ctrl.crashed.load(Ordering::Relaxed),
                ctrl.crashed.load(Ordering::Relaxed),
            ),
            None => (false, false),
        }
    }

    pub fn status(&self, ctx: &RunnerContext, workspace_id: u32) -> RunnerStatus {
        let (running, crashed) = self.liveness(workspace_id);
        let list_failures = {
            let threads = self.threads.lock().expect("RunnerRegistry poisoned");
            threads
                .get(&workspace_id)
                .map_or(0, |c| c.list_failures.load(Ordering::Relaxed))
        };
        match count_ready_running(ctx, workspace_id) {
            Ok((ready_count, running_count)) => RunnerStatus {
                running,
                crashed,
                ready_count: Some(ready_count),
                running_count: Some(running_count),
                store_error: None,
                list_failures,
            },
            Err(e) => RunnerStatus {
                running,
                crashed,
                ready_count: None,
                running_count: None,
                store_error: Some(e),
                list_failures,
            },
        }
    }
}

impl Default for RunnerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 조회 실패를 task가 없는 것으로 표시하지 않도록 오류를 그대로 반환한다.
fn count_ready_running(ctx: &RunnerContext, workspace_id: u32) -> Result<(u32, u32), String> {
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let tasks = store.list(workspace_id).map_err(|e| e.to_string())?;
        let r = tasks
            .iter()
            .filter(|t| matches!(t.state, TaskState::Ready))
            .count() as u32;
        let g = tasks
            .iter()
            .filter(|t| matches!(t.state, TaskState::Running))
            .count() as u32;
        Ok((r, g))
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Running 작업 중 semaphore 이름이 있고 holder가 task ID인 항목을 정리한다.
/// holder를 생략하면 task ID로 본다. 점유 해제 후 작업을 host restart 실패로 표시하도록 시도한다.
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
                // 별도 holder를 지정한 외부 점유는 러너가 회수하지 않는다.
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
                let _ = sem.release(workspace_id, name, holder); // 해제 실패도 여기서는 무시하고 작업 상태 처리를 계속한다.
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
        "agent runner ws{workspace_id}: processed {} stale semaphore holder candidate(s) during startup cleanup",
        candidates.len()
    );
}

/// metadata.lease.resource가 있고 holder가 task ID인 Running 작업만 정리한다.
/// candidates만 지정한 pool은 여기서 실제 획득 자원을 복원하지 않는다.
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
                let _ = lstore.release(workspace_id, resource, holder); // 해제 실패도 여기서는 무시하고 작업 상태 처리를 계속한다.
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
        "agent runner ws{workspace_id}: processed {} stale lease holder candidate(s) during startup cleanup",
        candidates.len()
    );
}

/// 저장된 handle을 분류해 복원하거나 정리한다.
/// ShellProcess는 PID가 없을 때만 저장된 종료 결과를 확인한다. PID 생존은 같은 자식임을 보장하지 않는다.
/// PolledDispatch·BarrierPoll은 복원하며 즉시 완료 handle은 삭제 대상으로 본다.
/// AwaitExternal은 reload 당시 만료됐으면 실패 처리한다. 미만료 복원의 한계는 아래 분기를 참고한다.
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
    let mut precise: Vec<(TaskId, PollOutcome)> = Vec::new();

    for e in entries {
        match classify_persisted_handle(ctx, workspace_id, now, e) {
            HandleClassification::Alive(task_id, handle) => alive.push((task_id, handle)),
            HandleClassification::Dead(task_id, err) => dead.push((task_id, err)),
            HandleClassification::Stale(task_id) => stale.push(task_id),
            HandleClassification::Precise(task_id, outcome) => precise.push((task_id, outcome)),
        }
    }

    evict_stale_handles(ctx, &scope, &stale);
    mark_dead_tasks(ctx, workspace_id, &scope, now, &dead);
    finalize_precise_tasks(ctx, workspace_id, &scope, now, &precise);

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

enum HandleClassification {
    Alive(TaskId, DispatchHandle),
    Dead(TaskId, String),
    Stale(TaskId),
    Precise(TaskId, PollOutcome),
}

/// handle별 정리 계획을 만든다. 저장소 변경은 분류를 모은 뒤 별도 함수에서 수행한다.
fn classify_persisted_handle(
    ctx: &RunnerContext,
    workspace_id: u32,
    now: u64,
    e: tasty_memory::MemoryEntry,
) -> HandleClassification {
    let task_id = e
        .key
        .strip_prefix(HANDLE_KEY_PREFIX)
        .unwrap_or(&e.key)
        .to_string();
    let MemoryValue::Json(v) = e.value else {
        return HandleClassification::Stale(task_id);
    };
    let handle: DispatchHandle = match serde_json::from_value(v) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("reload handle {task_id} deserialize: {e}");
            return HandleClassification::Stale(task_id);
        }
    };

    // Running이 아닌 작업은 handle만 지운다. 현재 구현은 task 조회 오류도 None으로 보아 같은 분류를 한다.
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
        return HandleClassification::Stale(task_id);
    }

    match &handle {
        DispatchHandle::ShellProcess { pid } => {
            if tasty_agent::platform::process_alive::is_alive(*pid) {
                HandleClassification::Alive(task_id, handle)
            } else if let Some(outcome) = load_run_result(ctx, workspace_id, &task_id) {
                HandleClassification::Precise(task_id, outcome)
            } else {
                HandleClassification::Dead(
                    task_id,
                    format!("host restart: pid {pid} died (exit_code unknown)"),
                )
            }
        }
        // 비영속 hook 매핑은 사라졌으므로 저장된 기한으로 만료를 판단한다. 옛 누락 필드는 0이다.
        DispatchHandle::AwaitExternal { deadline_ms, .. } if *deadline_ms <= now => {
            HandleClassification::Dead(
                task_id,
                "host restart: push completion strategy deadline already expired".to_string(),
            )
        }
        // 미만료 AwaitExternal도 외부 상태 변경 뒤 점유를 정리할 수 있도록 복원한다.
        // 재시작한 hook_wait 매핑은 복원하지 않아 훅과 tick의 timeout sweep이 이 작업을 찾지 못한다.
        // poll도 항상 Active이므로 기한 만료만으로는 끝나지 않고 다음 reload에서 다시 검사한다.
        DispatchHandle::PolledDispatch { .. }
        | DispatchHandle::BarrierPoll { .. }
        | DispatchHandle::AwaitExternal { .. } => HandleClassification::Alive(task_id, handle),
        DispatchHandle::ReduceImmediate(_)
        | DispatchHandle::CustomImmediate(_)
        | DispatchHandle::ImmediateFail(_) => HandleClassification::Stale(task_id),
    }
}

/// stale handle만 삭제하고 task 상태는 바꾸지 않는다.
fn evict_stale_handles(ctx: &RunnerContext, scope: &Scope, stale: &[TaskId]) {
    if stale.is_empty() {
        return;
    }
    ctx.with_memory(|mem| {
        for tid in stale {
            let _ = mem.delete(HOST_OWNER, scope, &handle_key(tid), None); // 삭제 실패는 무시하며 다음 reload에서 다시 시도할 수 있다.
        }
    });
}

fn mark_dead_tasks(
    ctx: &RunnerContext,
    workspace_id: u32,
    scope: &Scope,
    now: u64,
    dead: &[(TaskId, String)],
) {
    if dead.is_empty() {
        return;
    }
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            for (task_id, err) in dead {
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
        for (task_id, _) in dead {
            let _ = mem.delete(HOST_OWNER, scope, &handle_key(task_id), None); // 삭제 실패는 무시하며 handle이 남으면 다음 reload가 다시 분류한다.
        }
    });
}

fn finalize_precise_tasks(
    ctx: &RunnerContext,
    workspace_id: u32,
    scope: &Scope,
    now: u64,
    precise: &[(TaskId, PollOutcome)],
) {
    if precise.is_empty() {
        return;
    }
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            for (task_id, outcome) in precise {
                apply_precise_outcome(&mut store, workspace_id, task_id, outcome, now);
            }
        }
        evict_precise_handles(mem, scope, precise);
    });
    for (task_id, _) in precise {
        evict_run_result(ctx, workspace_id, task_id);
    }
}

/// 저장된 종료 결과로 상태를 갱신한다. 각 저장 실패는 경고하며 Active 결과는 처리하지 않는다.
fn apply_precise_outcome(
    store: &mut TaskStore,
    workspace_id: u32,
    task_id: &TaskId,
    outcome: &PollOutcome,
    now: u64,
) {
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
        PollOutcome::Active => return,
    };
    if let Err(e) = store.set_result(workspace_id, task_id, result) {
        tracing::warn!("reload precise set_result {task_id}: {e}");
    }
    if let Err(e) = store.set_state(workspace_id, task_id, next_state, now) {
        tracing::warn!("reload precise set_state {task_id}: {e}");
    }
}

fn evict_precise_handles(
    mem: &mut dyn tasty_memory::MemoryStorage,
    scope: &Scope,
    precise: &[(TaskId, PollOutcome)],
) {
    for (task_id, _) in precise {
        if let Err(e) = mem.delete(HOST_OWNER, scope, &handle_key(task_id), None) {
            tracing::warn!("reload precise evict handle {task_id}: {e}");
        }
    }
}

/// 대기 매핑을 먼저 회수한 뒤 만료 작업을 실패로 표시하고 대기자를 깨운다.
/// 저장 실패는 경고하며 제거한 매핑을 재등록하지 않는다. 재시작으로 사라진 매핑은 대상이 아니다.
/// 공유 매핑의 제거가 락 안에서 이뤄져 여러 workspace 러너가 같은 항목을 중복 회수하지 않는다.
fn expire_overdue_hook_waits(ctx: &RunnerContext, now_ms: u64) {
    let overdue = ctx.hook_task_waits.sweep_expired(now_ms);
    for (workspace_id, task_id) in overdue {
        let error = "push completion strategy timed out waiting for external report".to_string();
        let fire_target = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let result = TaskResult {
                exit_code: None,
                output: None,
                error: Some(error.clone()),
            };
            if let Err(e) = store.set_result(workspace_id, &task_id, result) {
                tracing::warn!("hook wait timeout: set_result {task_id} failed: {e}");
                return None;
            }
            match store.set_state(
                workspace_id,
                &task_id,
                TaskState::Failed {
                    error: error.clone(),
                },
                now_ms,
            ) {
                Ok((task, _downstream)) => Some((task.state, task.result)),
                Err(e) => {
                    tracing::warn!("hook wait timeout: set_state {task_id} failed: {e}");
                    None
                }
            }
        });
        if let Some((state, result)) = fire_target {
            ctx.task_waker_hub.fire(
                workspace_id,
                &task_id,
                crate::core::agent::task_waker::TerminalSnapshot { state, result },
            );
        }
        tracing::warn!(
            "agent task {task_id} (ws {workspace_id}): push completion strategy timed out \
             waiting for external report; failure update attempted"
        );
    }
}

/// 부팅 또는 러너 시작 때 점유 정리와 handle 복원을 수행한다.
/// 부팅에서는 반환 handle을 버리고 수동 러너 시작 때 다시 읽는다. 저장 실패 항목은 남을 수 있다.
fn purge_and_reload_on_restart(
    ctx: &RunnerContext,
    workspace_id: u32,
) -> Vec<(TaskId, DispatchHandle)> {
    purge_stale_semaphore_holders(ctx, workspace_id);
    purge_stale_lease_holders(ctx, workspace_id);
    reload_persistent_handles(ctx, workspace_id)
}

/// 전달받은 live workspace를 정리하되 러너 스레드를 자동으로 시작하지 않는다.
pub(crate) fn purge_stale_agent_state_on_boot(ctx: &RunnerContext, workspace_ids: &[u32]) {
    for &workspace_id in workspace_ids {
        // 아직 러너가 없어 복원 결과는 사용하지 않는다. 수동 시작 때 다시 읽는다.
        let _ = purge_and_reload_on_restart(ctx, workspace_id);
        gc_stale_tasks(ctx, workspace_id);
    }
}

/// 사용자가 며칠 안에 결과를 확인한다는 가정으로 정한 잠정 보존 기간. 실사용 측정에 근거한 값은 아니다.
const AGENT_TASK_GC_MIN_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// memory TTL 대신 TaskStore의 참조·Running 보호 규칙을 거쳐 오래된 작업을 지운다.
/// 완료 상태만 고르면 방치된 Waiting 작업과 그 입력을 계속 남기므로 상태 필터는 두지 않는다.
fn gc_stale_tasks(ctx: &RunnerContext, workspace_id: u32) {
    let Some(plan) = gc_plan_sweep(ctx, workspace_id) else {
        return;
    };
    if plan.deleted.is_empty() {
        return;
    }
    if !gc_apply_sweep_plan(ctx, workspace_id, &plan) {
        return;
    }
    for id in &plan.deleted {
        evict_task_side_keys(ctx, workspace_id, id);
    }
    tracing::info!(
        "agent runner ws{workspace_id}: GC swept {} stale task(s), {} retained (still referenced)",
        plan.deleted.len(),
        plan.retained.len()
    );
}

fn gc_plan_sweep(
    ctx: &RunnerContext,
    workspace_id: u32,
) -> Option<tasty_agent::task::TaskSweepPlan> {
    let filter = tasty_agent::task::TaskPurgeFilter {
        states: None,
        older_than_ms: Some(AGENT_TASK_GC_MIN_AGE_MS),
        now_ms: now_ms(),
    };
    let seq = ctx.agent_seq.clone();
    let plan = ctx.with_memory(|mem| {
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        store.plan_sweep(workspace_id, &filter)
    });
    match plan {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!("agent task GC ws{workspace_id}: plan_sweep failed: {e}");
            None
        }
    }
}

fn gc_apply_sweep_plan(
    ctx: &RunnerContext,
    workspace_id: u32,
    plan: &tasty_agent::task::TaskSweepPlan,
) -> bool {
    let seq = ctx.agent_seq.clone();
    let res = ctx.with_memory(|mem| {
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        store.apply_sweep_plan(workspace_id, plan)
    });
    match res {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("agent task GC ws{workspace_id}: apply_sweep_plan failed: {e}");
            false
        }
    }
}

/// snapshot 조회 실패를 세고 로그를 남긴 뒤 빈 목록으로 tick을 호출한다.
/// 목록 기반 dispatch·poll·점유 해제는 건너뛰지만 앞서 실행하는 hook 만료 처리는 별개다.
fn tick_snapshot(
    ctx: &RunnerContext,
    workspace_id: u32,
    store_list_failures: &AtomicU32,
) -> Vec<tasty_agent::Task> {
    let listed = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        store.list(workspace_id)
    });
    let e = match listed {
        Ok(tasks) => {
            let prev = store_list_failures.swap(0, Ordering::Relaxed);
            if prev > 0 {
                tracing::info!(
                    "agent runner ws{workspace_id}: task store recovered after {prev} \
                     consecutive list failures"
                );
            }
            return tasks;
        }
        Err(e) => e,
    };
    let n = store_list_failures
        .load(Ordering::Relaxed)
        .saturating_add(1);
    store_list_failures.store(n, Ordering::Relaxed);
    log_store_list_failure(workspace_id, n, &e);
    Vec::new()
}

fn log_store_list_failure(workspace_id: u32, n: u32, e: &dyn std::fmt::Display) {
    match store_list_failure_log(n) {
        StoreListFailureLog::Warn => tracing::warn!(
            "agent runner ws{workspace_id}: task store list failed: {e} \
             — snapshot-based dispatch, polling, and permit release skipped"
        ),
        StoreListFailureLog::Error => tracing::error!(
            "agent runner ws{workspace_id}: task store list failed {n} times in a row: {e} \
             — snapshot-based dispatch, polling, and permit release remain blocked"
        ),
        StoreListFailureLog::Silent => {}
    }
}

fn run_loop(
    ctx: RunnerContext,
    workspace_id: u32,
    stop_rx: mpsc::Receiver<()>,
    list_failures: &AtomicU32,
) {
    let reloaded = purge_and_reload_on_restart(&ctx, workspace_id);
    let executor = HostExecutor::new(ctx.clone());
    let mut runner = RunnerLoop::new(executor);
    for (task_id, handle) in reloaded {
        runner.running.insert(task_id, handle);
    }
    loop {
        // 만료 작업의 종결 상태를 이번 snapshot에서도 보고 handle·점유를 정리할 수 있도록 먼저 처리한다.
        let now = now_ms();
        expire_overdue_hook_waits(&ctx, now);

        let snapshot = tick_snapshot(&ctx, workspace_id, list_failures);

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

        match stop_rx.recv_timeout(TICK_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::runner_host::run_result_key;
    use super::*;
    use std::sync::OnceLock;
    use std::sync::atomic::AtomicU64;
    use tasty_agent::task::TaskCreateOpts;
    use tasty_agent::{OnFailure, TaskCommand};
    use tasty_memory::{MemoryStore, PutOpts};

    #[test]
    fn store_list_failure_log_is_rate_limited() {
        assert_eq!(store_list_failure_log(1), StoreListFailureLog::Warn);
        for n in 2..STORE_LIST_ERROR_AFTER {
            assert_eq!(
                store_list_failure_log(n),
                StoreListFailureLog::Silent,
                "{n} 번째 실패는 첫 warn 과 error 임계 사이라 조용해야 한다"
            );
        }
        assert_eq!(
            store_list_failure_log(STORE_LIST_ERROR_AFTER),
            StoreListFailureLog::Error
        );
        for n in (STORE_LIST_ERROR_AFTER + 1)..(STORE_LIST_ERROR_AFTER + STORE_LIST_REPEAT_EVERY) {
            assert_eq!(
                store_list_failure_log(n),
                StoreListFailureLog::Silent,
                "{n}"
            );
        }
        assert_eq!(
            store_list_failure_log(STORE_LIST_ERROR_AFTER + STORE_LIST_REPEAT_EVERY),
            StoreListFailureLog::Error
        );
        assert_eq!(
            store_list_failure_log(STORE_LIST_ERROR_AFTER + 2 * STORE_LIST_REPEAT_EVERY),
            StoreListFailureLog::Error
        );
    }

    fn fresh_ctx() -> (tempfile::TempDir, RunnerContext) {
        let td = tempfile::tempdir().unwrap();
        let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
        let ctx = RunnerContext {
            memory: Arc::new(Mutex::new(mem)),
            agent_seq: Arc::new(AtomicU64::new(0)),
            host_ipc: Arc::new(OnceLock::new()),
            task_waker_hub: Arc::new(crate::core::agent::task_waker::TaskWakerHub::new()),
            hook_task_waits: Arc::new(crate::core::agent::hook_wait::HookTaskWaits::new()),
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

    /// 실제 저장소 값을 손상시켜 snapshot 조회 실패 누적과 회복 시 초기화를 확인한다.
    #[test]
    fn tick_snapshot_counts_consecutive_failures_and_resets_on_recovery() {
        let (_td, ctx) = fresh_ctx();
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
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
                .unwrap();
        });
        let failures = AtomicU32::new(0);

        assert_eq!(tick_snapshot(&ctx, 1, &failures).len(), 1);
        assert_eq!(failures.load(Ordering::Relaxed), 0);

        // 키 형식을 복제하지 않고 실제 저장된 task 키의 값만 손상시킨다.
        let corrupt_key = ctx.with_memory(|mem| {
            let entries = mem
                .list(&Scope::Workspace(1), &ListOpts::default())
                .expect("list");
            let key = entries
                .iter()
                .map(|e| e.key.clone())
                .find(|k| k.contains("task"))
                .expect("task key");
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(1),
                &key,
                &MemoryValue::Json(serde_json::json!({ "not": "a task" })),
                &PutOpts::default(),
            )
            .expect("put");
            key
        });

        for expected in 1..=3u32 {
            assert!(
                tick_snapshot(&ctx, 1, &failures).is_empty(),
                "조회 실패면 빈 snapshot"
            );
            assert_eq!(
                failures.load(Ordering::Relaxed),
                expected,
                "error 승격에 사용하는 연속 실패 수가 누적되지 않았다"
            );
        }

        ctx.with_memory(|mem| {
            mem.delete(HOST_OWNER, &Scope::Workspace(1), &corrupt_key, None)
                .expect("delete");
        });
        assert!(tick_snapshot(&ctx, 1, &failures).is_empty());
        assert_eq!(
            failures.load(Ordering::Relaxed),
            0,
            "회복하면 카운터가 리셋돼야 한다"
        );
    }

    #[test]
    fn status_reports_unknown_counts_when_the_task_store_is_unreadable() {
        let (_td, ctx) = fresh_ctx();
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
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
                .unwrap();
        });
        let registry = RunnerRegistry::new();
        let healthy = registry.status(&ctx, 1);
        assert_eq!(healthy.ready_count, Some(1));
        assert!(healthy.store_error.is_none());

        ctx.with_memory(|mem| {
            let entries = mem
                .list(&Scope::Workspace(1), &ListOpts::default())
                .expect("list");
            let key = entries
                .iter()
                .map(|e| e.key.clone())
                .find(|k| k.contains("task"))
                .expect("task key");
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(1),
                &key,
                &MemoryValue::Json(serde_json::json!({ "not": "a task" })),
                &PutOpts::default(),
            )
            .expect("put");
        });

        let broken = registry.status(&ctx, 1);
        assert_eq!(broken.ready_count, None, "못 읽었으면 0 이 아니라 unknown");
        assert_eq!(broken.running_count, None);
        assert!(
            broken.store_error.is_some(),
            "카운트가 없는 이유가 응답에 실려야 한다"
        );
    }

    /// 실제 러너 스레드가 올린 조회 실패 수를 status가 반환하는지 확인한다.
    #[test]
    fn status_reports_the_running_runner_consecutive_list_failures() {
        let (_td, ctx) = fresh_ctx();
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
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
                .unwrap();
        });
        // 실제 task 키의 값을 역직렬화할 수 없는 데이터로 바꾼다.
        ctx.with_memory(|mem| {
            let entries = mem
                .list(&Scope::Workspace(1), &ListOpts::default())
                .expect("list");
            let key = entries
                .iter()
                .map(|e| e.key.clone())
                .find(|k| k.contains("task"))
                .expect("task key");
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(1),
                &key,
                &MemoryValue::Json(serde_json::json!({ "not": "a task" })),
                &PutOpts::default(),
            )
            .expect("put");
        });

        let registry = RunnerRegistry::new();
        assert!(registry.start(ctx.clone(), 1), "러너가 새로 떠야 한다");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut observed = 0;
        while std::time::Instant::now() < deadline {
            let st = registry.status(&ctx, 1);
            if st.list_failures > 0 {
                observed = st.list_failures;
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        let final_status = registry.status(&ctx, 1);
        registry.stop(1);

        assert!(
            observed > 0,
            "러너가 store 를 못 읽고 있으면 status 가 그 횟수를 드러내야 한다 \
             (running={}, store_error={:?})",
            final_status.running,
            final_status.store_error
        );
        assert!(
            final_status.running,
            "조회 실패와 별개로 러너 스레드는 실행 중이어야 한다"
        );
    }

    #[test]
    fn reload_persistent_handles_restores_alive_shell_process() {
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
        let my_pid = std::process::id();
        let handle = DispatchHandle::ShellProcess { pid: my_pid };
        put_handle(&ctx, 1, &task_id, &handle);

        let alive = reload_persistent_handles(&ctx, 1);
        assert_eq!(alive.len(), 1, "live pid should restore");
        assert_eq!(alive[0].0, task_id);
    }

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
        // 보통 존재하지 않는 큰 PID를 쓴다. 환경에 따라 사용 중일 가능성은 있다.
        let dead_pid: u32 = 0xFFFF_FFFE;
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::ShellProcess { pid: dead_pid },
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(alive.is_empty(), "dead pid should not restore");

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

        let still_there: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!still_there, "dead pid handle should be evicted");
    }

    fn put_run_result(ctx: &RunnerContext, ws: u32, task_id: &str, value: serde_json::Value) {
        ctx.with_memory(|mem| {
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(ws),
                &run_result_key(task_id),
                &MemoryValue::Json(value),
                &PutOpts::default(),
            )
            .unwrap();
        });
    }

    fn make_running_run_task(ctx: &RunnerContext, ws: u32) -> TaskId {
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: ws,
                    name: "t".into(),
                    command: TaskCommand::Run {
                        command: vec!["true".into()],
                        workspace_id: ws,
                        cwd: None,
                    },
                    depends_on: vec![],
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::Value::Null,
                    now_ms: 1000,
                })
                .unwrap();
            store
                .set_state(ws, &t.id, TaskState::Running, 1100)
                .unwrap();
            t.id
        })
    }

    #[test]
    fn reload_shell_process_with_persisted_done_result_succeeds() {
        let (_td, ctx) = fresh_ctx();
        let task_id = make_running_run_task(&ctx, 1);
        let dead_pid: u32 = 0xFFFF_FFFE;
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::ShellProcess { pid: dead_pid },
        );
        put_run_result(
            &ctx,
            1,
            &task_id,
            serde_json::json!({
                "kind": "done",
                "exit_code": 0,
                "output": { "pid": dead_pid },
                "error": null,
            }),
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(alive.is_empty(), "precise should not restore to alive");

        let task = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap()
        });
        assert!(
            matches!(task.state, TaskState::Succeeded),
            "got {:?}",
            task.state
        );
        let result = task.result.expect("result present");
        assert_eq!(result.exit_code, Some(0));

        let handle_present: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!handle_present, "handle evicted after precise");
        let result_present: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &run_result_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!result_present, "run_result evicted after precise");
    }

    #[test]
    fn reload_shell_process_with_persisted_failed_result_marks_failed() {
        let (_td, ctx) = fresh_ctx();
        let task_id = make_running_run_task(&ctx, 1);
        let dead_pid: u32 = 0xFFFF_FFFE;
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::ShellProcess { pid: dead_pid },
        );
        put_run_result(
            &ctx,
            1,
            &task_id,
            serde_json::json!({
                "kind": "failed",
                "error": "Run exited non-zero: code=Some(2)",
            }),
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(alive.is_empty());

        let task = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap()
        });
        match &task.state {
            TaskState::Failed { error } => assert!(error.contains("non-zero"), "got {error}"),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(
            !matches!(&task.state, TaskState::Failed { error } if error.contains("unknown")),
            "should not be 'unknown' message",
        );
    }

    #[test]
    fn reload_shell_process_dead_pid_without_run_result_falls_back_to_unknown() {
        let (_td, ctx) = fresh_ctx();
        let task_id = make_running_run_task(&ctx, 1);
        let dead_pid: u32 = 0xFFFF_FFFE;
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::ShellProcess { pid: dead_pid },
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(alive.is_empty());

        let task = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap()
        });
        match &task.state {
            TaskState::Failed { error } => {
                assert!(error.contains("exit_code unknown"), "got {error}")
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn reload_persistent_handles_evicts_stale_when_task_not_running() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
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

        let state: TaskState = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap().state
        });
        assert!(matches!(state, TaskState::Ready), "got {state:?}");
    }

    /// reload 순간 이미 지난 AwaitExternal 기한은 실패로 처리한다.
    #[test]
    fn reload_persistent_handles_fails_await_external_past_deadline() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Custom {
                        ipc_method: "acme.start".into(),
                        params: serde_json::json!({}),
                        poll: None,
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
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::AwaitExternal {
                wait_key: "hook-1".into(),
                deadline_ms: 1,
            },
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert!(
            alive.is_empty(),
            "past-deadline AwaitExternal must not restore alive"
        );

        let task = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap()
        });
        match task.state {
            TaskState::Failed { error } => assert!(
                error.contains("deadline already expired"),
                "unexpected error: {error}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }

        let still_there: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!still_there, "expired handle should be evicted");
    }

    /// reload 순간 미만료인 AwaitExternal은 복원한다. 이후 시간 경과만으로 끝나는지는 이 시험에서 확인하지 않는다.
    #[test]
    fn reload_persistent_handles_restores_await_external_before_deadline() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Custom {
                        ipc_method: "acme.start".into(),
                        params: serde_json::json!({}),
                        poll: None,
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
        put_handle(
            &ctx,
            1,
            &task_id,
            &DispatchHandle::AwaitExternal {
                wait_key: "hook-2".into(),
                deadline_ms: u64::MAX,
            },
        );

        let alive = reload_persistent_handles(&ctx, 1);
        assert_eq!(
            alive.len(),
            1,
            "not-yet-expired AwaitExternal should restore"
        );
        assert_eq!(alive[0].0, task_id);

        let state: TaskState = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap().state
        });
        assert!(matches!(state, TaskState::Running), "got {state:?}");
    }

    #[test]
    fn expire_overdue_hook_waits_fails_the_task_and_removes_the_entry() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Custom {
                        ipc_method: "acme.start".into(),
                        params: serde_json::json!({}),
                        poll: None,
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

        ctx.hook_task_waits.register(1, 1, task_id.clone(), 2000);
        expire_overdue_hook_waits(&ctx, 5000);

        assert_eq!(ctx.hook_task_waits.resolve(1), None);

        let task = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap()
        });
        assert!(
            matches!(task.state, TaskState::Failed { .. }),
            "got {:?}",
            task.state
        );
        assert!(task.result.is_some());
    }

    #[test]
    fn expire_overdue_hook_waits_leaves_fresh_entries_alone() {
        let (_td, ctx) = fresh_ctx();
        let task_id = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let t = store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: "t".into(),
                    command: TaskCommand::Custom {
                        ipc_method: "acme.start".into(),
                        params: serde_json::json!({}),
                        poll: None,
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

        ctx.hook_task_waits.register(2, 1, task_id.clone(), 9999);
        expire_overdue_hook_waits(&ctx, 5000);

        assert_eq!(ctx.hook_task_waits.resolve(2), Some((1, task_id.clone())));

        let task = ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &task_id).unwrap().unwrap()
        });
        assert!(matches!(task.state, TaskState::Running));
    }
}

#[cfg(test)]
mod poison_tests {
    use super::*;

    /// poison 뒤 liveness·stop은 복구하며 경고 표지가 기록되는지 확인한다. status의 별도 lock은 검사하지 않는다.
    #[test]
    fn a_poisoned_thread_map_still_answers_liveness_and_stop() {
        let registry = Arc::new(RunnerRegistry::new());

        let held = Arc::clone(&registry);
        let joined = thread::spawn(move || {
            let _guard = held.threads.lock().expect("fresh mutex");
            panic!("a thread dies while holding the runner registry");
        })
        .join();
        assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
        assert!(registry.threads.lock().is_err(), "poison 됐어야 한다");

        assert_eq!(registry.liveness(1), (false, false));
        assert!(!registry.stop(1));
        assert!(
            registry.poison_reported.load(Ordering::Relaxed),
            "poison 은 보고돼야 한다"
        );
    }
}
