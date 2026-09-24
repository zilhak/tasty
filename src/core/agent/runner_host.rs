//! 러너 스레드에서 작업을 실행한다. Run은 자식 프로세스, Custom은 IPC와 완료 전략을 사용한다.
//! lease·작업 출력 치환, 실행 handle 보존, 폴링 결과 수집도 담당한다.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde_json::json;
use tasty_agent::runner::{DispatchHandle, DispatchOutcome, PollOutcome, TaskExecutor};
use tasty_agent::{
    AgentError, BarrierState, BarrierStore, ElasticSpec, LeaseMode, LeaseStore, ReducerInput,
    SemaphoreStore, Task, TaskCommand, TaskId, TaskResult, reduce_with_custom,
};
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope};

/// 재시작 뒤 실행 중인 작업을 복원할 workspace별 handle 키. 즉시 끝나는 handle은 저장하지 않는다.
pub const HANDLE_KEY_PREFIX: &str = "tasty.agent.handle.";

pub fn handle_key(task_id: &str) -> String {
    format!("{HANDLE_KEY_PREFIX}{task_id}")
}

/// IPC 조회가 외부 완료 신호의 wait_key·deadline도 보여줄 수 있도록 저장된 handle을 읽는다.
pub fn load_dispatch_handle(
    ctx: &RunnerContext,
    workspace_id: u32,
    task_id: &str,
) -> Option<DispatchHandle> {
    let scope = Scope::Workspace(workspace_id);
    ctx.with_memory(|mem| {
        let entry = mem.get(&scope, &handle_key(task_id)).ok().flatten()?;
        match entry.value {
            MemoryValue::Json(v) => serde_json::from_value(v).ok(),
            _ => None,
        }
    })
}

/// 자식 종료와 출력 수집 뒤 기록하는 결과 키. 기록 전에 호스트가 종료되거나 저장이 실패하면 남지 않을 수 있다.
pub const RUN_RESULT_KEY_PREFIX: &str = "tasty.agent.run_result.";

pub fn run_result_key(task_id: &str) -> String {
    format!("{RUN_RESULT_KEY_PREFIX}{task_id}")
}

use crate::core::agent::task_output_ref;
use tasty_agent::run_custom_shell;
use tasty_ipc::host_call::HostIpcInjector;

/// 최초 IPC 요청과 후속 poll 요청에 공통으로 사용하는 응답 대기 시간.
const HOST_DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

/// poll 오류 분류가 이 문자열을 사용하므로 반환부와 분류부에서 같은 상수를 쓴다.
pub(crate) const INJECTOR_UNINIT_MSG: &str = "host IPC injector not initialized";

/// injector가 아직 준비되지 않은 poll 실패를 잠시 Active로 두는 유예 시간.
pub(crate) const INJECTOR_GRACE_MS: u64 = 30_000;

/// 메서드명 등이 붙은 오류도 포함하므로 정확한 일치가 아닌 부분 문자열로 분류한다.
pub(crate) fn is_injector_not_initialized(msg: &str) -> bool {
    msg.contains(INJECTOR_UNINIT_MSG)
}

#[derive(Clone)]
pub struct RunnerContext {
    pub memory: Arc<Mutex<dyn MemoryStorage>>,
    pub agent_seq: Arc<AtomicU64>,
    pub host_ipc: Arc<OnceLock<HostIpcInjector>>,
    /// Core를 거치지 않는 러너의 종료 처리도 대기자를 깨울 수 있도록 같은 hub를 공유한다.
    pub task_waker_hub: Arc<crate::core::agent::task_waker::TaskWakerHub>,
    /// 러너가 push 대기를 등록하고 호스트가 훅 결과를 전달하는 공유 매핑.
    pub hook_task_waits: Arc<crate::core::agent::hook_wait::HookTaskWaits>,
}

static MEMORY_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

static RUN_RESULT_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl RunnerContext {
    /// poison은 로그로 알리고 남은 저장소를 계속 사용한다. 임의 MemoryStorage 호출의 중간 실패를 복구하는 것은 아니다.
    pub fn with_memory<R>(&self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        let mut guard = crate::poison::recover_mutex(
            self.memory.lock(),
            "agent runner memory",
            &MEMORY_POISON_REPORTED,
        );
        f(&mut *guard)
    }

    /// 큐 입장 거절을 여기서 재시도하면 적체를 늘리므로 오류를 그대로 전달한다.
    pub fn dispatch_plugin(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let inj = self
            .host_ipc
            .get()
            .ok_or_else(|| INJECTOR_UNINIT_MSG.to_string())?;
        inj.dispatch(method, params, HOST_DISPATCH_TIMEOUT)
            .map_err(|e| e.to_string())
    }
}

/// watcher가 자식 종료와 두 출력 파이프의 EOF를 기다린 뒤 결과를 채운다.
/// 취소·permit 해제는 자식을 종료시키지 않는다. executor가 사라져도 watcher는 분리되어 계속 기다릴 수 있다.
struct ShellChildEntry {
    result: Arc<Mutex<Option<PollOutcome>>>,
    _watcher: thread::JoinHandle<()>,
}

pub struct HostExecutor {
    ctx: RunnerContext,
    /// Child는 watcher가 소유하고 Clone 가능한 DispatchHandle에는 PID만 남긴다.
    shell_children: HashMap<u32, ShellChildEntry>,
    /// 이 executor가 얻은 permit. 재시작 시 메모리 기록은 없어져 러너 시작 단계에서 저장된 holder를 정리한다.
    held_permits: HashMap<TaskId, (u32, String, String)>,
    /// 이 executor가 얻은 lease. 재시작 정리는 metadata.resource와 task ID holder로 찾은 Running 작업에 한정된다.
    held_leases: HashMap<TaskId, (u32, String, String)>,
    /// workspace가 없는 handle도 삭제할 수 있도록 저장 시 task별 workspace를 기억한다.
    held_handles: HashMap<TaskId, u32>,
    /// workspace executor의 모든 PolledDispatch가 공유한다. 한 poll이라도 성공하면 유예를 초기화한다.
    injector_grace_deadline_ms: Option<u64>,
}

impl HostExecutor {
    pub fn new(ctx: RunnerContext) -> Self {
        Self {
            ctx,
            shell_children: HashMap::new(),
            held_permits: HashMap::new(),
            held_leases: HashMap::new(),
            held_handles: HashMap::new(),
            injector_grace_deadline_ms: None,
        }
    }

    /// semaphore metadata가 없으면 None, 얻었으면 Some(true), 부족하면 Some(false)다. 잘못된 name·저장소 오류는 Err다.
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
        // TTL을 생략하면 자동 만료시키지 않는다.
        let ttl_ms = meta.get("ttl_ms").and_then(|v| v.as_u64());
        let name = name.to_string();
        let holder = holder.to_string();
        let ws = task.workspace_id;
        let now = now_ms();
        let result: Result<bool, String> = self.ctx.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store
                .acquire(ws, &name, &holder, ttl_ms, now)
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

