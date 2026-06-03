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
    AgentError, BarrierState, BarrierStore, LeaseMode, LeaseStore, ReducerInput, SemaphoreStore,
    Task, TaskCommand, TaskId, TaskResult, reduce_with_custom,
};
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope};

/// DispatchHandle 영속 key prefix (workspace scope). S2: `RunnerLoop.running` 의
/// in-memory map 을 호스트 재시작 사이에 복원하기 위한 mirror. Immediate*/ImmediateFail
/// 은 영속 대상 아님 (다음 tick poll 에서 즉시 흡수).
pub const HANDLE_KEY_PREFIX: &str = "tasty.agent.handle.";

pub fn handle_key(task_id: &str) -> String {
    format!("{HANDLE_KEY_PREFIX}{task_id}")
}

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
    /// `agent.task_await` blocking 용 waker hub. tick 의 set_state 클로저가 종결 전이
    /// 시 fire. R-5 회피: runner_thread 가 Core wrapper 를 우회하기 때문에 RunnerContext
    /// 에 직접 포함시켜야 누락 없음.
    pub task_waker_hub: Arc<crate::core::agent::task_waker::TaskWakerHub>,
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
    /// 본 executor 가 dispatch 시 점유한 lease — (workspace_id, resource, holder).
    /// semaphore 와 같은 정책: in-memory only, 재시작 시 runner_thread 의 purge 가 회수.
    held_leases: HashMap<TaskId, (u32, String, String)>,
    /// 영속된 DispatchHandle 의 ws 추적 — release_permit 에서 evict_handle 호출 시
    /// ws 가 필요한데 handle 자체에서 식별 불가 (ShellProcess { pid } 등) 이므로
    /// persist_handle 시점에 함께 저장.
    held_handles: HashMap<TaskId, u32>,
}

impl HostExecutor {
    pub fn new(ctx: RunnerContext) -> Self {
        Self {
            ctx,
            shell_children: HashMap::new(),
            held_permits: HashMap::new(),
            held_leases: HashMap::new(),
            held_handles: HashMap::new(),
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

    /// `task.metadata.lease` 컨벤션을 읽어 lease 점유 시도.
    /// metadata 형식: `{ resource: String, holder?: String, ttl_ms?: u64, mode?: "fail"|"block" }`.
    /// 반환: `Ok(None)` = 미사용, `Ok(Some(true))` = 점유, `Ok(Some(false))` = 충돌 (Block 모드),
    /// `Err(msg)` = Fail 모드 충돌 또는 store 오류.
    fn try_acquire_lease(&mut self, task: &Task) -> Result<Option<bool>, String> {
        let Some(meta) = task.metadata.get("lease").and_then(|v| v.as_object()) else {
            return Ok(None);
        };
        let resource = meta
            .get("resource")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "lease metadata: missing 'resource'".to_string())?;
        let holder = meta
            .get("holder")
            .and_then(|v| v.as_str())
            .unwrap_or(task.id.as_str());
        let ttl_ms = meta.get("ttl_ms").and_then(|v| v.as_u64());
        let mode = match meta.get("mode").and_then(|v| v.as_str()) {
            Some("fail") => LeaseMode::Fail,
            // Block 가 dispatch 컨벤션 (semaphore 와 일관: 부족 → Deferred → 다음 tick).
            None | Some("block") => LeaseMode::Block,
            Some(other) => {
                return Err(format!(
                    "lease metadata: invalid mode '{other}' (expected 'fail'|'block')"
                ));
            }
        };
        let resource = resource.to_string();
        let holder = holder.to_string();
        let ws = task.workspace_id;
        let now = now_ms();
        let result: Result<bool, String> = self.ctx.with_memory(|mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            match store.acquire(ws, &resource, &holder, ttl_ms, mode, now) {
                Ok(o) => Ok(o.acquired),
                Err(AgentError::LeaseConflict { resource, holder }) => {
                    Err(format!("lease conflict: '{resource}' held by '{holder}'"))
                }
                Err(e) => Err(e.to_string()),
            }
        });
        let acquired = result?;
        if acquired {
            self.held_leases
                .insert(task.id.clone(), (ws, resource, holder));
        }
        Ok(Some(acquired))
    }

