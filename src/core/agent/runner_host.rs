//! Host-side `TaskExecutor` 구현 — runner thread 가 사용.
//!
//! - `Custom { poll: None }` → IPC 동기 호출, 응답으로 즉시 종결.
//! - `Custom { poll: Some(..) }` → dispatch 후 `poll_method` 를 terminal 상태 도달까지
//!   반복 호출하는 범용 폴링 (코어가 모르는 임의 비동기 작업).
//! - `Reduce` → 즉시 collect + `reduce_with_custom`.
//! - `Run` → shell process spawn + watcher.

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

/// DispatchHandle 영속 key prefix (workspace scope). S2: `RunnerLoop.running` 의
/// in-memory map 을 호스트 재시작 사이에 복원하기 위한 mirror. Immediate*/ImmediateFail
/// 은 영속 대상 아님 (다음 tick poll 에서 즉시 흡수).
pub const HANDLE_KEY_PREFIX: &str = "tasty.agent.handle.";

pub fn handle_key(task_id: &str) -> String {
    format!("{HANDLE_KEY_PREFIX}{task_id}")
}

/// 영속된 handle 을 읽기 전용으로 조회한다(mutate 없음) — IPC 조회
/// (`task_get`) 가 "이 task 가 어떤 외부 신호를 기다리는 중인지"(`AwaitExternal`
/// 이면 `wait_key`/`deadline_ms`)를 노출할 때 쓴다. 결정 5 — `hook_wait` 자체는
/// 워크스페이스 무관 in-memory 매핑이라 조회 표면이 없지만, 그 매핑을 만든
/// `AwaitExternal` handle 은 영속되므로 이 경로로 조회 가능하다.
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

/// K.A-1: ShellProcess Run task 의 정확한 종료 결과 영속 key prefix.
/// watcher thread 가 자식 `wait()` 종료 직후 기록 → 호스트가 재시작돼도 다음 reload
/// 단계가 exit_code 까지 정확히 마감할 수 있다 (단, watcher 가 기록을 마치기 전에
/// 호스트가 죽으면 손실 — cross-platform 으로 회피 불가).
pub const RUN_RESULT_KEY_PREFIX: &str = "tasty.agent.run_result.";

pub fn run_result_key(task_id: &str) -> String {
    format!("{RUN_RESULT_KEY_PREFIX}{task_id}")
}

use crate::adapters::ipc::handler::agent::task::run_custom_shell;
use crate::core::agent::task_output_ref;
use crate::ipc::host_call::HostIpcInjector;

/// Host→plugin dispatch timeout — 자식 프로세스 생성/디스크 I/O 까지 포함할 수 있는
/// dispatch 와 1tick 만인 poll 양쪽에 같은 값으로 통일.
const HOST_DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

/// K.A-2: host IPC injector 미초기화 시 dispatch_plugin 이 반환하는 정적 메시지.
/// poll 분기가 사유 분류용으로 매칭. 메시지 변경 시 grace 가드가 동작하지 않으니
/// `dispatch_plugin` 과 동일 상수를 참조 (R-6 회피).
pub(crate) const INJECTOR_UNINIT_MSG: &str = "host IPC injector not initialized";

/// K.A-2: 첫 dispatch_plugin 실패 (injector 미초기화) 시점부터 grace 마감 시각까지의
/// 허용 시간. reload 직후 첫 tick 가 injector init 보다 먼저 도달해도 30 s 안에서는
/// task 를 Failed 로 떨어뜨리지 않고 Active 로 흡수.
pub(crate) const INJECTOR_GRACE_MS: u64 = 30_000;

/// K.A-2: 에러 메시지가 injector 미초기화 사유인지 분류 (`dispatch_plugin` 의
/// 정적 prefix 매칭). poll method 명 prefix 가 붙은 형태도 함께 흡수.
pub(crate) fn is_injector_not_initialized(msg: &str) -> bool {
    msg.contains(INJECTOR_UNINIT_MSG)
}

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
    /// hook_id → task_id 대기 매핑. push-kind `Custom`
    /// dispatch 가 여기 `register` 해 `AwaitExternal` 로 전이하고,
    /// `runner_thread.rs::expire_overdue_hook_waits` 가 timeout 안전망으로
    /// `sweep_expired` 를 돈다. `task_waker_hub` 와 동일 사유로 `Core` 를 거치지
    /// 않고 이 `Arc` 를 직접 공유(runner thread 는 main thread 소유 `Core`/
    /// `CoreState` 에 접근 불가).
    pub hook_task_waits: Arc<crate::core::agent::hook_wait::HookTaskWaits>,
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
            .ok_or_else(|| INJECTOR_UNINIT_MSG.to_string())?;
        inj.dispatch(method, params, HOST_DISPATCH_TIMEOUT)
    }
}

/// K.A-1: ShellProcess Run task 의 watcher 와 결과 cell.
///
/// dispatch 가 자식을 spawn 한 직후 watcher thread 를 띄워 `child.wait()` 호출 →
/// 종료 status 를 `result` cell + 영속 (run_result_key) 양쪽에 기록한다. poll path
/// 는 `try_wait()` 대신 cell 만 조회하므로, host 가 살아 있는 한 정확한 exit_code
/// 회수가 보장된다. cancel 시 watcher 는 *자연 종료까지 detach* — 별 phase 에서
/// `child.kill()` 추가 검토 (현재는 기존 동작 유지).
struct ShellChildEntry {
    /// watcher 가 자식 종료 후 채움. poll 이 take() 로 가져가면 entry 제거.
    result: Arc<Mutex<Option<PollOutcome>>>,
    /// watcher thread 핸들 — 누수 추적용. host 종료 시 detach.
    /// 자식이 살아 있는 한 watcher 도 wait() 에 block, 자식이 죽으면 자연 종료.
    _watcher: thread::JoinHandle<()>,
}

