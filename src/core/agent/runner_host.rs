//! Host-side `TaskExecutor` 구현 — runner thread 가 사용.
//!
//! - `ClaudeSpawn` → `claude.spawn` IPC 동기 호출 + `claude.wait` 로 poll.
//! - `Reduce` → 즉시 collect + `reduce_with_custom`.
//! - `Run` / `Custom` → F.6 에서 채움.

use std::collections::HashMap;
use std::process::Child;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::json;
use tasty_agent::runner::{DispatchHandle, DispatchOutcome, PollOutcome, TaskExecutor};
use tasty_agent::{
    BarrierState, BarrierStore, ReducerInput, SemaphoreStore, Task, TaskCommand, TaskId,
    TaskResult, reduce_with_custom,
};
use tasty_memory::{HOST_OWNER, MemoryStorage};

use crate::adapters::ipc::handler::agent::task::run_custom_shell;
use crate::ipc::host_call::HostIpcInjector;

/// Host→plugin dispatch timeout — claude.spawn 은 자식 프로세스 생성/디스크 I/O
/// 까지 포함하므로 비교적 여유. claude.wait 는 1tick 만이라 짧아도 되지만
/// 같은 값으로 통일.
const HOST_DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

/// runner thread 에 주입되는 컨텍스트 — Core 의 일부만 추려서 thread 로 옮긴다.
#[derive(Clone)]
pub struct RunnerContext {
    pub memory: Arc<Mutex<dyn MemoryStorage>>,
    pub agent_seq: Arc<AtomicU64>,
    pub host_ipc: Arc<OnceLock<HostIpcInjector>>,
}

impl RunnerContext {
    pub fn with_memory<R>(&self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        let mut guard = match self.memory.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut *guard)
    }

    /// host→plugin sync dispatch. injector 미초기화 시 Err.
    pub fn dispatch_plugin(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let inj = self
            .host_ipc
            .get()
            .ok_or_else(|| "host IPC injector not initialized".to_string())?;
        inj.dispatch(method, params, HOST_DISPATCH_TIMEOUT)
    }
}

pub struct HostExecutor {
    ctx: RunnerContext,
    /// Run task 의 child process 보관 — pid → Child. DispatchHandle 은 Clone
    /// 필요 (RunnerLoop 가 핸들을 복제), Child 는 Clone 불가이므로 분리.
    shell_children: HashMap<u32, Child>,
    /// 본 executor 가 dispatch 시 점유한 semaphore permit — (workspace_id, name, holder).
    /// in-memory only. 호스트 재시작 시 비어 있게 되므로 [`crate::core::agent::runner_thread`]
    /// 의 시작 시 정화 단계가 영속 holder 를 회수.
    held_permits: HashMap<TaskId, (u32, String, String)>,
}

impl HostExecutor {
    pub fn new(ctx: RunnerContext) -> Self {
        Self {
            ctx,
            shell_children: HashMap::new(),
            held_permits: HashMap::new(),
        }
    }

    /// `task.metadata.semaphore` 컨벤션을 읽어 permit 점유 시도.
    /// 반환: `Ok(None)` = 미사용 (semaphore metadata 없음), `Ok(Some(true))` = 점유 성공,
    /// `Ok(Some(false))` = 부족, `Err(msg)` = store 오류 또는 invalid metadata.
    fn try_acquire_semaphore(&mut self, task: &Task) -> Result<Option<bool>, String> {
        let Some(meta) = task.metadata.get("semaphore").and_then(|v| v.as_object()) else {
            return Ok(None);
        };
        let name = meta
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "semaphore metadata: missing 'name'".to_string())?;
        let holder = meta
            .get("holder")
            .and_then(|v| v.as_str())
            .unwrap_or(task.id.as_str());
        let name = name.to_string();
        let holder = holder.to_string();
        let ws = task.workspace_id;
        let result: Result<bool, String> = self.ctx.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store
                .acquire(ws, &name, &holder)
                .map(|o| o.acquired)
                .map_err(|e| e.to_string())
        });
        let acquired = result?;
        if acquired {
            self.held_permits
                .insert(task.id.clone(), (ws, name, holder));
        }
        Ok(Some(acquired))
    }
}