    /// DispatchHandle 영속. dispatch 가 Started 반환 직후 호출. ws 인자는
    /// `ShellProcess { pid }` variant 에 workspace_id 가 없어서 handle 자체로 식별 불가
    /// 하기에 호출자(dispatch)가 `task.workspace_id` 를 직접 전달한다.
    /// `Immediate*` / `ImmediateFail` 은 영속 대상 아님 — 다음 tick 에 흡수되므로
    /// 영속해 둘 의미가 없고, reload 시 재dispatch 되어 side-effect 위험.
    fn persist_handle(&mut self, ws: u32, task_id: &TaskId, handle: &DispatchHandle) {
        if matches!(
            handle,
            DispatchHandle::ReduceImmediate(_)
                | DispatchHandle::CustomImmediate(_)
                | DispatchHandle::ImmediateFail(_)
        ) {
            return;
        }
        let value = match serde_json::to_value(handle) {
            Ok(v) => MemoryValue::Json(v),
            Err(e) => {
                tracing::warn!("persist handle {task_id} serialize: {e}");
                return;
            }
        };
        let res = self.ctx.with_memory(|mem| {
            mem.put(
                HOST_OWNER,
                &Scope::Workspace(ws),
                &handle_key(task_id),
                &value,
                &PutOpts::default(),
            )
        });
        if let Err(e) = res {
            tracing::warn!("persist handle {task_id}: {e}");
            return;
        }
        self.held_handles.insert(task_id.clone(), ws);
    }

    /// 영속된 DispatchHandle 삭제. release_permit (task 종결) 시 호출.
    /// ws 는 `held_handles` 에서 꺼낸다 — handle 자체에서 식별 불가 (ShellProcess
    /// variant 에 workspace_id 없음) + task store 전수 검색은 race 위험.
    fn evict_handle(&mut self, task_id: &TaskId) {
        let Some(ws) = self.held_handles.remove(task_id) else {
            return; // 영속 안 됐던 task (Immediate*) — no-op.
        };
        let res = self.ctx.with_memory(|mem| {
            mem.delete(
                HOST_OWNER,
                &Scope::Workspace(ws),
                &handle_key(task_id),
                None,
            )
        });
        if let Err(e) = res {
            tracing::warn!("evict handle {task_id}: {e}");
        }
    }