pub struct HostExecutor {
    ctx: RunnerContext,
    /// Run task 의 watcher + 결과 cell — pid → entry. DispatchHandle 은 Clone 필요
    /// (RunnerLoop 가 핸들을 복제), Child 객체 자체는 watcher thread 가 소유.
    shell_children: HashMap<u32, ShellChildEntry>,
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
    /// K.A-2: `PolledDispatch` poll 이 injector 미초기화로 실패하기 시작한 시각 +
    /// `INJECTOR_GRACE_MS`. 한 executor (= 한 workspace runner) 가 모든 PolledDispatch
    /// 핸들에 공유 — reload 직후 injector init 까지의 window 를 흡수. injector 가
    /// ready 가 되어 정상 dispatch 가 1회라도 성공하면 None 으로 reset.
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
    ///
    /// metadata 형식(단일 resource — sugar): `{ resource: String, holder?,
    /// ttl_ms?, mode? }`. `resource: "x"` 는 `candidates: ["x"]` 와 store 상에서
    /// 동일한 `lease_key` 를 쓰므로 관측적으로 동일하다.
    ///
    /// metadata 형식(pool): `{ candidates: [String], holder?, ttl_ms?, mode?,
    /// elastic?: { max_candidates?: u32, overflow_prefix?: String } }`.
    /// `elastic` 생략 시 기본 **fixed** — candidates 안에서만 순회하고 전부
    /// 점유 중이면 대기/실패한다. `elastic` 이 있으면(빈 객체 `{}` 도 포함)
    /// 소진 시 새 candidate 이름을 자동 합성한다(명시적 opt-in — 문서
    /// `crates/tasty-agent/src/lease.rs` 모듈 주석 참조).
    ///
    /// 반환: `Ok(None)` = 미사용, `Ok(Some(true))` = 점유(실제 받은 자원은
    /// `self.held_leases` 에 기록 — `dispatch` 가 이걸로 `TaskCommand` 치환),
    /// `Ok(Some(false))` = 충돌/소진 (Block 모드), `Err(msg)` = Fail 모드
    /// 충돌/소진 또는 store 오류.
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
            // Block 가 dispatch 컨벤션 (semaphore 와 일관: 부족 → Deferred → 다음 tick).
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
        // K.A-1: ShellProcess 의 영속 run_result 도 함께 정리. Non-Shell variant 도
        // 같은 key 가 없을 뿐이라 delete 호출 자체는 idempotent (none-found → no-op).
        evict_run_result(&self.ctx, ws, task_id);
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