    /// resource 하나 또는 candidates 목록에서 자원을 얻는다. holder 기본값은 task ID다.
    /// elastic을 명시해야 후보를 자동 추가하며 생략하면 고정 목록만 사용한다.
    /// Block 충돌은 Some(false), Fail 충돌과 설정·저장소 오류는 Err다. 획득한 자원은 held_leases에 기록한다.
    fn try_acquire_lease(&mut self, task: &Task) -> Result<Option<bool>, String> {
        let Some(meta) = task.metadata.get("lease").and_then(|v| v.as_object()) else {
            return Ok(None);
        };
        let candidates: Vec<String> = if let Some(arr) = meta.get("candidates") {
            let arr = arr
                .as_array()
                .ok_or_else(|| "lease metadata: 'candidates' must be an array".to_string())?;
            arr.iter()
                .map(|v| {
                    v.as_str().map(str::to_string).ok_or_else(|| {
                        "lease metadata: 'candidates' entries must be strings".to_string()
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            let resource = meta
                .get("resource")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "lease metadata: missing 'resource' or 'candidates'".to_string())?;
            vec![resource.to_string()]
        };
        if candidates.is_empty() {
            return Err("lease metadata: 'candidates' must be non-empty".to_string());
        }
        let holder = meta
            .get("holder")
            .and_then(|v| v.as_str())
            .unwrap_or(task.id.as_str());
        let ttl_ms = meta.get("ttl_ms").and_then(|v| v.as_u64());
        let mode = match meta.get("mode").and_then(|v| v.as_str()) {
            Some("fail") => LeaseMode::Fail,
            // 점유가 부족하면 다음 tick에서 다시 시도하도록 기본 모드는 Block이다.
            None | Some("block") => LeaseMode::Block,
            Some(other) => {
                return Err(format!(
                    "lease metadata: invalid mode '{other}' (expected 'fail'|'block')"
                ));
            }
        };
        let elastic: Option<ElasticSpec> = match meta.get("elastic") {
            None => None,
            Some(v) => {
                let obj = v
                    .as_object()
                    .ok_or_else(|| "lease metadata: 'elastic' must be an object".to_string())?;
                let max_candidates = match obj.get("max_candidates") {
                    None => None,
                    Some(v) => Some(v.as_u64().ok_or_else(|| {
                        "lease metadata: 'elastic.max_candidates' must be a non-negative integer"
                            .to_string()
                    })? as u32),
                };
                let overflow_prefix = match obj.get("overflow_prefix") {
                    None => None,
                    Some(v) => Some(
                        v.as_str()
                            .ok_or_else(|| {
                                "lease metadata: 'elastic.overflow_prefix' must be a string"
                                    .to_string()
                            })?
                            .to_string(),
                    ),
                };
                Some(ElasticSpec {
                    max_candidates,
                    overflow_prefix,
                })
            }
        };
        let holder = holder.to_string();
        let ws = task.workspace_id;
        let now = now_ms();
        let result: Result<(bool, Option<String>), String> = self.ctx.with_memory(|mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            match store.acquire_any(
                ws,
                &candidates,
                &holder,
                ttl_ms,
                mode,
                elastic.as_ref(),
                now,
            ) {
                Ok(o) => Ok((o.acquired, o.resource)),
                Err(AgentError::LeaseConflict { resource, holder }) => {
                    Err(format!("lease conflict: '{resource}' held by '{holder}'"))
                }
                Err(AgentError::LeasePoolExhausted { candidates, holder }) => Err(format!(
                    "lease pool exhausted: none of {candidates:?} available for '{holder}'"
                )),
                Err(e) => Err(e.to_string()),
            }
        });
        let (acquired, resource) = result?;
        if acquired {
            let resource = resource.expect("acquire_any: acquired=true implies resource");
            self.held_leases
                .insert(task.id.clone(), (ws, resource, holder));
        }
        Ok(Some(acquired))
    }

    /// Started를 반환하기 전에 실행 handle을 저장한다. workspace는 handle에 없을 수 있어 별도로 받는다.
    /// 즉시 종료 handle은 저장하지 않으며 저장 실패는 로그를 남긴다.
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

    fn evict_handle(&mut self, task_id: &TaskId) {
        let Some(ws) = self.held_handles.remove(task_id) else {
            return;
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
        evict_run_result(&self.ctx, ws, task_id);
    }

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

    /// 저장소 락 안에서는 선행 작업 결과만 읽고 문자열 치환은 락 밖에서 수행한다.
    fn collect_task_outputs(
        &mut self,
        task: &Task,
    ) -> Result<HashMap<TaskId, serde_json::Value>, String> {
        let ids = task_output_ref::referenced_tasks(&task.command).map_err(|e| e.0)?;
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ws = task.workspace_id;
        let seq = self.ctx.agent_seq.clone();
        self.ctx.with_memory(|mem| {
            use tasty_agent::TaskStore;
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let mut out = HashMap::with_capacity(ids.len());
            for tid in ids {
                let t = store
                    .get(ws, &tid)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("task output reference '{tid}': task not found"))?;
                // 결과가 아직 없으면 null을 넣지 않고 참조 해석 실패로 알린다.
                let output = t.result.and_then(|r| r.output).ok_or_else(|| {
                    format!("task output reference '{tid}': upstream task has no result output yet")
                })?;
                out.insert(tid, output);
            }
            Ok(out)
        })
    }

    /// 조회에도 실제 실행할 치환값이 보이도록 command를 저장한다. 저장 실패는 경고하고 실행은 계속한다.
    fn persist_substituted_command(&mut self, ws: u32, task: &Task) {
        let seq = self.ctx.agent_seq.clone();
        let res: Result<(), String> = self.ctx.with_memory(|mem| {
            use tasty_agent::TaskStore;
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let Some(mut stored) = store.get(ws, &task.id).map_err(|e| e.to_string())? else {
                return Ok(());
            };
            stored.command = task.command.clone();
            store.put(&stored).map_err(|e| e.to_string())
        });
        if let Err(e) = res {
            tracing::warn!("persist lease-substituted command for {}: {e}", task.id);
        }
    }
}

impl TaskExecutor for HostExecutor {
    fn dispatch(&mut self, task: &Task) -> DispatchOutcome {
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
        // lease를 먼저, 선행 작업 출력을 나중에 치환한다.
        // 출력 데이터 안의 lease 표식을 다시 해석하지 않기 위한 순서다.
        let mut substituted = task.clone();
        let leased = self.held_leases.get(&task.id).cloned();
        if let Some((_, resource, _)) = &leased {
            substitute_lease_resource(&mut substituted.command, resource);
        }
        let outputs_substituted = match self.collect_task_outputs(task) {
            Ok(outputs) if outputs.is_empty() => Ok(false),
            Ok(outputs) => substitute_task_outputs(&mut substituted.command, &outputs),
            Err(e) => Err(e),
        };
        let dispatch_result = match outputs_substituted {
            Ok(changed) => {
                // 바뀐 command만 저장해 조회와 실제 실행 인자를 맞춘다.
                if let Some((ws, _, _)) = &leased {
                    self.persist_substituted_command(*ws, &substituted);
                } else if changed {
                    self.persist_substituted_command(task.workspace_id, &substituted);
                }
                self.dispatch_command(&substituted)
            }
            Err(e) => Err(format!("task output substitution: {e}")),
        };
        let result = match dispatch_result {
            Ok(h) => {
                self.persist_handle(task.workspace_id, &task.id, &h);
                DispatchOutcome::Started(h)
            }
            Err(e) => DispatchOutcome::PermanentFail(e),
        };
        if matches!(result, DispatchOutcome::PermanentFail(_)) {
            self.release_permit(&task.id);
        }
        result
    }

    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome {
        self.poll_handle(handle)
    }

    fn release_permit(&mut self, task_id: &TaskId) {
        // 이름과 달리 permit뿐 아니라 lease·저장된 handle도 정리한다. 자식 프로세스는 종료시키지 않는다.
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
    fn dispatch_command(&mut self, task: &Task) -> Result<DispatchHandle, String> {
        match &task.command {
            TaskCommand::Reduce { inputs, strategy } => {
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
                        out.push(ReducerInput {
                            succeeded,
                            task_id: tid.clone(),
                            output,
                        });
                    }
                    Ok(out)
                });
                let collected = collected?;
                // 사용자 reduce 작업은 저장소 락 밖에서 실행한다.
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
                tasty_utils::process::hide_console(&mut cmd);
                cmd.args(args);
                if let Some(c) = cwd {
                    cmd.current_dir(c);
                }
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());
                let mut child = cmd
                    .spawn()
                    .map_err(|e| format!("Run spawn '{program}': {e}"))?;
                let pid = child.id();
                // 자식이 파이프를 채운 채 종료를 기다리지 않도록 stdout·stderr를 wait와 동시에 읽는다.
                let stdout_pipe = child.stdout.take().expect("stdout piped");
                let stderr_pipe = child.stderr.take().expect("stderr piped");
                let stdout_thread = thread::Builder::new()
                    .name(format!("agent-shell-stdout-pid{pid}"))
                    .spawn(move || drain_capped(stdout_pipe))
                    .map_err(|e| format!("Run stdout drain spawn '{program}': {e}"))?;
                let stderr_thread = thread::Builder::new()
                    .name(format!("agent-shell-stderr-pid{pid}"))
                    .spawn(move || drain_capped(stderr_pipe))
                    .map_err(|e| format!("Run stderr drain spawn '{program}': {e}"))?;
                let result_cell: Arc<Mutex<Option<PollOutcome>>> = Arc::new(Mutex::new(None));
                let cell_clone = result_cell.clone();
                let mem_clone = self.ctx.memory.clone();
                let task_id_clone = task.id.clone();
                let ws = task.workspace_id;
                // 자식 종료 뒤에도 상속된 파이프가 열려 있으면 drain join은 계속 기다릴 수 있다.
                let watcher = thread::Builder::new()
                    .name(format!("agent-shell-watcher-pid{pid}"))
                    .spawn(move || {
                        let status = child.wait();
                        let stdout = stdout_thread.join().unwrap_or_default();
                        let stderr = stderr_thread.join().unwrap_or_default();
                        let outcome = match status {
                            Ok(status) => shell_outcome_from_status(
                                pid,
                                status.code(),
                                status.success(),
                                stdout,
                                stderr,
                            ),
                            Err(e) => PollOutcome::Failed(format!("Run wait: {e}")),
                        };
                        persist_run_result(&mem_clone, ws, &task_id_clone, &outcome);
                        // 결과를 기록하지 못하면 poll은 계속 Active라 poison을 알리고 cell을 사용한다.
                        *crate::poison::recover_mutex(
                            cell_clone.lock(),
                            "agent run result cell",
                            &RUN_RESULT_POISON_REPORTED,
                        ) = Some(outcome);
                    })
                    .map_err(|e| format!("Run watcher spawn '{program}': {e}"))?;
                self.shell_children.insert(
                    pid,
                    ShellChildEntry {
                        result: result_cell,
                        _watcher: watcher,
                    },
                );
                Ok(DispatchHandle::ShellProcess { pid })
            }
            TaskCommand::Custom {
                ipc_method,
                params,
                poll,
            } => {
                // IPC를 먼저 실행한 뒤 완료 전략을 해석한다. 전략 해석 실패가 이미 실행한 요청을 되돌리지는 않는다.
                // poll 미지정 시 기본 전략을 사용하고 그것도 없으면 응답으로 즉시 끝낸다.
                let value = self
                    .ctx
                    .dispatch_plugin(ipc_method, params.clone())
                    .map_err(|e| format!("Custom '{ipc_method}': {e}"))?;
                use tasty_agent::PollSpecRef;
                let spec: tasty_agent::PollSpec = match poll {
                    Some(PollSpecRef::Inline(spec)) => spec.clone(),
                    Some(PollSpecRef::Named { strategy }) => {
                        let id =
                            crate::completion_strategy::CompletionStrategyId::new(strategy.clone());
                        let strat = crate::completion_strategy::global()
                            .resolve_strategy(&id)
                            .map_err(|e| {
                                format!("Custom '{ipc_method}' poll strategy '{strategy}': {e}")
                            })?;
                        match strat.kind {
                            crate::completion_strategy::CompletionStrategyKind::Poll(spec) => spec,
                            crate::completion_strategy::CompletionStrategyKind::Push {
                                notify_via,
                                timeout_ms,
                            } => {
                                return self.dispatch_push_strategy(
                                    task,
                                    ipc_method,
                                    params,
                                    strat.id.as_str(),
                                    &notify_via,
                                    timeout_ms,
                                );
                            }
                        }
                    }
                    None => {
                        match crate::completion_strategy::global()
                            .resolve_default_for_method(ipc_method)
                        {
                            Some(strat) => match strat.kind {
                                crate::completion_strategy::CompletionStrategyKind::Poll(spec) => {
                                    spec
                                }
                                crate::completion_strategy::CompletionStrategyKind::Push {
                                    notify_via,
                                    timeout_ms,
                                } => {
                                    return self.dispatch_push_strategy(
                                        task,
                                        ipc_method,
                                        params,
                                        strat.id.as_str(),
                                        &notify_via,
                                        timeout_ms,
                                    );
                                }
                            },
                            None => {
                                return Ok(DispatchHandle::CustomImmediate(TaskResult {
                                    exit_code: Some(0),
                                    output: Some(value),
                                    error: None,
                                }));
                            }
                        }
                    }
                };
                let spec = &spec;
                // 같은 poll 인자를 매핑하면 응답값이 요청값을 덮는다.
                let mut poll_params = serde_json::Map::new();
                for (req_key, poll_key) in &spec.map_from_request {
                    if let Some(v) = params.get(req_key) {
                        poll_params.insert(poll_key.clone(), v.clone());
                    }
                }
                for (resp_key, poll_key) in &spec.map_from_response {
                    if let Some(v) = value.get(resp_key) {
                        poll_params.insert(poll_key.clone(), v.clone());
                    }
                }
                let deadline_ms = spec.timeout_ms.map(|t| now_ms() + t);
                Ok(DispatchHandle::PolledDispatch {
                    workspace_id: task.workspace_id,
                    poll_method: spec.poll_method.clone(),
                    poll_params: serde_json::Value::Object(poll_params),
                    state_field: spec.state_field.clone(),
                    terminal_states: spec.terminal_states.clone(),
                    failure_states: spec.failure_states.clone(),
                    interval_ms: spec.interval_ms,
                    deadline_ms,
                })
            }
            TaskCommand::WaitBarrier { name } => Ok(DispatchHandle::BarrierPoll {
                workspace_id: task.workspace_id,
                name: name.clone(),
            }),
        }
    }

    /// params.surface_id에 command-completed 일회성 훅을 걸고 외부 완료를 기다린다.
    /// 현재 이벤트 종류는 고정이다. 훅 수신 또는 별도 만료 처리가 task를 종결하며 이 handle의 poll은 Active다.
    fn dispatch_push_strategy(
        &mut self,
        task: &Task,
        ipc_method: &str,
        params: &serde_json::Value,
        strategy_id: &str,
        notify_via: &crate::hook_handler::HookHandlerId,
        timeout_ms: u64,
    ) -> Result<DispatchHandle, String> {
        let surface_id = params
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                format!(
                    "Custom '{ipc_method}' push strategy '{strategy_id}': missing 'surface_id' \
                 param — push completion needs a target surface to bind the completion hook to"
                )
            })?;
        let hook_params = json!({
            "surface_id": surface_id,
            "event": "command-completed",
            "handler": notify_via.as_str(),
            "once": true,
        });
        let hook_resp = self
            .ctx
            .dispatch_plugin("hook.set", hook_params)
            .map_err(|e| {
                format!("Custom '{ipc_method}' push strategy '{strategy_id}': hook.set failed: {e}")
            })?;
        let hook_id = hook_resp.get("hook_id").and_then(|v| v.as_u64()).ok_or_else(|| {
            format!(
                "Custom '{ipc_method}' push strategy '{strategy_id}': hook.set response missing 'hook_id'"
            )
        })?;
        let deadline_ms = now_ms() + timeout_ms;
        self.ctx
            .hook_task_waits
            .register(hook_id, task.workspace_id, task.id.clone(), deadline_ms);
        // 훅 매핑은 재시작 때 사라져도 handle의 기한으로 reload에서 만료를 판단할 수 있게 한다.
        Ok(DispatchHandle::AwaitExternal {
            wait_key: hook_id.to_string(),
            deadline_ms,
        })
    }

    fn poll_handle(&mut self, handle: &DispatchHandle) -> PollOutcome {
        match handle {
            DispatchHandle::PolledDispatch {
                poll_method,
                poll_params,
                state_field,
                terminal_states,
                failure_states,
                deadline_ms,
                ..
            } => {
                let resp = match self.ctx.dispatch_plugin(poll_method, poll_params.clone()) {
                    Ok(v) => {
                        self.injector_grace_deadline_ms = None;
                        v
                    }
                    Err(e) if is_injector_not_initialized(&e) => {
                        let now = now_ms();
                        let deadline = *self
                            .injector_grace_deadline_ms
                            .get_or_insert(now + INJECTOR_GRACE_MS);
                        if now < deadline {
                            return PollOutcome::Active;
                        }
                        return PollOutcome::Failed(format!(
                            "{poll_method}: injector grace expired ({INJECTOR_GRACE_MS}ms)"
                        ));
                    }
                    Err(e) => return PollOutcome::Failed(format!("{poll_method}: {e}")),
                };
                let state = resp.get(state_field).and_then(|v| v.as_str()).unwrap_or("");
                // 성공·실패 목록에 모두 있으면 실패를 우선한다. 없는 산출물로 후속 작업을 진행하지 않게 한다.
                if failure_states.iter().any(|s| s == state) {
                    return PollOutcome::Failed(format!(
                        "{poll_method}: failure state '{state}' — {}",
                        summarize_poll_response(&resp)
                    ));
                }
                if terminal_states.iter().any(|s| s == state) {
                    PollOutcome::Done(TaskResult {
                        exit_code: None,
                        output: Some(resp),
                        error: None,
                    })
                } else {
                    // 미완료 응답에서 기한을 확인한다. 한 번의 IPC 대기 자체를 이 기한으로 중단하지는 않는다.
                    if let Some(deadline) = deadline_ms
                        && now_ms() >= *deadline
                    {
                        return PollOutcome::Failed(format!("{poll_method}: poll timeout"));
                    }
                    PollOutcome::Active
                }
            }
            DispatchHandle::ReduceImmediate(r) | DispatchHandle::CustomImmediate(r) => {
                PollOutcome::Done(r.clone())
            }
            DispatchHandle::ShellProcess { pid } => {
                if let Some(entry) = self.shell_children.get(pid) {
                    let taken = crate::poison::recover_mutex(
                        entry.result.lock(),
                        "agent run result cell",
                        &RUN_RESULT_POISON_REPORTED,
                    )
                    .take();
                    if let Some(outcome) = taken {
                        self.shell_children.remove(pid);
                        return outcome;
                    }
                    return PollOutcome::Active;
                }
                // 이 executor의 watcher가 없으면 PID 생존 여부만 본다. 재사용된 PID가 원래 자식인지 확인하지 않는다.
                // 저장된 종료 결과의 적용은 reload 단계가 담당한다.
                if tasty_agent::platform::process_alive::is_alive(*pid) {
                    return PollOutcome::Active;
                }
                PollOutcome::Failed(format!("Run handle lost (pid {pid} no longer tracked)"))
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
            // 외부 훅·만료 처리가 store를 종결시키면 다음 러너 tick이 handle과 점유 자원을 정리한다.
            DispatchHandle::AwaitExternal { .. } => PollOutcome::Active,
        }
    }
}