impl TaskExecutor for HostExecutor {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        // Phase I.A.3: semaphore-gated dispatch. metadata.semaphore 가 있으면 acquire
        // 시도. 점유 실패 시 Deferred — state 전이 없이 다음 tick 재시도.
        match self.try_acquire_semaphore(task) {
            Ok(None) => {}
            Ok(Some(true)) => {}
            Ok(Some(false)) => return DispatchOutcome::Deferred,
            Err(e) => return DispatchOutcome::PermanentFail(format!("semaphore: {e}")),
        }
        let result = match self.dispatch_command(task) {
            Ok(h) => DispatchOutcome::Started(h),
            Err(e) => DispatchOutcome::PermanentFail(e),
        };
        // dispatch 가 실패하면 막 점유한 permit 을 즉시 반환 (release 는 idempotent).
        if matches!(result, DispatchOutcome::PermanentFail(_)) {
            self.release_permit(&task.id);
        }
        result
    }

    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        self.poll_handle(handle)
    }

    fn release_permit(&mut self, task_id: &TaskId) {
        let Some((ws, name, holder)) = self.held_permits.remove(task_id) else {
            return;
        };
        let res: Result<(), String> = self.ctx.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store
                .release(ws, &name, &holder)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        if let Err(e) = res {
            tracing::warn!("semaphore release failed for task {task_id} ({name}/{holder}): {e}");
        }
    }
}