    /// command 가 참조하는 upstream task 들의 `result.output` 을 모은다.
    ///
    /// `Reduce` dispatch 와 **같은 방식**이다 — memory lock 안에서는 조회만 하고
    /// (`with_memory` + `TaskStore::get` + `result.output`), 치환 자체는 lock
    /// 바깥에서 한다. `Core::memory` 는 프로세스 전역 `Mutex` 라 lock 안에서 일을
    /// 오래 붙들면 러너 전체가 직렬화된다.
    ///
    /// 참조가 없으면 store 를 아예 건드리지 않는다(가장 흔한 경로).
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
                // `result` 가 없으면(아직 안 끝났거나 결과를 안 남긴 상태) 조용히
                // null 로 두지 않는다 — depends_on 검증을 통과했는데도 여기 오면
                // 그건 진짜 이상 상황이라 드러나야 한다.
                let output = t.result.and_then(|r| r.output).ok_or_else(|| {
                    format!("task output reference '{tid}': upstream task has no result output yet")
                })?;
                out.insert(tid, output);
            }
            Ok(out)
        })
    }

    /// lease pool 이 배정한 resource 로 치환된 `task.command` 를 store 에도
    /// 되써서 task-get/task-list 가 원본(`${lease.resource}`/빈 cwd) 대신 실제
    /// 배정 값을 보여주게 한다. best-effort — 실패해도 dispatch 자체(진짜
    /// 실행)는 이미 이 command 로 진행되므로 warn 로그만 남기고 계속한다.
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
        // lease + semaphore-gated dispatch. lease → semaphore 순서
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
        // lease 로 획득한 resource 식별자를 dispatch_command 호출 직전 command 에
        // 주입 — pool 모드에서 dispatch된 task가 실제로 어느 candidate 를
        // 받았는지 알 방법이 이 치환뿐이다(§ 아래 substitute_lease_resource).
        // 치환된 command 는 store 에도 되써서 task-get/task-list 조회 시점에
        // 실제 배정된 자원(예: cwd)이 그대로 드러나게 한다 — 안 그러면 원본
        // `${lease.resource}`/빈 cwd 만 보여 "어느 candidate 를 받았는지" 를
        // 외부에서 확인할 방법이 lease-list 뿐이게 된다.
        //
        // 치환은 **lease 먼저, upstream 출력 나중** 이다. 순서를 뒤집으면 upstream
        // task 가 만든 문자열 안에 `${lease.resource}` 가 들어 있을 때 그게
        // lease 치환의 입력이 되어, 자식 에이전트가 만든 데이터가 lease 의미론에
        // 끼어든다. 지금 순서면 나중에 주입되는 값(런타임 데이터)이 다시 해석되지
        // 않는다 — 각 치환은 자기 결과를 재훑지 않는 1-pass 다.
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
                // 치환된 command 를 store 에 되쓰면 task-get/task-list 가 "실제로
                // 무엇으로 호출됐는지" 를 보여준다(lease 선례와 같은 이유). 아무것도
                // 안 바뀌었으면 쓸 이유가 없다 — dispatch 마다 store write 를 더하지
                // 않는다.
                if let Some((ws, _, _)) = &leased {
                    self.persist_substituted_command(*ws, &substituted);
                } else if changed {
                    self.persist_substituted_command(task.workspace_id, &substituted);
                }
                self.dispatch_command(&substituted)
            }
            // 참조 해석 실패는 dispatch 실패다. 조용한 null 로 흘리면 downstream 이
            // 훨씬 뒤에서, 원인과 먼 자리에서 터진다.
            Err(e) => Err(format!("task output substitution: {e}")),
        };
        let result = match dispatch_result {
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
                        out.push(ReducerInput {
                            succeeded,
                            task_id: tid.clone(),
                            output,
                        });
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
                // 파이프 교착 회피: `wait()` 전에 stdout/stderr 를 각각 별도 스레드로
                // 드레인 시작. 읽지 않고 wait() 하면 자식이 OS 파이프 버퍼(16~64KB)를
                // 채우고 block, 부모는 그 자식의 종료를 기다리므로 둘 다 영원히 멈춘다.
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
                // task 당 스레드가 3개(watcher + stdout/stderr drain)로 는다 — drain
                // 스레드가 EOF 까지 읽는 동안 watcher 는 child.wait() 로 exit status 를
                // 기다리고, 그 뒤 두 drain 스레드를 join 해 캡처 결과를 조립한다.
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
                        if let Ok(mut g) = cell_clone.lock() {
                            *g = Some(outcome);
                        }
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
                // 동기 IPC dispatch. poll=None 이면 응답으로 즉시 종결(단, 결정 6 기본
                // 전략이 매칭되면 그 전략의 사양을 대신 사용), poll=Some(Inline) 이면
                // 인라인 사양대로, poll=Some(Named) 이면 완료 판정 전략 레지스트리로
                // 이름을 해석한다. poll/push 두 kind 모두 kind-agnostic
                // `resolve_strategy`(§B 결정 7)로 다룬다 — poll 이면 기존과 동일하게
                // `PolledDispatch`, push 면 `dispatch_push_strategy`로 `AwaitExternal`.
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
                // poll params 사전 해석: 원 요청 → 응답 순으로 채움 (응답이 요청보다 우선).
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

    /// push-kind 완료 전략 dispatch. `notify_via` 가 가리키는
    /// 훅 핸들러를 대상 surface 에 1회성(`once: true`)으로 바인딩해 살아있는
    /// `hook_id` 를 얻고, `hook_task_waits` 에 `(workspace_id, task_id, deadline)`
    /// 로 등록한 뒤 `AwaitExternal` 로 전이한다. 실제 종결(Succeeded/Failed)은
    /// 이 dispatch 가 아니라 `PendingHostEvent::HookFired` 소비부
    /// (`Core::resolve_hook_task_wait`, exit code 로 성공/실패 분기)와 timeout
    /// 안전망(`runner_thread::expire_overdue_hook_waits`)이 담당한다 —
    /// `AwaitExternal` 의 poll 은 계약대로 항상 `Active` 다(§C-2).
    ///
    /// **범위 제한**: 오늘 유일한 push 전략(`host/command-completed`)의 필요만
    /// 반영해 이벤트를 `HookEvent::CommandCompleted(None)`(모든 exit code 매칭)
    /// 으로 고정한다. 두 번째 push 유스케이스가 실제로 생기면(예: 향후
    /// claude/codex idle 신호) 그때 전략에 이벤트를 싣는 필드를 추가해
    /// 일반화한다 — 지금 존재하지 않는 요구를 미리 설계하지 않는다.
    ///
    /// surface_id 는 원 dispatch `params.surface_id` 에서 얻는다 — `surface.send`
    /// 등 surface 대상 IPC 메서드 대다수가 이 키를 쓰는 host 공통 관례
    /// (`require_surface_id`, `src/adapters/ipc/handler.rs`)를 그대로 재사용한다.
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
        // deadline 을 handle 자체에도 실어 둔다 — `hook_task_waits` 매핑은
        // 재시작 시 비영속으로 사라지지만, 영속되는 handle 쪽 deadline 은
        // 재시작 후 reload 경로가 만료 판정에 쓸 수 있다(결정 4).
        Ok(DispatchHandle::AwaitExternal {
            wait_key: hook_id.to_string(),
            deadline_ms,
        })
    }

    /// 내부 poll 본체.
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
                        // K.A-2: injector 가 ready → grace deadline reset.
                        self.injector_grace_deadline_ms = None;
                        v
                    }
                    Err(e) if is_injector_not_initialized(&e) => {
                        // K.A-2: 첫 미초기화 시점에 deadline 세팅. grace 안이면 Active,
                        // 도래 후이면 기존 정책대로 Failed.
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
                // 실패 목록을 **먼저** 본다. 두 목록에 같은 값이 들어간 선언은
                // 모순이지만 거부하지 않고 실패 쪽을 채택한다 — 실패를 성공으로
                // 읽어 downstream 이 없는 산출물을 전제로 진행하는 쪽이, 성공을
                // 실패로 읽어 멈추는 쪽보다 위험하다.
                if failure_states.iter().any(|s| s == state) {
                    // 응답 JSON 전체를 잃지 않게 요약을 메시지에 싣는다 —
                    // `Run` 이 stdout/stderr tail 을 에러 메시지에 이어붙이는 것과
                    // 같은 관례(`PollOutcome::Failed` 는 문자열 하나만 나른다).
                    return PollOutcome::Failed(format!(
                        "{poll_method}: failure state '{state}' — {}",
                        summarize_poll_response(&resp)
                    ));
                }
                if terminal_states.iter().any(|s| s == state) {
                    // terminal 도달 → 전체 응답을 산출물로 종결.
                    PollOutcome::Done(TaskResult {
                        exit_code: None,
                        output: Some(resp),
                        error: None,
                    })
                } else {
                    // 비-terminal → 계속 폴링(Active). 단 전체 timeout deadline 초과 시 Failed.
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
                // K.A-1: watcher cell 우선 조회. 채워졌으면 즉시 종결, 비어있으면 Active.
                if let Some(entry) = self.shell_children.get(pid) {
                    let taken = entry.result.lock().ok().and_then(|mut g| g.take());
                    if let Some(outcome) = taken {
                        self.shell_children.remove(pid);
                        return outcome;
                    }
                    return PollOutcome::Active;
                }
                // child map 없음 — 호스트 재시작 후 reload 된 핸들 (이 executor 에서
                // dispatch 하지 않음 → watcher 도 없음). 정확한 마감은 reload 단계가
                // run_result 영속 조회로 처리하므로, 여기서는 alive 만 판별:
                // alive → Active, dead → Failed (이미 reload 가 처리했어야 할 경우의
                // race 안전망).
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
            // AwaitExternal 계약대로 poll 은 절대 종결시키지 않는다 — 종결은
            // 외부(hook_id → task_id 매핑 소비 등)가 store 를 직접 전이시켜
            // 이뤄지고, `RunnerLoop::tick` 0단계(terminal 흡수)가 다음 tick 에
            // handle 정리 + release_permit 을 담당한다.
            DispatchHandle::AwaitExternal { .. } => PollOutcome::Active,
        }
    }
}