    /// task 가 점유 중인 lease 가 있으면 release. release_permit 안에서 호출.
    fn release_lease(&mut self, task_id: &TaskId) {
        let Some((ws, resource, holder)) = self.held_leases.remove(task_id) else {
            return;
        };
        let res: Result<(), String> = self.ctx.with_memory(|mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            store
                .release(ws, &resource, &holder)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        if let Err(e) = res {
            tracing::warn!("lease release failed for task {task_id} ({resource}/{holder}): {e}");
        }
    }
}

impl TaskExecutor for HostExecutor {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
        // Phase J.A.S1: lease + semaphore-gated dispatch. lease → semaphore 순서
        // (lease 가 conflict 잦고 더 가벼움). 어느 한쪽이라도 막 점유한 후 다음 게이트가
        // 실패하면 점유한 자원 즉시 release (idempotent).
        match self.try_acquire_lease(task) {
            Ok(None) => {}
            Ok(Some(true)) => {}
            Ok(Some(false)) => return DispatchOutcome::Deferred,
            Err(e) => return DispatchOutcome::PermanentFail(format!("lease: {e}")),
        }
        match self.try_acquire_semaphore(task) {
            Ok(None) => {}
            Ok(Some(true)) => {}
            Ok(Some(false)) => {
                self.release_lease(&task.id);
                return DispatchOutcome::Deferred;
            }
            Err(e) => {
                self.release_lease(&task.id);
                return DispatchOutcome::PermanentFail(format!("semaphore: {e}"));
            }
        }
        let result = match self.dispatch_command(task) {
            Ok(h) => {
                // S2: Started 직후 영속 — handle 자체에서 ws 식별 불가 (ShellProcess
                // 에는 workspace_id 가 없음) 라 task.workspace_id 를 직접 전달.
                self.persist_handle(task.workspace_id, &task.id, &h);
                DispatchOutcome::Started(h)
            }
            Err(e) => DispatchOutcome::PermanentFail(e),
        };
        // dispatch 가 실패하면 막 점유한 자원을 즉시 반환.
        if matches!(result, DispatchOutcome::PermanentFail(_)) {
            self.release_permit(&task.id);
        }
        result
    }

    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        self.poll_handle(handle)
    }

    fn release_permit(&mut self, task_id: &TaskId) {
        // 의미: 이 task 의 모든 자원 해제 (semaphore + lease) + 영속 handle evict.
        if let Some((ws, name, holder)) = self.held_permits.remove(task_id) {
            let res: Result<(), String> = self.ctx.with_memory(|mem| {
                let mut store = SemaphoreStore::new(mem, HOST_OWNER);
                store
                    .release(ws, &name, &holder)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
            if let Err(e) = res {
                tracing::warn!(
                    "semaphore release failed for task {task_id} ({name}/{holder}): {e}"
                );
            }
        }
        self.release_lease(task_id);
        self.evict_handle(task_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_memory::MemoryStore;

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

    /// J.A.S2: persist 후 별도 reader 가 deserialize → 같은 variant 복원.
    #[test]
    fn persist_handle_round_trip_via_memory_store() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let handle = DispatchHandle::ShellProcess { pid: 4242 };
        exec.persist_handle(1, &"t-test".to_string(), &handle);

        // fresh read from memory store
        let loaded: Option<DispatchHandle> = ctx.with_memory(|mem| {
            let entry = mem
                .get(&Scope::Workspace(1), &handle_key("t-test"))
                .ok()??;
            match entry.value {
                MemoryValue::Json(v) => serde_json::from_value(v).ok(),
                _ => None,
            }
        });
        let loaded = loaded.expect("handle loaded");
        match loaded {
            DispatchHandle::ShellProcess { pid } => assert_eq!(pid, 4242),
            other => panic!("expected ShellProcess, got {other:?}"),
        }
    }

    /// J.A.S2: ImmediateFail / Reduce / Custom Immediate 는 영속 대상 아님.
    #[test]
    fn persist_handle_skips_immediate_variants() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let immediates = vec![
            DispatchHandle::ImmediateFail("e".into()),
            DispatchHandle::ReduceImmediate(TaskResult {
                exit_code: Some(0),
                output: None,
                error: None,
            }),
            DispatchHandle::CustomImmediate(TaskResult {
                exit_code: Some(0),
                output: None,
                error: None,
            }),
        ];
        for (i, h) in immediates.into_iter().enumerate() {
            let id = format!("t-im-{i}");
            exec.persist_handle(1, &id, &h);
            let present: bool = ctx.with_memory(|mem| {
                mem.get(&Scope::Workspace(1), &handle_key(&id))
                    .map(|v| v.is_some())
                    .unwrap_or(false)
            });
            assert!(!present, "Immediate handle {h:?} should not persist");
        }
    }

    /// J.A.S2: evict 후 store 에서 사라짐.
    #[test]
    fn evict_handle_removes_from_store() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let task_id = "t-evict".to_string();
        let handle = DispatchHandle::ClaudeChild {
            parent_sid: 1,
            child_index: 0,
            workspace_id: 1,
        };
        exec.persist_handle(1, &task_id, &handle);
        let present_before: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(present_before, "persist should write entry");

        exec.evict_handle(&task_id);
        let present_after: bool = ctx.with_memory(|mem| {
            mem.get(&Scope::Workspace(1), &handle_key(&task_id))
                .map(|v| v.is_some())
                .unwrap_or(false)
        });
        assert!(!present_after, "evict should remove entry");
        assert!(!exec.held_handles.contains_key(&task_id));
    }
}