impl HostExecutor {
    /// 내부 dispatch 본체. `?` 로 `String` 에러 흡수 후 호출자가 `DispatchOutcome` 변환.
    fn dispatch_command(&mut self, task: &Task) -> Result<DispatchHandle, String> {
        match &task.command {
            TaskCommand::ClaudeSpawn {
                prompt,
                role,
                nickname,
                cwd,
                parent_surface,
                direction,
            } => {
                let parent_sid = parent_surface
                    .ok_or_else(|| "ClaudeSpawn requires parent_surface".to_string())?;
                let mut params = json!({
                    "surface_id": parent_sid,
                    "workspace": task.workspace_id.to_string(),
                    "prompt": prompt,
                });
                if let Some(r) = role {
                    params["role"] = json!(r);
                }
                if let Some(n) = nickname {
                    params["nickname"] = json!(n);
                }
                if let Some(c) = cwd {
                    params["cwd"] = json!(c.to_string_lossy());
                }
                if let Some(d) = direction {
                    params["direction"] = json!(d);
                }
                let resp = self
                    .ctx
                    .dispatch_plugin(
                        &format!("{}.spawn", "claude"), // claude.spawn — plugin namespace
                        params,
                    )
                    .map_err(|e| format!("claude.spawn: {e}"))?;
                let child_index = resp["child_index"]
                    .as_u64()
                    .ok_or_else(|| "claude.spawn response missing child_index".to_string())?
                    as u32;
                Ok(DispatchHandle::ClaudeChild {
                    parent_sid,
                    child_index,
                    workspace_id: task.workspace_id,
                })
            }
            TaskCommand::Reduce { inputs, strategy } => {
                // 1단계: 입력 task 결과 수집 (memory lock).
                let collected: Result<Vec<ReducerInput>, String> = self.ctx.with_memory(|mem| {
                    use tasty_agent::{TaskState, TaskStore};
                    let seq = self.ctx.agent_seq.clone();
                    let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                    let mut out: Vec<ReducerInput> = Vec::with_capacity(inputs.len());
                    for tid in inputs {
                        let t = store
                            .get(task.workspace_id, tid)
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| format!("input task not found: {tid}"))?;
                        let succeeded = matches!(t.state, TaskState::Succeeded);
                        let output = t
                            .result
                            .and_then(|r| r.output)
                            .unwrap_or(serde_json::Value::Null);
                        out.push(ReducerInput { succeeded, output });
                    }
                    Ok(out)
                });
                let collected = collected?;
                // 2단계: reduce — memory lock 바깥에서.
                let value = reduce_with_custom(strategy, &collected, run_custom_shell)
                    .map_err(|e| e.to_string())?;
                Ok(DispatchHandle::ReduceImmediate(TaskResult {
                    exit_code: Some(0),
                    output: Some(value),
                    error: None,
                }))
            }
            TaskCommand::Run { command, cwd, .. } => {
                if command.is_empty() {
                    return Err("Run: empty command".to_string());
                }
                let (program, args) = command.split_first().expect("non-empty");
                let mut cmd = std::process::Command::new(program);
                cmd.args(args);
                if let Some(c) = cwd {
                    cmd.current_dir(c);
                }
                let child = cmd
                    .spawn()
                    .map_err(|e| format!("Run spawn '{program}': {e}"))?;
                let pid = child.id();
                self.shell_children.insert(pid, child);
                Ok(DispatchHandle::ShellProcess { pid })
            }
            TaskCommand::Custom { ipc_method, params } => {
                // 동기 IPC dispatch — 즉시 완료 가정. 응답이 즉시 안 오면 timeout
                // 으로 Failed 처리됨 (HostIpcInjector::dispatch).
                let value = self
                    .ctx
                    .dispatch_plugin(ipc_method, params.clone())
                    .map_err(|e| format!("Custom '{ipc_method}': {e}"))?;
                Ok(DispatchHandle::CustomImmediate(TaskResult {
                    exit_code: Some(0),
                    output: Some(value),
                    error: None,
                }))
            }
            TaskCommand::WaitBarrier { name } => Ok(DispatchHandle::BarrierPoll {
                workspace_id: task.workspace_id,
                name: name.clone(),
            }),
        }
    }

    /// 내부 poll 본체.
    fn poll_handle(&mut self, handle: &DispatchHandle) -> PollOutcome {
        match handle {
            DispatchHandle::ClaudeChild {
                parent_sid,
                child_index,
                ..
            } => {
                let params = json!({
                    "surface_id": parent_sid,
                    "child_index": child_index,
                });
                let resp = match self.ctx.dispatch_plugin("claude.wait", params) {
                    Ok(v) => v,
                    Err(e) => return PollOutcome::Failed(format!("claude.wait: {e}")),
                };
                match resp["state"].as_str().unwrap_or("active") {
                    "active" => PollOutcome::Active,
                    s @ ("idle" | "needs_input" | "exited") => PollOutcome::Done(TaskResult {
                        exit_code: None,
                        output: Some(json!({
                            "final_state": s,
                            "child_index": child_index,
                            "parent_surface_id": parent_sid,
                        })),
                        error: None,
                    }),
                    other => PollOutcome::Failed(format!("unknown claude.wait state: {other}")),
                }
            }
            DispatchHandle::ReduceImmediate(r) | DispatchHandle::CustomImmediate(r) => {
                PollOutcome::Done(r.clone())
            }
            DispatchHandle::ShellProcess { pid } => {
                let child = match self.shell_children.get_mut(pid) {
                    Some(c) => c,
                    None => {
                        // handle 은 있는데 child map 에 없음 — 호스트 재시작 후
                        // 잔여 task 시나리오. process_alive 로 확인 후 처리.
                        if tasty_agent::platform::process_alive::is_alive(*pid) {
                            return PollOutcome::Active;
                        }
                        return PollOutcome::Failed(format!(
                            "Run handle lost (pid {pid} no longer tracked)"
                        ));
                    }
                };
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let code = status.code();
                        let pid_done = *pid;
                        self.shell_children.remove(&pid_done);
                        if status.success() {
                            PollOutcome::Done(TaskResult {
                                exit_code: code,
                                output: Some(json!({ "pid": pid_done })),
                                error: None,
                            })
                        } else {
                            PollOutcome::Failed(format!("Run exited non-zero: code={:?}", code))
                        }
                    }
                    Ok(None) => PollOutcome::Active,
                    Err(e) => PollOutcome::Failed(format!("Run try_wait: {e}")),
                }
            }
            DispatchHandle::ImmediateFail(err) => PollOutcome::Failed(err.clone()),
            DispatchHandle::BarrierPoll { workspace_id, name } => {
                let now = now_ms();
                let res = self.ctx.with_memory(|mem| {
                    let mut store = BarrierStore::new(mem, HOST_OWNER);
                    store.state(*workspace_id, name, now)
                });
                match res {
                    Ok(b) => match b.state {
                        BarrierState::Open => PollOutcome::Active,
                        BarrierState::Closed => PollOutcome::Done(TaskResult {
                            exit_code: Some(0),
                            output: Some(json!({
                                "barrier": name,
                                "count_signaled": b.count_signaled,
                                "count_required": b.count_required,
                            })),
                            error: None,
                        }),
                        BarrierState::TimedOut => {
                            PollOutcome::Failed(format!("barrier '{name}' timed out"))
                        }
                    },
                    Err(e) => PollOutcome::Failed(format!("barrier poll '{name}': {e}")),
                }
            }
        }
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