/// dispatch 직전 lease pool 이 배정한 resource 식별자를 command 에 주입하는
/// placeholder 토큰. `Run.cwd`/`Run.command` 인자/`Custom.params` 어디든 이
/// 문자열이 나타나면 실제 resource 로 치환된다.
const LEASE_RESOURCE_PLACEHOLDER: &str = "${lease.resource}";

/// `try_acquire_lease` 가 획득한 resource(`held_leases`)를 실제 실행 파라미터에
/// 주입한다.
///
/// - `Run.command` 의 각 인자 / `Run.cwd`: placeholder 치환. `cwd` 가 애초에
///   `None` 이면(가장 흔한 pool 사용법 — "이 후보 경로에서 실행해라") 곧장
///   resource 경로로 채운다.
/// - `Custom.params`: JSON 트리 전체를 재귀적으로 훑어 문자열 값 안의
///   placeholder 를 치환(예: `claude.spawn` 의 `cwd` 파라미터).
/// - `Reduce`/`WaitBarrier`: lease 와 무관 — no-op.
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

/// dispatch 직전 upstream task 의 `result.output` 을 command 에 주입한다.
///
/// 문법 소유자는 [`task_output_ref`](crate::core::agent::task_output_ref) 다 —
/// 여기서는 해석된 참조를 실제 값으로 바꾸는 일만 한다.
///
/// **타입 보존이 이 함수의 핵심이다.** 문자열 값이 *정확히* placeholder 하나뿐이면
/// 그 자리를 뽑아낸 `Value` 로 **통째 교체**한다. 문자열로 떨어뜨리면 숫자가
/// `"731"` 이 되어 `claude.spawn` 의 `require_surface_id`(`as_u64`) 같은 타입
/// 검사가 거부한다. placeholder 가 다른 텍스트에 섞여 있을 때만 문자열 보간이다
/// (그때는 애초에 문자열이 기대값이다).
///
/// 반환값은 "무언가 치환했는가" — 호출자가 store 되쓰기 여부를 정하는 데 쓴다.
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
            // 문자열 전체가 placeholder 하나 → 타입 보존 통째 교체.
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

/// 문자열 보간 — placeholder 가 없으면 `None`(원문 유지).
///
/// 값이 문자열이면 따옴표 없이 내용만, 그 밖(숫자/불리언/객체/배열/null)은 compact
/// JSON 으로 떨어뜨린다. **1-pass** 다 — 주입된 값 안에 placeholder 처럼 보이는
/// 텍스트가 있어도 다시 훑지 않는다(원문에서 찾은 범위만 교체한다).
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

/// 참조 하나를 값으로 해석. 조용한 `null` 대신 **에러**다 — 해석 실패를 값으로
/// 흘리면 downstream 이 한참 뒤에 엉뚱한 자리에서 터진다.
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

/// 스트림당 tail 상한 (64 KiB). 캡처 결과는 `run_result` 키 하나로 memory store 에
/// JSON 직렬화되어 들어가는데, 그 store 는 값 하나당 1 MiB(`MemoryConfig::entry_max_bytes`)
/// 를 넘으면 거부한다. JSON 이스케이프 최악의 경우(cargo 출력의 ANSI ESC 등 제어문자가
/// `\u00XX` 6바이트로 팽창)를 가정해도 64 KiB × 2스트림 raw → 최악 768 KiB 로 1 MiB
/// 아래에 안전하게 들어간다. tail(마지막 N바이트)을 남기는 이유: 빌드 도구는 실패
/// 요약과 실패 지점을 대개 출력 뒤쪽에 남긴다 — 첫 에러가 앞쪽에 있으면 놓친다는
/// 한계가 있고(v1 범위), ANSI 는 벗기지 않고 그대로 보존한다(벗기는 구현은 후속).
const CAPTURE_TAIL_CAP: usize = 64 * 1024;

/// 자식 stdout/stderr 드레인 스레드가 모은 결과 — 마지막 [`CAPTURE_TAIL_CAP`] 바이트만
/// 보관한다. `truncated`/`dropped_bytes` 는 절단이 실제로 일어났는지, 얼마나 버렸는지
/// 를 담아 `TaskResult.output` 에 그대로 실린다.
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

/// 실패로 종결한 poll 응답을 에러 메시지에 실을 때의 상한(바이트).
/// `Run` 의 [`CAPTURE_TAIL_CAP`] 과 달리 스트림 tail 이 아니라 상태 응답 JSON 이라
/// 훨씬 작다 — 원인 파악에 필요한 필드가 보이면 충분하다.
const POLL_FAILURE_SUMMARY_CAP: usize = 2 * 1024;

/// 실패로 종결하는 poll 응답을 에러 메시지에 실을 한 줄로 접는다.
///
/// `PollOutcome::Failed` 는 문자열 하나만 나르므로, 응답 JSON 을 통째로 잃으면
/// "왜 실패했는지" 의 유일한 단서가 상태값 하나로 줄어든다. 컴팩트 JSON 으로
/// 직렬화하되 [`POLL_FAILURE_SUMMARY_CAP`] 바이트에서 자른다 — task 의 error
/// 문자열은 memory store 값 하나에 실려 영속되므로 응답이 크면 상한이 필요하다.
fn summarize_poll_response(resp: &serde_json::Value) -> String {
    let mut text = resp.to_string();
    if text.len() > POLL_FAILURE_SUMMARY_CAP {
        // char boundary 로 내려서 자른다 — 멀티바이트 중간에서 자르면 패닉한다.
        let mut cut = POLL_FAILURE_SUMMARY_CAP;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("...(truncated)");
    }
    text
}