/// 실행 전에 lease로 받은 자원을 넣는 표식. Run 인자·cwd와 Custom의 문자열 값에 적용한다.
const LEASE_RESOURCE_PLACEHOLDER: &str = "${lease.resource}";

/// Run의 cwd가 없으면 lease 자원으로 채우고, 있으면 표식만 치환한다.
/// Custom의 JSON 문자열 값도 치환하며 Reduce·WaitBarrier는 바꾸지 않는다.
fn substitute_lease_resource(command: &mut TaskCommand, resource: &str) {
    match command {
        TaskCommand::Run { command, cwd, .. } => {
            for arg in command.iter_mut() {
                if arg.contains(LEASE_RESOURCE_PLACEHOLDER) {
                    *arg = arg.replace(LEASE_RESOURCE_PLACEHOLDER, resource);
                }
            }
            match cwd {
                None => *cwd = Some(std::path::PathBuf::from(resource)),
                Some(existing) => {
                    if let Some(s) = existing.to_str()
                        && s.contains(LEASE_RESOURCE_PLACEHOLDER)
                    {
                        *cwd = Some(std::path::PathBuf::from(
                            s.replace(LEASE_RESOURCE_PLACEHOLDER, resource),
                        ));
                    }
                }
            }
        }
        TaskCommand::Custom { params, .. } => {
            substitute_lease_resource_in_json(params, resource);
        }
        TaskCommand::Reduce { .. } | TaskCommand::WaitBarrier { .. } => {}
    }
}

fn substitute_lease_resource_in_json(value: &mut serde_json::Value, resource: &str) {
    match value {
        serde_json::Value::String(s) => {
            if s.contains(LEASE_RESOURCE_PLACEHOLDER) {
                *s = s.replace(LEASE_RESOURCE_PLACEHOLDER, resource);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_lease_resource_in_json(v, resource);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                substitute_lease_resource_in_json(v, resource);
            }
        }
        _ => {}
    }
}

/// JSON 문자열 전체가 표식 하나이면 원래 값의 타입을 유지한다. 다른 글자와 섞이면 문자열로 보간한다.
/// Run 인자·cwd는 문자열로 보간하며 바뀐 값이 있으면 true다. 문법은 task_output_ref가 담당한다.
fn substitute_task_outputs(
    command: &mut TaskCommand,
    outputs: &HashMap<TaskId, serde_json::Value>,
) -> Result<bool, String> {
    let mut changed = false;
    match command {
        TaskCommand::Run { command, cwd, .. } => {
            for arg in command.iter_mut() {
                if let Some(next) = interpolate_string(arg, outputs)? {
                    *arg = next;
                    changed = true;
                }
            }
            if let Some(p) = cwd.as_ref()
                && let Some(s) = p.to_str()
                && let Some(next) = interpolate_string(s, outputs)?
            {
                *cwd = Some(std::path::PathBuf::from(next));
                changed = true;
            }
        }
        TaskCommand::Custom { params, .. } => {
            substitute_task_outputs_in_json(params, outputs, &mut changed)?;
        }
        TaskCommand::Reduce { .. } | TaskCommand::WaitBarrier { .. } => {}
    }
    Ok(changed)
}