/// 자식 stdout/stderr 파이프를 EOF 까지 계속 읽으면서 마지막 [`CAPTURE_TAIL_CAP`]
/// 바이트만 남긴다(ring buffer 대신 단순 drain — 상한이 64KiB 라 성능 문제 없음).
///
/// **호출자 주의**: 이 함수는 자식이 stdio 를 닫을 때까지(보통 종료 시점) block 한다.
/// `child.wait()` 와 **반드시 별도 스레드**에서 동시에 돌려야 한다 — 안 그러면 자식이
/// OS 파이프 버퍼를 채우고 block, 부모가 그 자식의 wait() 를 기다리는 교착이 생긴다.
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

/// K.A-1: `ExitStatus` → `PollOutcome` 변환 (poll 의 try_wait fast path 와 동일 의미).
/// 결정 2·3: stdout/stderr 캡처(각 tail 64KiB + truncated/dropped_bytes)를 성공 시엔
/// `TaskResult.output` 에, 실패 시엔(Failed 가 `String` 하나만 나를 수 있는 계약이라)
/// 에러 메시지에 함께 실어 실패 진단(예: `cargo build` 컴파일 에러 본문)을 가능하게 한다.
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

/// K.A-1: ShellProcess 종료 결과를 memory store 에 영속 (workspace scope).
/// watcher thread 가 `child.wait()` 완료 직후 호출. best-effort — 실패 시 warn 로그.
pub(crate) fn persist_run_result(
    memory: &Arc<Mutex<dyn MemoryStorage>>,
    workspace_id: u32,
    task_id: &str,
    outcome: &PollOutcome,
) {
    let value = MemoryValue::Json(run_outcome_to_value(outcome));
    let res = {
        let mut guard = match memory.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
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

/// K.A-1: 영속된 ShellProcess 결과를 load (reload 단계가 사용).
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

/// K.A-1: ShellProcess 결과를 evict (release_permit / reload 가 호출).
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

/// 영속된 `DispatchHandle` 를 workspace_id 를 이미 아는 호출자가 직접 지운다.
/// `HostExecutor::evict_handle` 과 달리 `held_handles`(러너 스레드 로컬 북키핑)
/// 없이 동작 — task 삭제(`Core::task_delete`/GC sweep) 는 러너 스레드 밖(IPC
/// 핸들러 스레드, 부팅 시점)에서 일어나 `HostExecutor` 인스턴스에 접근할 수
/// 없으므로 필요하다. non-ShellProcess task 는 애초에 이 키가 없을 뿐이라
/// delete 자체는 idempotent(no-op)다.
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

/// task 삭제 시 정리해야 할 host 측 side-key 전부 — `tasty.agent.handle.<id>`
/// + `tasty.agent.run_result.<id>`(TODO11 결정 4: 정상 종료 경로 밖에서 지워지는
/// task 도 이 두 키가 orphan 으로 남지 않아야 한다).
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
        // Active 영속은 의미 없음 — watcher 는 종결 시점에만 호출.
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

    /// K.A.1: run_outcome JSON 직렬화 → 역직렬화 round trip.
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

    /// K.A.1: Failed variant serde round trip.
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

    /// K.A.1: watcher thread 가 자식 종료 후 run_result 영속 + cell 채움.
    /// "true" 명령 (Unix) — 즉시 종료 코드 0.
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
        // watcher 가 wait → cell 채움 + 영속 까지 대기 (max 2s).
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
        // memory 에도 영속됐는지 확인.
        let loaded = load_run_result(&ctx, 1, &task.id).expect("persisted");
        match loaded {
            PollOutcome::Done(r) => assert_eq!(r.exit_code, Some(0)),
            other => panic!("expected persisted Done, got {other:?}"),
        }
        // shell_children 에서도 제거됐는지.
        assert!(!exec.shell_children.contains_key(&pid));
    }

    /// 테스트 전용 Run task 빌더 — 필드 대부분이 테스트마다 동일해 중복 축소.
    #[cfg(unix)]
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

    /// dispatch 후 `exec.poll` 을 terminal 상태 도달까지 반복 — 도달 못 하면(교착 등)
    /// `max_ticks * 50ms` 후 panic 해 회귀를 잡는다("영원히 hang" 대신 실패로 드러남).
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
            "task did not reach a terminal state within {}ms — possible pipe deadlock",
            max_ticks * 50
        );
    }

    /// A: 출력이 있는 Run 명령의 stdout 이 `TaskResult.output` 에 담기는지 확인
    /// (완료 확인 방법 §1 — 구현 전에는 output 이 `{"pid": N}` 뿐이라 반드시 실패).
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

    /// A: 비0 종료 — state 는 Failed, exit code 는 에러 메시지에 명시(완료 확인 방법 §2).
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

    /// B: 대용량 stdout(파이프 버퍼 상한을 훌쩍 넘는 2MB) 을 내는 명령이 교착 없이
    /// 종결되는지 확인 — `Stdio::piped()` 만 붙이고 드레인을 안 하면 여기서 hang 한다.
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
        // 대용량 출력 드레인 여유를 두고 넉넉한 timeout(10s).
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

    /// B: stderr 만 대량 출력하는 케이스 — 한 스트림만 안 읽어도 교착하므로 별도 확인.
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

    /// 결정 2: `drain_capped` 가 상한 초과 시 tail 만 남기고 dropped_bytes 를 정확히
    /// 누적하는지 — 파이프/스레드 없이 순수 로직만 단위 테스트.
    #[test]
    fn drain_capped_truncates_to_tail_and_tracks_dropped_bytes() {
        // CAPTURE_TAIL_CAP(64KiB) 보다 큰 입력 — 앞부분은 버려지고 뒷부분(tail)만 남아야.
        let total = CAPTURE_TAIL_CAP + 100;
        let mut input = vec![b'a'; total];
        // 마지막 100 바이트만 다른 값으로 표시해 tail 이 실제로 "끝부분"인지 확인.
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

    /// 범용 폴링 핸들 헬퍼 — 특정 에이전트와 무관한 임의 poll method 로.
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

    /// 폴링 판정 단위 테스트용 fake injector — 주어진 응답을 요청 순서대로 돌려준다.
    /// `ctx.host_ipc` 를 채우고 워커 스레드 핸들을 반환한다(호출자가 join).
    fn spawn_fake_injector(
        ctx: &RunnerContext,
        responses: Vec<serde_json::Value>,
    ) -> std::thread::JoinHandle<()> {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
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

    /// 폴링 판정 단위 테스트용 핸들 — terminal/failure 목록을 직접 지정한다.
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

    /// `failure_states` 적중은 Done 이 아니라 Failed 다 — 자식이 죽었는데
    /// downstream 이 그 산출물을 전제로 진행하면 안 된다. 메시지에는 상태값과
    /// 응답 요약이 함께 실려야 원인을 볼 수 있다.
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

    /// terminal 과 failure 에 같은 값이 있으면 failure 우선 — 실패를 성공으로 읽는
    /// 쪽이 그 반대보다 위험하다.
    #[test]
    fn polled_dispatch_failure_states_take_precedence_over_terminal() {
        let (_td, ctx) = fresh_ctx();
        let worker = spawn_fake_injector(&ctx, vec![json!({ "state": "exited" })]);
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled_with_states("fake.poll", &["idle", "exited"], &["exited"]);
        assert!(matches!(exec.poll(&handle), PollOutcome::Failed(_)));
        worker.join().unwrap();
    }

    /// `failure_states` 가 비면 이 필드 도입 전과 완전히 같게 — terminal 도달은
    /// 전부 성공이다(기존 영속 handle 의 동작 보존).
    #[test]
    fn polled_dispatch_without_failure_states_still_succeeds_on_terminal() {
        let (_td, ctx) = fresh_ctx();
        let worker = spawn_fake_injector(&ctx, vec![json!({ "state": "exited" })]);
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled_with_states("fake.poll", &["idle", "exited"], &[]);
        assert!(matches!(exec.poll(&handle), PollOutcome::Done(_)));
        worker.join().unwrap();
    }

    /// 기존 영속 DAG JSON(`failure_states` 없음)이 그대로 역직렬화돼야 한다.
    #[test]
    fn poll_spec_without_failure_states_deserializes() {
        let spec: tasty_agent::PollSpec = serde_json::from_str(
            r#"{"poll_method":"x","state_field":"state","terminal_states":["idle"]}"#,
        )
        .expect("legacy inline PollSpec should still deserialize");
        assert!(spec.failure_states.is_empty());
        assert_eq!(spec.interval_ms, 500);
    }

    /// 실패 메시지 요약은 상한에서 잘리되 멀티바이트 경계에서 패닉하지 않는다.
    #[test]
    fn poll_failure_summary_truncates_on_char_boundary() {
        let big = "가".repeat(POLL_FAILURE_SUMMARY_CAP);
        let summary = summarize_poll_response(&json!({ "state": "exited", "log": big }));
        assert!(summary.ends_with("...(truncated)"), "{}", &summary[..80]);
        assert!(summary.len() <= POLL_FAILURE_SUMMARY_CAP + "...(truncated)".len());
    }

    /// K.A.2: injector 미초기화 1회 → Active + grace deadline 세팅.
    #[test]
    fn polled_dispatch_poll_injector_uninit_returns_active_within_grace() {
        let (_td, ctx) = fresh_ctx();
        // host_ipc OnceLock 비어있는 상태 — set_host_ipc_injector 호출 X.
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

    /// K.A.2: grace deadline 을 과거로 강제 → 다음 poll 은 Failed("grace expired").
    #[test]
    fn polled_dispatch_poll_after_grace_expired_returns_failed() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let handle = mk_polled("fake.poll");
        // 첫 poll 로 deadline 세팅 — 반환값은 Active (검증은 다음 라인의 덮어쓰기 후).
        let _first = exec.poll(&handle);
        debug_assert!(matches!(_first, PollOutcome::Active));
        exec.injector_grace_deadline_ms = Some(0); // 과거로 강제.
        let outcome = exec.poll(&handle);
        match outcome {
            PollOutcome::Failed(err) => {
                assert!(err.contains("injector grace expired"), "got {err}");
                assert!(err.contains("fake.poll"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// K.A.2: injector ready 가 되어 정상 dispatch 가 응답하면 grace deadline reset.
    /// 테스트 worker thread 가 poll method 에 terminal 상태 응답 — Done 으로 종결.
    #[test]
    fn polled_dispatch_recovers_after_injector_ready() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let handle = mk_polled("fake.poll");
        // 1차 poll — injector 미초기화 → Active + deadline 세팅.
        let outcome1 = exec.poll(&handle);
        assert!(matches!(outcome1, PollOutcome::Active));
        assert!(exec.injector_grace_deadline_ms.is_some());

        // Fake injector + worker thread: fake.poll 에 terminal("done") 응답.
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
        // 2차 poll — injector ready → fake.poll 응답 "done"(terminal) → Done.
        let outcome2 = exec.poll(&handle);
        worker.join().unwrap();
        match outcome2 {
            PollOutcome::Done(_) => {}
            other => panic!("expected Done, got {other:?}"),
        }
        // grace deadline reset 확인.
        assert!(
            exec.injector_grace_deadline_ms.is_none(),
            "deadline should reset after successful dispatch"
        );
    }

    /// K.A.2: injector 외 사유 Err 는 grace 우회 — 즉시 Failed.
    #[test]
    fn polled_dispatch_non_injector_error_fails_immediately() {
        use crate::ipc::host_call::HostIpcInjector;
        use std::sync::mpsc;
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let handle = mk_polled("fake.poll");
        // injector 를 ready 로 만들되 worker 가 응답을 보내지 않게 두면 dispatch 가
        // HOST_DISPATCH_TIMEOUT (5s) 후 timeout Err. 본 테스트는 channel disconnect 로
        // 빠르게 Err 유도 — sender 만 drop.
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        drop(rx); // receiver drop → sender.send 가 즉시 Err.
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
        // deadline 은 우회 — 변경 없음 (None 그대로).
        assert!(exec.injector_grace_deadline_ms.is_none());
    }

    /// 범용 폴링: Custom{poll:Some} → dispatch 응답/요청 키를 poll_params 로 매핑한 뒤
    /// terminal 상태 도달까지 폴링. (fake.start → {"job":"J1"}, fake.poll → running→done)
    #[test]
    fn custom_with_poll_maps_params_and_polls_to_done() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::collections::HashMap;
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpec, PollSpecRef};
        use tasty_ipc::server::IpcCommand;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());

        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let waker = std::sync::Arc::new(|| {});
        ctx.host_ipc
            .set(HostIpcInjector::new(tx, waker))
            .ok()
            .expect("set once");
        // worker: fake.start → {"job":"J1"}, fake.poll(1) → running, fake.poll(2) → done.
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
            // poll_params 매핑 검증: 요청(surface_id) + 응답(job) 양쪽 반영.
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

        // dispatch → PolledDispatch 핸들 + poll_params 매핑.
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
        // 1차 poll → running(비-terminal) → Active.
        assert!(matches!(exec.poll(&handle), PollOutcome::Active));
        // 2차 poll → done(terminal) → Done.
        match exec.poll(&handle) {
            PollOutcome::Done(r) => {
                assert_eq!(r.output, Some(json!({ "state": "done" })));
            }
            other => panic!("expected Done, got {other:?}"),
        }
        worker.join().unwrap();
    }

    /// 회귀: Custom{poll:None} → dispatch 응답으로 즉시 CustomImmediate.
    #[test]
    fn custom_without_poll_is_immediate() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::OnFailure;
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

    /// `poll: Some(Named)` → 완료 판정 전략 레지스트리로 이름 해석 후 그
    /// `PollSpec` 대로 폴링한다(인라인과 동등하게 동작).
    #[test]
    fn custom_with_named_poll_strategy_resolves_and_polls() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
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

    /// `poll: Some(Named)` 이 push-kind 전략을 가리키면
    /// `AwaitExternal` 로 전이한다: `hook.set` 를 통해 대상 surface(`params.
    /// surface_id`)에 1회성 훅을 등록하고, 그 hook_id 를 `hook_task_waits` 에
    /// 등록한다. poll 은 계약대로 절대 종결시키지 않는다(항상 Active) — 종결은
    /// `Core::resolve_hook_task_wait`(훅 발화 소비부)의 몫.
    #[test]
    fn custom_with_named_push_strategy_registers_hook_and_awaits_external() {
        use crate::hook_handler::types::{
            HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
        };
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
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
        // 계약: poll 은 절대 종결시키지 않는다.
        assert!(matches!(exec.poll(&handle), PollOutcome::Active));
        // hook_task_waits 에 실제로 등록됐는지 확인(1회성 소비 — resolve 로 검증).
        assert_eq!(
            ctx.hook_task_waits.resolve(999),
            Some((1, "t-push".to_string()))
        );

        crate::completion_strategy::HostCompletionStrategyPort.uninstall_plugin("rhtest-push");
    }

    /// push 전략인데 원 dispatch `params` 에 `surface_id` 가 없으면
    /// hook 을 어느 surface 에 걸지 알 수 없다 — dispatch 자체가 실패한다.
    #[test]
    fn custom_with_push_strategy_missing_surface_id_fails_dispatch() {
        use crate::hook_handler::types::{
            HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
        };
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
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

    /// `poll: Some(Named)` 이 미등록 이름을 가리키면 dispatch 자체가 실패(PermanentFail)
    /// — Running 진입 후가 아니라 dispatch 시점에 드러난다. dispatch 는 poll 해석
    /// 전에 먼저 `ipc_method` 를 호출하므로 그 응답까지는 정상적으로 흘려보낸다.
    #[test]
    fn custom_with_unknown_named_poll_strategy_fails_dispatch() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::{OnFailure, PollSpecRef};
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

    /// 결정 6 — `poll: None` 이라도 `default_for_methods` 로 그 IPC 메서드를 지목한
    /// poll 전략이 있으면 그 사양을 대신 사용한다(즉시-성공 하위호환보다 우선).
    #[test]
    fn custom_without_poll_uses_default_for_method_strategy() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::OnFailure;
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
        // poll:None 이라도 default_for_methods 매칭 때문에 즉시 CustomImmediate 가
        // 아니라 PolledDispatch 가 나와야 한다.
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

    /// J.A.S2: evict 후 store 에서 사라짐.
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

    // =====================================================================
    // lease pool 모드 (TODO 08: candidates/elastic) — `try_acquire_lease`
    // 자체는 실행(dispatch_command)과 독립적이므로 여기 테스트는 실제 process
    // spawn 없이 private 메서드를 직접 호출해 배정 로직만 검증한다. cwd/params
    // 치환은 뒤쪽 `dispatch_*` 테스트에서 실제 spawn 으로 end-to-end 확인한다.
    // =====================================================================

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

        // 4번째는 대기(Deferred = Some(false)) — Block 이 기본 mode.
        let mut task4 = mk_run_task("t-pool-3", vec!["true"]);
        task4.metadata = json!({ "lease": { "candidates": candidates } });
        assert_eq!(exec.try_acquire_lease(&task4).unwrap(), Some(false));
        assert!(!exec.held_leases.contains_key(&task4.id));

        // elastic 미지정 — 새 candidate 이름이 절대 합성되지 않았어야 한다.
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
        // 3(fixed) + 1(합성 상한) = 4개만 즉시 점유, 나머지 2개는 대기.
        assert_eq!(acquired, 4);
    }

    /// sugar 통합: `resource`(단일)와 `candidates`([단일])가 같은 lease key 로
    /// 충돌 판정되는지 — `try_acquire_lease` 파싱 층까지 포함해 확인.
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

    /// dispatch 치환이 store 에도 되써지는지 — `task-get`/`task-list` 가 원본
    /// (빈 cwd)이 아니라 실제 배정된 resource 를 보여줘야 한다(완료 확인 방법
    /// §2). in-memory `Task` 만 바꾸고 store 를 안 건드리면 이 요구를 못
    /// 만족하므로, 여기선 실제로 `TaskStore` 에 넣은 뒤 dispatch → 다시
    /// `TaskStore::get` 으로 읽어 확인한다.
    #[cfg(unix)]
    #[test]
    fn dispatch_persists_lease_substituted_cwd_for_task_get() {
        use tasty_agent::TaskStore;

        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx.clone());
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap().to_string();
        // `pwd` 는 getcwd() 가 준 실제 경로를 찍는다. macOS 의 tempdir 은
        // `/var`(→ `/private/var` 심볼릭 링크) 아래라 원본 문자열과 다르므로,
        // stdout 비교에는 심볼릭 링크를 푼 경로를 쓴다.
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

        // dispatch 도중(아직 Running 으로 store 전이되기 전) store 를 직접
        // 조회해도 이미 치환된 cwd 가 보여야 한다 — RunnerLoop 가 set_state
        // 를 부르기 전에 이 persist 가 먼저 일어나기 때문.
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

        // 정상 종결도 확인(치환된 cwd 로 실제 spawn 됐는지).
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

    // ── `${task.<id>.output<pointer>}` 치환 ──────────────────────────────

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

    /// 문자열 값 전체가 단일 placeholder 면 뽑아낸 Value 로 통째 교체 — 타입 보존.
    /// 문자열로 떨어지면 `claude.spawn` 의 `require_surface_id`(`as_u64`) 가 거부한다.
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
            "타입이 문자열로 떨어졌다"
        );
        assert!(
            params["surface_id"].as_u64().is_some(),
            "require_surface_id 의 as_u64() 가 통과해야 한다"
        );
        assert_eq!(params["message"], json!("hi"), "무관한 값은 그대로");
    }

    /// 숫자만이 아니라 객체/배열/불리언도 통째 교체된다 — 빈 포인터는 출력 전체.
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

    /// 부분 문자열 안에 섞이면 문자열 보간.
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
        // 문자열 값은 따옴표 없이 내용만 들어간다.
        assert_eq!(params["two"], json!("zilhak opened #42"));
    }

    /// 중첩 구조(배열/객체 안쪽)와 `Run` 인자에도 적용된다.
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
        // argv 는 어차피 문자열이라 타입 보존 문제가 없다 — 전부 보간.
        assert_eq!(command[1], "id=7");
        assert_eq!(cwd.as_deref(), Some(std::path::Path::new("/tmp/build")));
    }

    /// 포인터가 출력에 없으면 조용한 null 이 아니라 에러다.
    #[test]
    fn task_output_substitution_rejects_pointer_miss() {
        let outputs = outputs_of(&[("t-a", json!({ "child_surface_id": 731 }))]);
        let mut cmd = custom_params(json!({ "x": "${task.t-a.output/nope}" }));
        let err = substitute_task_outputs(&mut cmd, &outputs).unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }

    /// 주입된 값 안에 placeholder 처럼 보이는 텍스트가 있어도 재치환하지 않는다(1-pass).
    #[test]
    fn task_output_substitution_does_not_rescan_injected_values() {
        let outputs = outputs_of(&[("t-a", json!({ "text": "${task.t-b.output/x}" }))]);
        let mut cmd = custom_params(json!({ "p": "${task.t-a.output/text}" }));
        // t-b 는 outputs 에 없다 — 재치환한다면 여기서 에러가 났을 것이다.
        substitute_task_outputs(&mut cmd, &outputs).unwrap();
        let TaskCommand::Custom { params, .. } = &cmd else {
            panic!("expected Custom");
        };
        assert_eq!(params["p"], json!("${task.t-b.output/x}"));
    }

    /// 참조 task 가 아직 결과가 없으면 조용히 null 이 아니라 dispatch 실패.
    #[test]
    fn task_output_substitution_missing_result_is_permanent_fail() {
        use tasty_agent::TaskStore;
        let (_td, ctx) = fresh_ctx();
        let seq = ctx.agent_seq.clone();
        // upstream 은 store 에 있지만 result 가 없다.
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

    /// 참조 task 가 store 에 아예 없어도 dispatch 실패.
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

    /// dispatch 가 치환한 command 를 store 에 되쓴다 — task-get 이 "실제로 무엇으로
    /// 호출됐는지" 를 보여줘야 한다(lease 치환과 같은 이유).
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

    /// dispatch 치환: `cwd` 가 `None` 이면 획득한 lease resource 로 그대로
    /// 채워야 한다(완료 확인 방법 §2 — task-get 결과에 어느 자원을 받았는지
    /// 드러나야 한다는 요구를 `cwd` 로 실증).
    #[cfg(unix)]
    #[test]
    fn dispatch_fills_empty_run_cwd_with_acquired_lease_resource() {
        let (_td, ctx) = fresh_ctx();
        let mut exec = HostExecutor::new(ctx);
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_str().unwrap().to_string();
        // 위 테스트와 같은 이유 — `pwd` 출력은 심볼릭 링크가 풀린 경로다.
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

    /// dispatch 치환: `cwd`/command 인자에 이미 `${lease.resource}` placeholder
    /// 가 있으면 그 안의 값만 치환(원래 cwd 를 통째로 덮어쓰지 않음).
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
                // spawn 이 성공했다는 사실 자체가 cwd placeholder 가 실제 존재하는
                // 디렉터리로 치환됐음을 증명한다(치환 실패 시 존재하지 않는
                // "${lease.resource}" 경로로 spawn 이 실패해 PermanentFail).
                assert_eq!(
                    stdout_text.trim(),
                    dir_path,
                    "command arg placeholder should also have been substituted"
                );
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    /// dispatch 치환: `Custom.params` 안의 placeholder 도 dispatch_plugin 호출
    /// 전에 치환돼야 한다(예: `claude.spawn` 의 `cwd` 파라미터 시나리오).
    #[test]
    fn dispatch_substitutes_lease_placeholder_in_custom_params() {
        use crate::ipc::host_call::HostIpcInjector;
        use crate::ipc::protocol::JsonRpcResponse;
        use std::sync::mpsc;
        use tasty_agent::OnFailure;
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

    /// release 경로: pool 모드로 얻은 자원도 `release_permit` 한 번으로 정상
    /// 반환되고, 그 뒤 다른 holder 가 바로 이어받을 수 있어야 한다.
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