fn substitute_task_outputs_in_json(
    value: &mut serde_json::Value,
    outputs: &HashMap<TaskId, serde_json::Value>,
    changed: &mut bool,
) -> Result<(), String> {
    match value {
        serde_json::Value::String(s) => {
            let refs = task_output_ref::parse_refs(s).map_err(|e| e.0)?;
            if refs.is_empty() {
                return Ok(());
            }
            if refs.len() == 1 && refs[0].0.start == 0 && refs[0].0.end == s.len() {
                *value = resolve(&refs[0].1, outputs)?.clone();
            } else if let Some(next) = interpolate_string(s, outputs)? {
                *s = next;
            }
            *changed = true;
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_task_outputs_in_json(v, outputs, changed)?;
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                substitute_task_outputs_in_json(v, outputs, changed)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 문자열 결과는 내용만, 나머지 값은 compact JSON으로 넣는다.
/// 원문 표식만 한 번 치환하며 주입한 값에 든 표식을 다시 해석하지 않는다.
fn interpolate_string(
    s: &str,
    outputs: &HashMap<TaskId, serde_json::Value>,
) -> Result<Option<String>, String> {
    let refs = task_output_ref::parse_refs(s).map_err(|e| e.0)?;
    if refs.is_empty() {
        return Ok(None);
    }
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    for (range, r) in &refs {
        out.push_str(&s[cursor..range.start]);
        let v = resolve(r, outputs)?;
        match v {
            serde_json::Value::String(text) => out.push_str(text),
            other => out.push_str(&other.to_string()),
        }
        cursor = range.end;
    }
    out.push_str(&s[cursor..]);
    Ok(Some(out))
}

fn resolve<'a>(
    r: &task_output_ref::TaskOutputRef,
    outputs: &'a HashMap<TaskId, serde_json::Value>,
) -> Result<&'a serde_json::Value, String> {
    let output = outputs.get(&r.task_id).ok_or_else(|| {
        format!(
            "task output reference '{}': upstream task has no result output yet",
            r.task_id
        )
    })?;
    output.pointer(&r.pointer).ok_or_else(|| {
        format!(
            "task output reference '{}': JSON pointer '{}' not found in output {}",
            r.task_id,
            if r.pointer.is_empty() { "" } else { &r.pointer },
            output
        )
    })
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 출력 뒤쪽의 실패 요약을 보존한다. 앞부분의 오류는 빠질 수 있으며 ANSI escape는 유지한다.
/// 두 tail의 JSON 이스케이프를 고려해 기본 저장소 항목 상한보다 작게 잡았다. 더 낮은 설정에서는 저장이 실패할 수 있다.
const CAPTURE_TAIL_CAP: usize = 64 * 1024;

/// 스트림 뒤쪽의 제한된 바이트와 잘린 양. 읽기 오류도 종료로 처리해 출력이 불완전할 수 있다.
#[derive(Debug, Default)]
pub(crate) struct DrainedStream {
    data: Vec<u8>,
    truncated: bool,
    dropped_bytes: u64,
}

impl DrainedStream {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "text": self.text(),
            "truncated": self.truncated,
            "dropped_bytes": self.dropped_bytes,
        })
    }
}

/// task 오류에 저장할 poll 응답 요약의 바이트 상한.
const POLL_FAILURE_SUMMARY_CAP: usize = 2 * 1024;

/// Failed는 문자열 하나만 전달하므로 응답 JSON을 길이 제한한 진단에 포함한다.
fn summarize_poll_response(resp: &serde_json::Value) -> String {
    let mut text = resp.to_string();
    if text.len() > POLL_FAILURE_SUMMARY_CAP {
        // UTF-8 문자 중간을 자르지 않는다.
        let mut cut = POLL_FAILURE_SUMMARY_CAP;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("...(truncated)");
    }
    text
}

/// EOF나 읽기 오류까지 읽고 마지막 CAPTURE_TAIL_CAP 바이트만 남긴다.
/// 파이프가 찬 자식의 종료를 기다리는 교착을 피하려고 child.wait와 별도 스레드에서 실행한다.
fn drain_capped<R: std::io::Read>(mut reader: R) -> DrainedStream {
    let mut data = Vec::with_capacity(CAPTURE_TAIL_CAP);
    let mut dropped_bytes: u64 = 0;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                data.extend_from_slice(&chunk[..n]);
                if data.len() > CAPTURE_TAIL_CAP {
                    let excess = data.len() - CAPTURE_TAIL_CAP;
                    data.drain(0..excess);
                    dropped_bytes += excess as u64;
                }
            }
            Err(_) => break,
        }
    }
    DrainedStream {
        truncated: dropped_bytes > 0,
        dropped_bytes,
        data,
    }
}

/// 성공은 구조화된 출력으로, 실패는 종료 코드와 출력 tail을 포함한 오류 문자열로 반환한다.
pub(crate) fn shell_outcome_from_status(
    pid: u32,
    code: Option<i32>,
    success: bool,
    stdout: DrainedStream,
    stderr: DrainedStream,
) -> PollOutcome {
    if success {
        PollOutcome::Done(TaskResult {
            exit_code: code,
            output: Some(json!({
                "pid": pid,
                "stdout": stdout.to_json(),
                "stderr": stderr.to_json(),
            })),
            error: None,
        })
    } else {
        let stdout_note = if stdout.truncated {
            format!(" (truncated, {} bytes dropped)", stdout.dropped_bytes)
        } else {
            String::new()
        };
        let stderr_note = if stderr.truncated {
            format!(" (truncated, {} bytes dropped)", stderr.dropped_bytes)
        } else {
            String::new()
        };
        PollOutcome::Failed(format!(
            "Run exited non-zero: code={:?}\n--- stdout{stdout_note} ---\n{}\n--- stderr{stderr_note} ---\n{}",
            code,
            stdout.text(),
            stderr.text(),
        ))
    }
}

/// 자식 종료와 출력 수집 뒤 결과를 저장한다. 실패하면 경고하지만 cell에는 결과를 계속 전달한다.
pub(crate) fn persist_run_result(
    memory: &Arc<Mutex<dyn MemoryStorage>>,
    workspace_id: u32,
    task_id: &str,
    outcome: &PollOutcome,
) {
    let value = MemoryValue::Json(run_outcome_to_value(outcome));
    let res = {
        let mut guard = crate::poison::recover_mutex(
            memory.lock(),
            "agent runner memory",
            &MEMORY_POISON_REPORTED,
        );
        guard.put(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &run_result_key(task_id),
            &value,
            &PutOpts::default(),
        )
    };
    if let Err(e) = res {
        tracing::warn!("persist run_result {task_id}: {e}");
    }
}

pub(crate) fn load_run_result(
    ctx: &RunnerContext,
    workspace_id: u32,
    task_id: &str,
) -> Option<PollOutcome> {
    ctx.with_memory(|mem| {
        let entry = mem
            .get(&Scope::Workspace(workspace_id), &run_result_key(task_id))
            .ok()??;
        match entry.value {
            MemoryValue::Json(v) => run_outcome_from_value(&v),
            _ => None,
        }
    })
}

pub(crate) fn evict_run_result(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    let res = ctx.with_memory(|mem| {
        mem.delete(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &run_result_key(task_id),
            None,
        )
    });
    if let Err(e) = res {
        tracing::warn!("evict run_result {task_id}: {e}");
    }
}

/// executor 밖의 삭제·GC도 정리할 수 있도록 task와 workspace ID로 handle을 지운다.
pub(crate) fn evict_handle_key(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    let res = ctx.with_memory(|mem| {
        mem.delete(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &handle_key(task_id),
            None,
        )
    });
    if let Err(e) = res {
        tracing::warn!("evict handle {task_id}: {e}");
    }
}

/// 정상 종료를 거치지 않고 task를 삭제해도 handle과 실행 결과 키를 함께 지운다.
pub(crate) fn evict_task_side_keys(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    evict_handle_key(ctx, workspace_id, task_id);
    evict_run_result(ctx, workspace_id, task_id);
}

fn run_outcome_to_value(outcome: &PollOutcome) -> serde_json::Value {
    match outcome {
        PollOutcome::Done(r) => json!({
            "kind": "done",
            "exit_code": r.exit_code,
            "output": r.output,
            "error": r.error,
        }),
        PollOutcome::Failed(e) => json!({
            "kind": "failed",
            "error": e,
        }),
        PollOutcome::Active => json!({ "kind": "active" }),
    }
}

fn run_outcome_from_value(v: &serde_json::Value) -> Option<PollOutcome> {
    match v.get("kind")?.as_str()? {
        "done" => {
            let exit_code = v
                .get("exit_code")
                .and_then(|x| x.as_i64())
                .map(|x| x as i32);
            let output = v.get("output").cloned();
            let error = v
                .get("error")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            Some(PollOutcome::Done(TaskResult {
                exit_code,
                output,
                error,
            }))
        }
        "failed" => Some(PollOutcome::Failed(v.get("error")?.as_str()?.to_string())),
        _ => None,
    }
}

#[cfg(test)]
// 이유: 시험의 반환값 무시는 허용하되 제품 코드의 검사는 유지한다.
#[allow(clippy::let_underscore_must_use)]
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
            hook_task_waits: Arc::new(crate::core::agent::hook_wait::HookTaskWaits::new()),
        };
        (td, ctx)
    }

    #[test]
    fn persist_handle_round_trip_via_memory_store() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let handle = DispatchHandle::ShellProcess { pid: 4242 };
        exec.persist_handle(1, &"t-test".to_string(), &handle);

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

    #[test]
    fn run_outcome_done_serde_round_trip() {
        let outcome = PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: Some(json!({ "pid": 1234u32 })),
            error: None,
        });
        let v = run_outcome_to_value(&outcome);
        let back = run_outcome_from_value(&v).expect("round trip");
        match back {
            PollOutcome::Done(r) => {
                assert_eq!(r.exit_code, Some(0));
                assert_eq!(r.output, Some(json!({ "pid": 1234u32 })));
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn run_outcome_failed_serde_round_trip() {
        let outcome = PollOutcome::Failed("Run exited non-zero: code=Some(1)".into());
        let v = run_outcome_to_value(&outcome);
        let back = run_outcome_from_value(&v).expect("round trip");
        match back {
            PollOutcome::Failed(err) => assert!(err.contains("non-zero")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn shell_dispatch_watcher_persists_exit_code_on_success() {
        use tasty_agent::OnFailure;
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let task = Task {
            id: "t-sh-ok".to_string(),
            workspace_id: 1,
            name: "shell-ok".into(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id: 1,
                cwd: None,
            },
            state: tasty_agent::TaskState::Running,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        let outcome = exec.dispatch(&task);
        let handle = match outcome {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        let pid = match handle {
            DispatchHandle::ShellProcess { pid } => pid,
            other => panic!("expected ShellProcess, got {other:?}"),
        };
        let mut final_outcome: Option<PollOutcome> = None;
        for _ in 0..40 {
            match exec.poll(&handle) {
                PollOutcome::Active => std::thread::sleep(Duration::from_millis(50)),
                other => {
                    final_outcome = Some(other);
                    break;
                }
            }
        }
        let outcome = final_outcome.expect("watcher should have completed");
        match outcome {
            PollOutcome::Done(r) => assert_eq!(r.exit_code, Some(0)),
            other => panic!("expected Done, got {other:?}"),
        }
        let loaded = load_run_result(&ctx, 1, &task.id).expect("persisted");
        match loaded {
            PollOutcome::Done(r) => assert_eq!(r.exit_code, Some(0)),
            other => panic!("expected persisted Done, got {other:?}"),
        }
        assert!(!exec.shell_children.contains_key(&pid));
    }

    /// poison 뒤 결과 조회를 검사한다. watcher 쓰기의 실행 타이밍을 재현하는 시험은 아니다.
    #[test]
    fn a_poisoned_run_result_cell_still_delivers_the_outcome() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);

        let cell = Arc::new(Mutex::new(Some(PollOutcome::Done(TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        }))));
        let poisoner = cell.clone();
        let _ = thread::spawn(move || {
            let _guard = poisoner.lock().expect("fresh lock");
            panic!("poison the run result cell on purpose");
        })
        .join();
        assert!(
            cell.is_poisoned(),
            "락이 실제로 poison 됐어야 전제가 성립한다"
        );

        let pid = 424242;
        exec.shell_children.insert(
            pid,
            ShellChildEntry {
                result: cell,
                _watcher: thread::spawn(|| {}),
            },
        );

        let handle = DispatchHandle::ShellProcess { pid };
        match exec.poll(&handle) {
            PollOutcome::Done(r) => assert_eq!(r.exit_code, Some(0)),
            other => panic!("poison 된 cell 에서도 결과를 꺼내야 한다, got {other:?}"),
        }
        assert!(!exec.shell_children.contains_key(&pid));
    }

    /// 값만 만드는 공용 task 빌더. 실제 프로세스 실행 시험에는 별도 Unix 조건을 둔다.
    fn mk_run_task(id: &str, command: Vec<&str>) -> Task {
        use tasty_agent::OnFailure;
        Task {
            id: id.to_string(),
            workspace_id: 1,
            name: id.to_string(),
            command: TaskCommand::Run {
                command: command.into_iter().map(str::to_string).collect(),
                workspace_id: 1,
                cwd: None,
            },
            state: tasty_agent::TaskState::Running,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        }
    }

    /// 지정한 횟수만 폴링한다. sleep 합 외에 poll 자체도 기다릴 수 있어 실제 시간 상한은 아니다.
    #[cfg(unix)]
    fn poll_until_terminal(
        exec: &mut HostExecutor,
        handle: &DispatchHandle,
        max_ticks: u32,
    ) -> PollOutcome {
        for _ in 0..max_ticks {
            match exec.poll(handle) {
                PollOutcome::Active => std::thread::sleep(Duration::from_millis(50)),
                other => return other,
            }
        }
        panic!(
            "task did not reach a terminal state; nominal polling sleep budget: {}ms",
            max_ticks * 50
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_dispatch_captures_stdout_in_output() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let task = mk_run_task("t-sh-capture", vec!["sh", "-c", "echo hello; exit 0"]);
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match poll_until_terminal(&mut exec, &handle, 40) {
            PollOutcome::Done(r) => {
                assert_eq!(r.exit_code, Some(0));
                let stdout_text = r
                    .output
                    .as_ref()
                    .and_then(|o| o.get("stdout"))
                    .and_then(|s| s.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                assert!(
                    stdout_text.contains("hello"),
                    "expected stdout text to contain 'hello', got {stdout_text:?}"
                );
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn shell_dispatch_nonzero_exit_fails_with_exit_code_in_error() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let task = mk_run_task("t-sh-fail", vec!["sh", "-c", "exit 3"]);
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match poll_until_terminal(&mut exec, &handle, 40) {
            PollOutcome::Failed(err) => assert!(
                err.contains("Some(3)"),
                "expected error to mention exit code 3, got {err}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// 파이프 용량을 넘는 stdout을 계속 읽어 자식과 wait가 서로 막히지 않는지 확인한다.
    #[cfg(unix)]
    #[test]
    fn shell_dispatch_large_stdout_does_not_deadlock() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let task = mk_run_task("t-sh-bigout", vec!["sh", "-c", "yes | head -c 2000000"]);
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match poll_until_terminal(&mut exec, &handle, 200) {
            PollOutcome::Done(r) => {
                assert_eq!(r.exit_code, Some(0));
                let stdout = r.output.as_ref().and_then(|o| o.get("stdout")).cloned();
                let truncated = stdout
                    .as_ref()
                    .and_then(|s| s.get("truncated"))
                    .and_then(|t| t.as_bool())
                    .unwrap_or(false);
                assert!(truncated, "2MB stdout should exceed the 64KiB tail cap");
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    /// stderr만 차도 자식이 멈출 수 있어 별도로 검사한다.
    #[cfg(unix)]
    #[test]
    fn shell_dispatch_large_stderr_does_not_deadlock() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let task = mk_run_task(
            "t-sh-bigerr",
            vec!["sh", "-c", "yes | head -c 2000000 1>&2"],
        );
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match poll_until_terminal(&mut exec, &handle, 200) {
            PollOutcome::Done(r) => {
                assert_eq!(r.exit_code, Some(0));
                let stderr = r.output.as_ref().and_then(|o| o.get("stderr")).cloned();
                let truncated = stderr
                    .as_ref()
                    .and_then(|s| s.get("truncated"))
                    .and_then(|t| t.as_bool())
                    .unwrap_or(false);
                assert!(truncated, "2MB stderr should exceed the 64KiB tail cap");
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn drain_capped_truncates_to_tail_and_tracks_dropped_bytes() {
        let total = CAPTURE_TAIL_CAP + 100;
        let mut input = vec![b'a'; total];
        // 끝부분에 다른 바이트를 넣어 앞부분이 아닌 tail이 남았는지 확인한다.
        for b in input.iter_mut().rev().take(100) {
            *b = b'b';
        }
        let result = drain_capped(std::io::Cursor::new(input));
        assert_eq!(result.data.len(), CAPTURE_TAIL_CAP);
        assert!(result.truncated);
        assert_eq!(result.dropped_bytes, 100);
        assert!(result.data.iter().all(|&b| b == b'b' || b == b'a'));
        assert!(result.data.ends_with(&[b'b'; 100]));
    }

    fn mk_polled(method: &str) -> DispatchHandle {
        DispatchHandle::PolledDispatch {
            workspace_id: 1,
            poll_method: method.to_string(),
            poll_params: json!({}),
            state_field: "state".to_string(),
            terminal_states: vec!["done".to_string()],
            failure_states: vec![],
            interval_ms: 1,
            deadline_ms: None,
        }
    }

    /// 지정한 응답을 요청 순서대로 보내는 모의 IPC. 호출자는 반환한 워커를 join한다.
    fn spawn_fake_injector(
        ctx: &RunnerContext,
        responses: Vec<serde_json::Value>,
    ) -> std::thread::JoinHandle<()> {
        use std::sync::mpsc;
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;

        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        std::thread::spawn(move || {
            for resp in responses {
                let cmd = rx.recv().expect("recv poll request");
                cmd.response_tx
                    .send(JsonRpcResponse::success(
                        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                        resp,
                    ))
                    .expect("send poll response");
            }
        })
    }

    fn mk_polled_with_states(method: &str, terminal: &[&str], failure: &[&str]) -> DispatchHandle {
        DispatchHandle::PolledDispatch {
            workspace_id: 1,
            poll_method: method.to_string(),
            poll_params: json!({}),
            state_field: "state".to_string(),
            terminal_states: terminal.iter().map(|s| s.to_string()).collect(),
            failure_states: failure.iter().map(|s| s.to_string()).collect(),
            interval_ms: 1,
            deadline_ms: None,
        }
    }

    #[test]
    fn polled_dispatch_failure_state_yields_failed_not_done() {
        let (_td, ctx) = fresh_ctx();
        let worker = spawn_fake_injector(&ctx, vec![json!({ "state": "exited", "why": "killed" })]);
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled_with_states("fake.poll", &["idle"], &["exited"]);
        match exec.poll(&handle) {
            PollOutcome::Failed(msg) => {
                assert!(msg.contains("exited"), "{msg}");
                assert!(msg.contains("killed"), "response summary missing: {msg}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        worker.join().unwrap();
    }

    /// 성공·실패 목록이 겹치면 실패를 우선한다.
    #[test]
    fn polled_dispatch_failure_states_take_precedence_over_terminal() {
        let (_td, ctx) = fresh_ctx();
        let worker = spawn_fake_injector(&ctx, vec![json!({ "state": "exited" })]);
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled_with_states("fake.poll", &["idle", "exited"], &["exited"]);
        assert!(matches!(exec.poll(&handle), PollOutcome::Failed(_)));
        worker.join().unwrap();
    }

    #[test]
    fn polled_dispatch_without_failure_states_still_succeeds_on_terminal() {
        let (_td, ctx) = fresh_ctx();
        let worker = spawn_fake_injector(&ctx, vec![json!({ "state": "exited" })]);
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled_with_states("fake.poll", &["idle", "exited"], &[]);
        assert!(matches!(exec.poll(&handle), PollOutcome::Done(_)));
        worker.join().unwrap();
    }

    #[test]
    fn poll_spec_without_failure_states_deserializes() {
        let spec: tasty_agent::PollSpec = serde_json::from_str(
            r#"{"poll_method":"x","state_field":"state","terminal_states":["idle"]}"#,
        )
        .expect("legacy inline PollSpec should still deserialize");
        assert!(spec.failure_states.is_empty());
        assert_eq!(spec.interval_ms, 500);
    }

    /// 긴 응답을 줄일 때 UTF-8 문자 경계를 지켜야 한다.
    #[test]
    fn poll_failure_summary_truncates_on_char_boundary() {
        let big = "가".repeat(POLL_FAILURE_SUMMARY_CAP);
        let summary = summarize_poll_response(&json!({ "state": "exited", "log": big }));
        assert!(summary.ends_with("...(truncated)"), "{}", &summary[..80]);
        assert!(summary.len() <= POLL_FAILURE_SUMMARY_CAP + "...(truncated)".len());
    }

    #[test]
    fn polled_dispatch_poll_injector_uninit_returns_active_within_grace() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled("fake.poll");
        let outcome = exec.poll(&handle);
        assert!(
            matches!(outcome, PollOutcome::Active),
            "expected Active, got {outcome:?}"
        );
        assert!(
            exec.injector_grace_deadline_ms.is_some(),
            "deadline should be set on first uninit"
        );
    }

    #[test]
    fn polled_dispatch_poll_after_grace_expired_returns_failed() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled("fake.poll");
        let _first = exec.poll(&handle);
        debug_assert!(matches!(_first, PollOutcome::Active));
        exec.injector_grace_deadline_ms = Some(0);
        let outcome = exec.poll(&handle);
        match outcome {
            PollOutcome::Failed(err) => {
                assert!(err.contains("injector grace expired"), "got {err}");
                assert!(err.contains("fake.poll"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn polled_dispatch_recovers_after_injector_ready() {
        use std::sync::mpsc;
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let handle = mk_polled("fake.poll");
        let outcome1 = exec.poll(&handle);
        assert!(matches!(outcome1, PollOutcome::Active));
        assert!(exec.injector_grace_deadline_ms.is_some());

        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        let injector = HostIpcInjector::new(tx, waker);
        ctx.host_ipc.set(injector).ok().expect("set once");
        let worker = std::thread::spawn(move || {
            let cmd = rx.recv().expect("recv fake.poll");
            assert_eq!(cmd.request.method, "fake.poll");
            let resp = JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "state": "done" }),
            );
            cmd.response_tx.send(resp).expect("send resp");
        });
        let outcome2 = exec.poll(&handle);
        worker.join().unwrap();
        match outcome2 {
            PollOutcome::Done(_) => {}
            other => panic!("expected Done, got {other:?}"),
        }
        assert!(
            exec.injector_grace_deadline_ms.is_none(),
            "deadline should reset after successful dispatch"
        );
    }

    #[test]
    fn polled_dispatch_non_injector_error_fails_immediately() {
        use std::sync::mpsc;
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let handle = mk_polled("fake.poll");
        // timeout을 실제로 기다리지 않고 수신자를 닫아 다른 종류의 IPC 실패를 만든다.
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        drop(rx);
        let waker = std::sync::Arc::new(|| {});
        let injector = HostIpcInjector::new(tx, waker);
        ctx.host_ipc.set(injector).ok().expect("set once");
        let outcome = exec.poll(&handle);
        match outcome {
            PollOutcome::Failed(err) => {
                assert!(err.contains("fake.poll"), "got {err}");
                assert!(
                    !err.contains("grace expired"),
                    "non-injector error must not use grace path: {err}"
                );
                assert!(
                    !err.contains("injector not initialized"),
                    "should not be uninit path: {err}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(exec.injector_grace_deadline_ms.is_none());
    }

    #[test]
    fn custom_with_poll_maps_params_and_polls_to_done() {
        use std::collections::HashMap;
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpec, PollSpecRef};
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());

        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let start = rx.recv().expect("recv fake.start");
            assert_eq!(start.request.method, "fake.start");
            start
                .response_tx
                .send(JsonRpcResponse::success(
                    start.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "job": "J1" }),
                ))
                .expect("send start resp");

            let poll1 = rx.recv().expect("recv fake.poll 1");
            assert_eq!(poll1.request.method, "fake.poll");
            assert_eq!(poll1.request.params.get("surface_id"), Some(&json!(7)));
            assert_eq!(poll1.request.params.get("job"), Some(&json!("J1")));
            poll1
                .response_tx
                .send(JsonRpcResponse::success(
                    poll1.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "state": "running" }),
                ))
                .expect("send poll1 resp");

            let poll2 = rx.recv().expect("recv fake.poll 2");
            poll2
                .response_tx
                .send(JsonRpcResponse::success(
                    poll2.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "state": "done" }),
                ))
                .expect("send poll2 resp");
        });

        let mut map_from_request = HashMap::new();
        map_from_request.insert("surface_id".to_string(), "surface_id".to_string());
        let mut map_from_response = HashMap::new();
        map_from_response.insert("job".to_string(), "job".to_string());
        let task = Task {
            id: "t-poll".to_string(),
            workspace_id: 1,
            name: "poll".into(),
            command: TaskCommand::Custom {
                ipc_method: "fake.start".into(),
                params: json!({ "surface_id": 7 }),
                poll: Some(PollSpecRef::Inline(PollSpec {
                    poll_method: "fake.poll".into(),
                    map_from_response,
                    map_from_request,
                    state_field: "state".into(),
                    terminal_states: vec!["done".into()],
                    failure_states: vec![],
                    interval_ms: 1,
                    timeout_ms: None,
                })),
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };

        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match &handle {
            DispatchHandle::PolledDispatch {
                poll_method,
                poll_params,
                ..
            } => {
                assert_eq!(poll_method, "fake.poll");
                assert_eq!(poll_params.get("surface_id"), Some(&json!(7)));
                assert_eq!(poll_params.get("job"), Some(&json!("J1")));
            }
            other => panic!("expected PolledDispatch, got {other:?}"),
        }
        assert!(matches!(exec.poll(&handle), PollOutcome::Active));
        match exec.poll(&handle) {
            PollOutcome::Done(r) => {
                assert_eq!(r.output, Some(json!({ "state": "done" })));
            }
            other => panic!("expected Done, got {other:?}"),
        }
        worker.join().unwrap();
    }

    #[test]
    fn custom_without_poll_is_immediate() {
        use std::sync::mpsc;
        use tasty_agent::OnFailure;
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let cmd = rx.recv().expect("recv fake.do");
            cmd.response_tx
                .send(JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "ok": true }),
                ))
                .expect("send resp");
        });
        let task = Task {
            id: "t-imm".to_string(),
            workspace_id: 1,
            name: "imm".into(),
            command: TaskCommand::Custom {
                ipc_method: "fake.do".into(),
                params: json!({}),
                poll: None,
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        worker.join().unwrap();
        match handle {
            DispatchHandle::CustomImmediate(r) => {
                assert_eq!(r.output, Some(json!({ "ok": true })));
            }
            other => panic!("expected CustomImmediate, got {other:?}"),
        }
    }

    #[test]
    fn custom_with_named_poll_strategy_resolves_and_polls() {
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;
        use tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort;

        crate::completion_strategy::HostCompletionStrategyPort
            .install_plugin_completion_strategies(
                "rhtest1",
                &[serde_json::json!({
                    "id": "wait-done",
                    "priority": 100,
                    "spec": {
                        "kind": "poll",
                        "poll_method": "rhtest1.poll",
                        "state_field": "state",
                        "terminal_states": ["done"],
                        "interval_ms": 1,
                    },
                })],
            );

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let start = rx.recv().expect("recv rhtest1.start");
            start
                .response_tx
                .send(JsonRpcResponse::success(
                    start.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({}),
                ))
                .expect("send start resp");
            let poll = rx.recv().expect("recv rhtest1.poll");
            poll.response_tx
                .send(JsonRpcResponse::success(
                    poll.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "state": "done" }),
                ))
                .expect("send poll resp");
        });
        let task = Task {
            id: "t-named".to_string(),
            workspace_id: 1,
            name: "named".into(),
            command: TaskCommand::Custom {
                ipc_method: "rhtest1.start".into(),
                params: json!({}),
                poll: Some(PollSpecRef::Named {
                    strategy: "rhtest1/wait-done".into(),
                }),
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match exec.poll(&handle) {
            PollOutcome::Done(r) => assert_eq!(r.output, Some(json!({ "state": "done" }))),
            other => panic!("expected Done, got {other:?}"),
        }
        worker.join().unwrap();
        crate::completion_strategy::HostCompletionStrategyPort.uninstall_plugin("rhtest1");
    }

    #[test]
    fn custom_with_named_push_strategy_registers_hook_and_awaits_external() {
        use crate::hook_handler::types::{
            HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
        };
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;
        use tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort;

        crate::hook_handler::global()
            .upsert_full_handler(HookHandler {
                id: HookHandlerId::new("rhtest-push/notify"),
                source: HookSource::Hook,
                priority: 100,
                owner: HookHandlerOwner::Plugin("rhtest-push".into()),
                action: HookHandlerAction::IpcSequence { calls: vec![] },
                display_name_i18n_key: None,
                disabled: false,
            })
            .expect("test hook handler upsert");

        crate::completion_strategy::HostCompletionStrategyPort
            .install_plugin_completion_strategies(
                "rhtest-push",
                &[serde_json::json!({
                    "id": "wait-done",
                    "priority": 100,
                    "spec": {
                        "kind": "push",
                        "notify_via": "rhtest-push/notify",
                        "timeout_ms": 60000,
                    },
                })],
            );

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let start = rx.recv().expect("recv rhtest-push.start");
            start
                .response_tx
                .send(JsonRpcResponse::success(
                    start.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({}),
                ))
                .expect("send start resp");
            let hook_set = rx.recv().expect("recv hook.set");
            assert_eq!(hook_set.request.method, "hook.set");
            assert_eq!(hook_set.request.params.get("surface_id"), Some(&json!(7)));
            assert_eq!(
                hook_set.request.params.get("event"),
                Some(&json!("command-completed"))
            );
            assert_eq!(
                hook_set.request.params.get("handler"),
                Some(&json!("rhtest-push/notify"))
            );
            assert_eq!(hook_set.request.params.get("once"), Some(&json!(true)));
            hook_set
                .response_tx
                .send(JsonRpcResponse::success(
                    hook_set
                        .request
                        .id
                        .clone()
                        .unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "hook_id": 999 }),
                ))
                .expect("send hook.set resp");
        });
        let task = Task {
            id: "t-push".to_string(),
            workspace_id: 1,
            name: "push".into(),
            command: TaskCommand::Custom {
                ipc_method: "rhtest-push.start".into(),
                params: json!({ "surface_id": 7 }),
                poll: Some(PollSpecRef::Named {
                    strategy: "rhtest-push/wait-done".into(),
                }),
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        worker.join().unwrap();
        match &handle {
            DispatchHandle::AwaitExternal {
                wait_key,
                deadline_ms,
            } => {
                assert_eq!(wait_key, "999");
                assert!(
                    *deadline_ms > 0,
                    "deadline must be populated from timeout_ms"
                );
            }
            other => panic!("expected AwaitExternal, got {other:?}"),
        }
        assert!(matches!(exec.poll(&handle), PollOutcome::Active));
        assert_eq!(
            ctx.hook_task_waits.resolve(999),
            Some((1, "t-push".to_string()))
        );

        crate::completion_strategy::HostCompletionStrategyPort.uninstall_plugin("rhtest-push");
    }

    #[test]
    fn custom_with_push_strategy_missing_surface_id_fails_dispatch() {
        use crate::hook_handler::types::{
            HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
        };
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;
        use tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort;

        crate::hook_handler::global()
            .upsert_full_handler(HookHandler {
                id: HookHandlerId::new("rhtest-push2/notify"),
                source: HookSource::Hook,
                priority: 100,
                owner: HookHandlerOwner::Plugin("rhtest-push2".into()),
                action: HookHandlerAction::IpcSequence { calls: vec![] },
                display_name_i18n_key: None,
                disabled: false,
            })
            .expect("test hook handler upsert");

        crate::completion_strategy::HostCompletionStrategyPort
            .install_plugin_completion_strategies(
                "rhtest-push2",
                &[serde_json::json!({
                    "id": "wait-done",
                    "priority": 100,
                    "spec": {
                        "kind": "push",
                        "notify_via": "rhtest-push2/notify",
                        "timeout_ms": 60000,
                    },
                })],
            );

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let start = rx.recv().expect("recv rhtest-push2.start");
            start
                .response_tx
                .send(JsonRpcResponse::success(
                    start.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({}),
                ))
                .expect("send start resp");
        });
        let task = Task {
            id: "t-push-no-surface".to_string(),
            workspace_id: 1,
            name: "push-no-surface".into(),
            command: TaskCommand::Custom {
                ipc_method: "rhtest-push2.start".into(),
                params: json!({}),
                poll: Some(PollSpecRef::Named {
                    strategy: "rhtest-push2/wait-done".into(),
                }),
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        match exec.dispatch(&task) {
            DispatchOutcome::PermanentFail(e) => assert!(e.contains("surface_id")),
            other => panic!("expected PermanentFail, got {other:?}"),
        }
        worker.join().unwrap();
        crate::completion_strategy::HostCompletionStrategyPort.uninstall_plugin("rhtest-push2");
    }

    /// 미등록 전략 오류 전에 원래 IPC는 이미 실행된다. 응답 뒤 전략 해석에서 실패하는 순서를 확인한다.
    #[test]
    fn custom_with_unknown_named_poll_strategy_fails_dispatch() {
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let start = rx.recv().expect("recv rhtest2.start");
            start
                .response_tx
                .send(JsonRpcResponse::success(
                    start.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({}),
                ))
                .expect("send start resp");
        });
        let task = Task {
            id: "t-named-missing".to_string(),
            workspace_id: 1,
            name: "named-missing".into(),
            command: TaskCommand::Custom {
                ipc_method: "rhtest2.start".into(),
                params: json!({}),
                poll: Some(PollSpecRef::Named {
                    strategy: "rhtest2/does-not-exist".into(),
                }),
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        match exec.dispatch(&task) {
            DispatchOutcome::PermanentFail(e) => assert!(e.contains("rhtest2/does-not-exist")),
            other => panic!("expected PermanentFail, got {other:?}"),
        }
        worker.join().unwrap();
    }

    #[test]
    fn custom_without_poll_uses_default_for_method_strategy() {
        use std::sync::mpsc;
        use tasty_agent::OnFailure;
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;
        use tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort;

        crate::completion_strategy::HostCompletionStrategyPort
            .install_plugin_completion_strategies(
                "rhtest3",
                &[serde_json::json!({
                    "id": "auto-wait",
                    "priority": 100,
                    "default_for_methods": ["rhtest3.start"],
                    "spec": {
                        "kind": "poll",
                        "poll_method": "rhtest3.poll",
                        "state_field": "state",
                        "terminal_states": ["done"],
                        "interval_ms": 1,
                    },
                })],
            );

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let start = rx.recv().expect("recv rhtest3.start");
            start
                .response_tx
                .send(JsonRpcResponse::success(
                    start.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({}),
                ))
                .expect("send start resp");
            let poll = rx.recv().expect("recv rhtest3.poll");
            poll.response_tx
                .send(JsonRpcResponse::success(
                    poll.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({ "state": "done" }),
                ))
                .expect("send poll resp");
        });
        let task = Task {
            id: "t-default".to_string(),
            workspace_id: 1,
            name: "default".into(),
            command: TaskCommand::Custom {
                ipc_method: "rhtest3.start".into(),
                params: json!({}),
                poll: None,
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
        };
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        assert!(matches!(handle, DispatchHandle::PolledDispatch { .. }));
        match exec.poll(&handle) {
            PollOutcome::Done(r) => assert_eq!(r.output, Some(json!({ "state": "done" }))),
            other => panic!("expected Done, got {other:?}"),
        }
        worker.join().unwrap();
        crate::completion_strategy::HostCompletionStrategyPort.uninstall_plugin("rhtest3");
    }

    #[test]
    fn evict_handle_removes_from_store() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let task_id = "t-evict".to_string();
        let handle = mk_polled("fake.poll");
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

    #[cfg(unix)]
    #[test]
    fn try_acquire_lease_pool_fixed_distributes_distinct_resources_and_defers() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let candidates = vec!["wt-1".to_string(), "wt-2".to_string(), "wt-3".to_string()];

        let mut got = Vec::new();
        for i in 0..3 {
            let mut task = mk_run_task(&format!("t-pool-{i}"), vec!["true"]);
            task.metadata = json!({ "lease": { "candidates": candidates } });
            assert_eq!(
                exec.try_acquire_lease(&task).unwrap(),
                Some(true),
                "task {i} should acquire a distinct slot"
            );
            got.push(exec.held_leases.get(&task.id).unwrap().1.clone());
        }
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            3,
            "expected 3 distinct resources, got {got:?}"
        );

        let mut task4 = mk_run_task("t-pool-3", vec!["true"]);
        task4.metadata = json!({ "lease": { "candidates": candidates } });
        assert_eq!(exec.try_acquire_lease(&task4).unwrap(), Some(false));
        assert!(!exec.held_leases.contains_key(&task4.id));

        for (resource, _holder) in exec.held_leases.values().map(|(_, r, h)| (r, h)) {
            assert!(
                candidates.contains(resource),
                "fixed mode must never synthesize, got {resource}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn try_acquire_lease_pool_elastic_unbounded_synthesizes_beyond_candidates() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let candidates = vec!["wt-1".to_string(), "wt-2".to_string(), "wt-3".to_string()];

        let mut got = Vec::new();
        for i in 0..5 {
            let mut task = mk_run_task(&format!("t-elastic-{i}"), vec!["true"]);
            task.metadata = json!({ "lease": { "candidates": candidates, "elastic": {} } });
            assert_eq!(
                exec.try_acquire_lease(&task).unwrap(),
                Some(true),
                "task {i} must not wait under unbounded elastic"
            );
            got.push(exec.held_leases.get(&task.id).unwrap().1.clone());
        }
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            5,
            "expected 5 distinct resources, got {got:?}"
        );
        let synthesized_count = got.iter().filter(|r| !candidates.contains(r)).count();
        assert_eq!(synthesized_count, 2, "3 fixed + 2 synthesized");
    }

    #[cfg(unix)]
    #[test]
    fn try_acquire_lease_pool_elastic_max_candidates_caps_synthesis() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let candidates = vec!["wt-1".to_string(), "wt-2".to_string(), "wt-3".to_string()];

        let mut acquired = 0;
        for i in 0..6 {
            let mut task = mk_run_task(&format!("t-cap-{i}"), vec!["true"]);
            task.metadata = json!({
                "lease": { "candidates": candidates, "elastic": { "max_candidates": 4 } }
            });
            if exec.try_acquire_lease(&task).unwrap() == Some(true) {
                acquired += 1;
            }
        }
        assert_eq!(acquired, 4);
    }

    #[cfg(unix)]
    #[test]
    fn try_acquire_lease_resource_sugar_conflicts_with_single_candidate_pool() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);

        let mut task_a = mk_run_task("t-sugar-a", vec!["true"]);
        task_a.metadata = json!({ "lease": { "resource": "single-path" } });
        assert_eq!(exec.try_acquire_lease(&task_a).unwrap(), Some(true));

        let mut task_b = mk_run_task("t-sugar-b", vec!["true"]);
        task_b.metadata = json!({ "lease": { "candidates": ["single-path"] } });
        assert_eq!(
            exec.try_acquire_lease(&task_b).unwrap(),
            Some(false),
            "candidates:['single-path'] must conflict with resource:'single-path' \
             on the same lease key"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dispatch_persists_lease_substituted_cwd_for_task_get() {
        use tasty_agent::TaskStore;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap().to_string();
        // macOS 임시 경로는 symlink를 포함할 수 있어 pwd의 결과와 비교할 때 canonicalize한다.
        let resolved_path = dir
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let mut task = mk_run_task("t-persist-cwd", vec!["pwd"]);
        task.metadata = json!({ "lease": { "resource": dir_path } });
        ctx.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref());
            store.put(&task).unwrap();
        });

        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };

        let stored = ctx.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref());
            store.get(1, &task.id).unwrap().unwrap()
        });
        match stored.command {
            TaskCommand::Run { cwd, .. } => {
                assert_eq!(
                    cwd,
                    Some(std::path::PathBuf::from(&dir_path)),
                    "stored task.command.cwd should reflect the acquired lease resource"
                );
            }
            other => panic!("expected Run, got {other:?}"),
        }

        match poll_until_terminal(&mut exec, &handle, 40) {
            PollOutcome::Done(r) => {
                let stdout_text = r
                    .output
                    .as_ref()
                    .and_then(|o| o.get("stdout"))
                    .and_then(|s| s.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                assert_eq!(stdout_text.trim(), resolved_path);
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    fn outputs_of(pairs: &[(&str, serde_json::Value)]) -> HashMap<TaskId, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    fn custom_params(params: serde_json::Value) -> TaskCommand {
        TaskCommand::Custom {
            ipc_method: "claude.tell".to_string(),
            params,
            poll: None,
        }
    }

    #[test]
    fn task_output_substitution_preserves_number_type() {
        let outputs = outputs_of(&[("t-a", json!({ "child_surface_id": 731 }))]);
        let mut cmd = custom_params(json!({
            "surface_id": "${task.t-a.output/child_surface_id}",
            "message": "hi",
        }));
        assert!(substitute_task_outputs(&mut cmd, &outputs).unwrap());

        let TaskCommand::Custom { params, .. } = &cmd else {
            panic!("expected Custom");
        };
        assert_eq!(params["surface_id"], json!(731));
        assert_ne!(
            params["surface_id"],
            json!("731"),
            "숫자 결과가 문자열로 바뀌었다"
        );
        assert!(
            params["surface_id"].as_u64().is_some(),
            "치환한 surface_id는 u64로 읽을 수 있어야 한다"
        );
        assert_eq!(
            params["message"],
            json!("hi"),
            "참조가 없는 값은 바뀌면 안 된다"
        );
    }

    #[test]
    fn task_output_substitution_preserves_composite_types() {
        let outputs = outputs_of(&[(
            "t-a",
            json!({ "obj": {"k": 1}, "arr": [1, 2], "flag": true }),
        )]);
        let mut cmd = custom_params(json!({
            "o": "${task.t-a.output/obj}",
            "a": "${task.t-a.output/arr}",
            "b": "${task.t-a.output/flag}",
            "whole": "${task.t-a.output}",
        }));
        substitute_task_outputs(&mut cmd, &outputs).unwrap();

        let TaskCommand::Custom { params, .. } = &cmd else {
            panic!("expected Custom");
        };
        assert_eq!(params["o"], json!({"k": 1}));
        assert_eq!(params["a"], json!([1, 2]));
        assert_eq!(params["b"], json!(true));
        assert_eq!(
            params["whole"],
            json!({ "obj": {"k": 1}, "arr": [1, 2], "flag": true })
        );
    }

    #[test]
    fn task_output_substitution_interpolates_within_string() {
        let outputs = outputs_of(&[("t-a", json!({ "pr_number": 42, "who": "zilhak" }))]);
        let mut cmd = custom_params(json!({
            "prompt": "review PR ${task.t-a.output/pr_number}",
            "two": "${task.t-a.output/who} opened #${task.t-a.output/pr_number}",
        }));
        substitute_task_outputs(&mut cmd, &outputs).unwrap();

        let TaskCommand::Custom { params, .. } = &cmd else {
            panic!("expected Custom");
        };
        assert_eq!(params["prompt"], json!("review PR 42"));
        assert_eq!(params["two"], json!("zilhak opened #42"));
    }

    #[test]
    fn task_output_substitution_reaches_nested_json_and_run_args() {
        let outputs = outputs_of(&[("t-a", json!({ "id": 7, "dir": "build" }))]);
        let mut custom = custom_params(json!({ "nested": [{ "deep": "${task.t-a.output/id}" }] }));
        substitute_task_outputs(&mut custom, &outputs).unwrap();
        let TaskCommand::Custom { params, .. } = &custom else {
            panic!("expected Custom");
        };
        assert_eq!(params["nested"][0]["deep"], json!(7));

        let mut run = TaskCommand::Run {
            command: vec!["echo".into(), "id=${task.t-a.output/id}".into()],
            workspace_id: 1,
            cwd: Some("/tmp/${task.t-a.output/dir}".into()),
        };
        substitute_task_outputs(&mut run, &outputs).unwrap();
        let TaskCommand::Run { command, cwd, .. } = &run else {
            panic!("expected Run");
        };
        assert_eq!(command[1], "id=7");
        assert_eq!(cwd.as_deref(), Some(std::path::Path::new("/tmp/build")));
    }

    #[test]
    fn task_output_substitution_rejects_pointer_miss() {
        let outputs = outputs_of(&[("t-a", json!({ "child_surface_id": 731 }))]);
        let mut cmd = custom_params(json!({ "x": "${task.t-a.output/nope}" }));
        let err = substitute_task_outputs(&mut cmd, &outputs).unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }

    /// 결과에 포함된 다른 표식을 재치환하지 않는다.
    #[test]
    fn task_output_substitution_does_not_rescan_injected_values() {
        let outputs = outputs_of(&[("t-a", json!({ "text": "${task.t-b.output/x}" }))]);
        let mut cmd = custom_params(json!({ "p": "${task.t-a.output/text}" }));
        substitute_task_outputs(&mut cmd, &outputs).unwrap();
        let TaskCommand::Custom { params, .. } = &cmd else {
            panic!("expected Custom");
        };
        assert_eq!(params["p"], json!("${task.t-b.output/x}"));
    }

    #[test]
    fn task_output_substitution_missing_result_is_permanent_fail() {
        use tasty_agent::TaskStore;
        let (_td, ctx) = fresh_ctx();
        let seq = ctx.agent_seq.clone();
        let mut upstream = mk_run_task("t-up", vec!["true"]);
        upstream.result = None;
        ctx.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.put(&upstream).unwrap();
        });

        let mut exec = HostExecutor::new(ctx);
        let mut task = mk_run_task("t-down", vec!["true"]);
        task.command =
            custom_params(json!({ "surface_id": "${task.t-up.output/child_surface_id}" }));
        task.depends_on = vec!["t-up".to_string()];

        match exec.dispatch(&task) {
            DispatchOutcome::PermanentFail(e) => {
                assert!(e.contains("task output substitution"), "{e}");
                assert!(e.contains("no result output"), "{e}");
            }
            other => panic!("expected PermanentFail, got {other:?}"),
        }
    }

    #[test]
    fn task_output_substitution_unknown_task_is_permanent_fail() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let mut task = mk_run_task("t-down", vec!["true"]);
        task.command = custom_params(json!({ "x": "${task.t-ghost.output/y}" }));

        match exec.dispatch(&task) {
            DispatchOutcome::PermanentFail(e) => assert!(e.contains("task not found"), "{e}"),
            other => panic!("expected PermanentFail, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn dispatch_persists_task_output_substituted_command() {
        use tasty_agent::TaskStore;
        let (_td, ctx) = fresh_ctx();
        let seq = ctx.agent_seq.clone();
        let mut upstream = mk_run_task("t-up", vec!["true"]);
        upstream.result = Some(TaskResult {
            exit_code: Some(0),
            output: Some(json!({ "child_surface_id": 731 })),
            error: None,
        });
        let mut downstream = mk_run_task(
            "t-down",
            vec!["echo", "${task.t-up.output/child_surface_id}"],
        );
        downstream.depends_on = vec!["t-up".to_string()];
        ctx.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.put(&upstream).unwrap();
            store.put(&downstream).unwrap();
        });

        let mut exec = HostExecutor::new(ctx.clone());
        match exec.dispatch(&downstream) {
            DispatchOutcome::Started(_) => {}
            other => panic!("expected Started, got {other:?}"),
        }

        let stored = ctx.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(1, &"t-down".to_string()).unwrap().unwrap()
        });
        match stored.command {
            TaskCommand::Run { command, .. } => assert_eq!(command[1], "731"),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn dispatch_fills_empty_run_cwd_with_acquired_lease_resource() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap().to_string();
        // pwd는 symlink를 해석한 경로를 출력한다.
        let resolved_path = dir
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let mut task = mk_run_task("t-cwd-fill", vec!["pwd"]);
        task.metadata = json!({ "lease": { "resource": dir_path } });

        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match poll_until_terminal(&mut exec, &handle, 40) {
            PollOutcome::Done(r) => {
                let stdout_text = r
                    .output
                    .as_ref()
                    .and_then(|o| o.get("stdout"))
                    .and_then(|s| s.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                assert_eq!(
                    stdout_text.trim(),
                    resolved_path,
                    "Run.cwd should have been filled with the acquired lease resource"
                );
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn dispatch_substitutes_lease_placeholder_in_cwd_and_command_args() {
        use tasty_agent::OnFailure;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap().to_string();

        let task = Task {
            id: "t-placeholder".to_string(),
            workspace_id: 1,
            name: "placeholder".into(),
            command: TaskCommand::Run {
                command: vec!["echo".into(), "${lease.resource}".into()],
                workspace_id: 1,
                cwd: Some(std::path::PathBuf::from("${lease.resource}")),
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: json!({ "lease": { "resource": dir_path } }),
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
            reserved_for_fallback: false,
        };

        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        match poll_until_terminal(&mut exec, &handle, 40) {
            PollOutcome::Done(r) => {
                let stdout_text = r
                    .output
                    .as_ref()
                    .and_then(|o| o.get("stdout"))
                    .and_then(|s| s.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                assert_eq!(
                    stdout_text.trim(),
                    dir_path,
                    "command arg placeholder should also have been substituted"
                );
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_substitutes_lease_placeholder_in_custom_params() {
        use std::sync::mpsc;
        use tasty_agent::OnFailure;
        use tasty_ipc::host_call::HostIpcInjector;
        use tasty_ipc::protocol::JsonRpcResponse;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        let worker = std::thread::spawn(move || {
            let cmd = rx.recv().expect("recv fake.spawn");
            assert_eq!(cmd.request.params.get("cwd"), Some(&json!("wt-1")));
            cmd.response_tx
                .send(JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    json!({ "ok": true }),
                ))
                .expect("send resp");
        });
        let task = Task {
            id: "t-custom-lease".to_string(),
            workspace_id: 1,
            name: "custom".into(),
            command: TaskCommand::Custom {
                ipc_method: "fake.spawn".into(),
                params: json!({ "cwd": "${lease.resource}" }),
                poll: None,
            },
            state: tasty_agent::TaskState::Ready,
            depends_on: vec![],
            on_failure: OnFailure::Abort,
            metadata: json!({ "lease": { "candidates": ["wt-1"] } }),
            result: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
            reserved_for_fallback: false,
        };
        let handle = match exec.dispatch(&task) {
            DispatchOutcome::Started(h) => h,
            other => panic!("expected Started, got {other:?}"),
        };
        worker.join().unwrap();
        assert!(matches!(handle, DispatchHandle::CustomImmediate(_)));
    }

    #[cfg(unix)]
    #[test]
    fn release_permit_returns_pool_resource_for_next_holder() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let candidates = vec!["wt-1".to_string()];

        let mut task_a = mk_run_task("t-rel-a", vec!["true"]);
        task_a.metadata = json!({ "lease": { "candidates": candidates } });
        assert_eq!(exec.try_acquire_lease(&task_a).unwrap(), Some(true));

        let mut task_b = mk_run_task("t-rel-b", vec!["true"]);
        task_b.metadata = json!({ "lease": { "candidates": candidates } });
        assert_eq!(exec.try_acquire_lease(&task_b).unwrap(), Some(false));

        exec.release_permit(&task_a.id);
        assert!(!exec.held_leases.contains_key(&task_a.id));

        assert_eq!(exec.try_acquire_lease(&task_b).unwrap(), Some(true));
        assert_eq!(
            exec.held_leases.get(&task_b.id).unwrap().1,
            "wt-1".to_string()
        );
    }
}
