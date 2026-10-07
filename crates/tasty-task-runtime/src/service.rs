//! 작업 실행 자원을 소유하고 IPC·App이 부르는 작업 API를 제공한다.
//! 작업 정의·상태·결과의 원본은 memory의 TaskStore이며 이 서비스는 두 번째 사본을 만들지 않는다.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use tasty_ipc::host_call::HostIpcInjector;
use tasty_memory::MemoryStorage;

use crate::event_feed::AgentEventQueue;
use crate::hook_wait::HookTaskWaits;
use crate::runner_host::RunnerContext;
use crate::runner_thread::{RunnerRegistry, RunnerStatus, RunnerStopReceipt};
use crate::task_waker::{AwaitOutcome, TaskWakerHub, TerminalSnapshot};

/// engine 하나의 작업 실행 범위. 완료 대기 허브와 사건 큐는 engine마다 따로 두며,
/// 완료 통지는 task가 속한 workspace를 가진 engine의 범위로만 간다.
pub struct TaskScope {
    stopping: Arc<std::sync::atomic::AtomicBool>,
    /// 같은 밀리초에 만든 task ID를 구별하는 순번. 창을 새로 열면 기존 engine과 공유한다.
    agent_seq: Arc<AtomicU64>,
    /// task 종결을 대기자에게 알리고 같은 사건 큐에도 기록한다.
    waker_hub: Arc<TaskWakerHub>,
    /// runner 스레드에서 생성한 사건을 메인 루프로 넘기는 큐.
    event_queue: Arc<AgentEventQueue>,
    /// 서비스를 받지 않는 렌더 경로가 runner 상태를 조회한다. 모든 engine이 TaskService와 같은 Arc를 든다.
    runner_registry: Arc<RunnerRegistry>,
}

impl TaskScope {
    /// runner 등록부는 TaskService의 것을 넘긴다. 다른 등록부를 넘기면 화면의 runner 상태가 틀린다.
    pub fn new(runner_registry: Arc<RunnerRegistry>) -> Self {
        Self::with_seq(Arc::new(AtomicU64::new(0)), runner_registry)
    }

    /// 허브·사건 큐는 새로 만들고 task ID 순번은 받은 것을 쓴다.
    /// 새 창의 engine은 기존 engine의 순번을 넘겨 ID 발급을 공유한다.
    pub fn with_seq(agent_seq: Arc<AtomicU64>, runner_registry: Arc<RunnerRegistry>) -> Self {
        // 허브가 기록하는 큐와 메인 루프가 비우는 큐가 같아야 한다.
        let event_queue = Arc::new(AgentEventQueue::new());
        Self {
            stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            agent_seq,
            waker_hub: Arc::new(TaskWakerHub::with_feed(Arc::clone(&event_queue))),
            event_queue,
            runner_registry,
        }
    }

    pub fn agent_seq(&self) -> &Arc<AtomicU64> {
        &self.agent_seq
    }

    pub fn waker_hub(&self) -> &Arc<TaskWakerHub> {
        &self.waker_hub
    }

    pub fn event_queue(&self) -> &Arc<AgentEventQueue> {
        &self.event_queue
    }

    /// Retire this scope's original runner only; dropping TaskScope itself does not stop it.
    pub fn request_stop_workspace(&self, workspace: u32) -> RunnerStopReceipt {
        self.runner_registry
            .request_stop(&self.waker_hub, Some(workspace))
    }

    pub fn runner_registry(&self) -> &Arc<RunnerRegistry> {
        &self.runner_registry
    }

    /// 화면은 표시 중인 DAG의 task만 세므로 러너 실행·crash 여부만 반환한다.
    pub fn runner_liveness(&self, workspace_id: u32) -> (bool, bool) {
        self.runner_registry
            .scoped_liveness(&self.waker_hub, workspace_id)
    }
}

/// 러너 등록부와 훅-작업 연결은 프로세스에 하나다. engine별 자원은 engine이 가진 범위로 받는다.
pub struct TaskService {
    memory: Arc<Mutex<dyn MemoryStorage>>,
    /// App과 같은 Arc다. IPC 서버 시작 뒤 주입된 값을 러너 스레드가 읽는다.
    host_ipc: Arc<OnceLock<HostIpcInjector>>,
    runner_registry: Arc<RunnerRegistry>,
    hook_task_waits: Arc<HookTaskWaits>,
    agent_turns: Arc<crate::agent_turns::AgentTurns>,
    completion: Arc<dyn crate::completion::CompletionResolver>,
    /// 저장소가 memory 대체 모드면 그 원인. 재시작 복구를 요구한 그래프를 거절하는 데 쓴다.
    store_fallback: Option<String>,
}

impl TaskService {
    pub fn new(
        memory: Arc<Mutex<dyn MemoryStorage>>,
        host_ipc: Arc<OnceLock<HostIpcInjector>>,
        completion: Arc<dyn crate::completion::CompletionResolver>,
    ) -> Self {
        Self {
            memory,
            host_ipc,
            completion,
            runner_registry: Arc::new(RunnerRegistry::new()),
            hook_task_waits: Arc::new(HookTaskWaits::new()),
            agent_turns: Arc::new(crate::agent_turns::AgentTurns::new()),
            store_fallback: None,
        }
    }

    /// 저장소가 memory 대체 모드임을 알린다. `cause` 는 대체 모드의 원인 이름이다.
    pub fn with_store_fallback(mut self, cause: Option<String>) -> Self {
        self.store_fallback = cause;
        self
    }

    /// 저장소가 재시작 뒤에도 남는가.
    pub fn store_durable(&self) -> bool {
        self.store_fallback.is_none()
    }

    pub(crate) fn store_fallback(&self) -> Option<&str> {
        self.store_fallback.as_deref()
    }

    /// 공용 poison 복구 정책으로 저장소 락을 얻는다. 콜백이 끝날 때까지 락을 유지한다.
    pub fn with_memory<R>(&self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.memory.lock(),
            tasty_memory::STORE_LOCK_WHAT,
            &tasty_memory::STORE_LOCK_POISONED,
        );
        f(&mut *guard)
    }

    pub fn memory(&self) -> &Mutex<dyn MemoryStorage> {
        &self.memory
    }

    pub(crate) fn runner_context(&self, scope: &TaskScope) -> RunnerContext {
        RunnerContext {
            scope_stopping: scope.stopping.clone(),
            memory: self.memory.clone(),
            agent_seq: scope.agent_seq().clone(),
            host_ipc: self.host_ipc.clone(),
            task_waker_hub: scope.waker_hub().clone(),
            hook_task_waits: self.hook_task_waits.clone(),
            agent_turns: self.agent_turns.clone(),
            completion: self.completion.clone(),
        }
    }

    /// 받은 workspace에 남은 runner 상태를 정리한다. runner 스레드를 자동 시작하지는 않는다.
    pub fn purge_stale_agent_state_on_boot(&self, scope: &TaskScope, workspace_ids: &[u32]) {
        let ctx = self.runner_context(scope);
        crate::runner_thread::purge_stale_agent_state_on_boot(&ctx, workspace_ids);
    }

    pub fn agent_turns(&self) -> &crate::agent_turns::AgentTurns {
        &self.agent_turns
    }

    pub fn hook_task_waits(&self) -> &HookTaskWaits {
        &self.hook_task_waits
    }

    /// engine의 TaskScope를 만들 때 넘길 등록부. 모든 engine이 이 Arc 하나를 공유한다.
    pub fn runner_registry(&self) -> &Arc<RunnerRegistry> {
        &self.runner_registry
    }

    /// 이미 실행 중이면 false다. 작업 취소나 OS 자식 종료와는 별개의 계약이다.
    pub fn runner_start(&self, scope: &TaskScope, workspace_id: u32) -> bool {
        self.runner_registry
            .start(self.runner_context(scope), workspace_id)
    }

    /// runner 스레드만 멈춘다. 작업 상태를 바꾸거나 실행 중인 자식 프로세스를 종료하지 않는다.
    pub fn runner_stop(&self, workspace_id: u32) -> bool {
        self.runner_registry.stop(workspace_id)
    }

    pub fn request_stop_workspace(&self, scope: &TaskScope, workspace: u32) -> RunnerStopReceipt {
        self.runner_registry
            .request_stop(scope.waker_hub(), Some(workspace))
    }
    pub fn request_stop_scope(&self, scope: &TaskScope) -> RunnerStopReceipt {
        scope
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        self.runner_registry.request_stop(scope.waker_hub(), None)
    }
    /// Explicit compatibility stop, fenced to the supplied scope rather than only the numeric ID.
    pub fn runner_stop_scoped(&self, scope: &TaskScope, workspace: u32) -> bool {
        self.runner_registry
            .stop_scoped(scope.waker_hub(), workspace)
    }
    pub fn poll_runner_stops(&self) -> usize {
        self.runner_registry.poll_stops()
    }

    /// 러너가 꺼져 있어도 저장소를 조회해 실제 작업 수를 채운다.
    pub fn runner_status(&self, scope: &TaskScope, workspace_id: u32) -> RunnerStatus {
        self.runner_registry
            .status(&self.runner_context(scope), workspace_id)
    }

    /// 외부 완료 신호를 기다리는 작업의 저장된 dispatch handle을 읽는다.
    pub fn dispatch_handle(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &str,
    ) -> Option<tasty_agent::DispatchHandle> {
        crate::runner_host::load_dispatch_handle(&self.runner_context(scope), workspace_id, task_id)
    }

    /// 메인 루프 밖의 워커로 옮겨 기다릴 수 있도록 이 engine 범위의 대기 계약을 떼어 준다.
    pub fn awaiter(&self, scope: &TaskScope) -> TaskAwaiter {
        TaskAwaiter {
            memory: self.memory.clone(),
            agent_seq: scope.agent_seq().clone(),
            waker_hub: scope.waker_hub().clone(),
        }
    }
}

/// 한 engine 범위의 task 종결 대기. 허브에 대기자를 먼저 등록한 뒤 저장소를 읽어
/// 조회와 등록 사이에 지나간 완료를 놓치지 않는다.
pub struct TaskAwaiter {
    memory: Arc<Mutex<dyn MemoryStorage>>,
    agent_seq: Arc<AtomicU64>,
    waker_hub: Arc<TaskWakerHub>,
}

impl TaskAwaiter {
    /// None·Some(0)은 무기한 대기다. 저장소 조회가 실패하면 작업이 없는 것으로 본다.
    pub fn await_terminal(
        &self,
        workspace_id: u32,
        task_id: &tasty_agent::TaskId,
        timeout_ms: Option<u64>,
    ) -> AwaitOutcome {
        let load_current = || -> Option<TerminalSnapshot> {
            let mut guard = tasty_utils::poison::recover_mutex(
                self.memory.lock(),
                tasty_memory::STORE_LOCK_WHAT,
                &tasty_memory::STORE_LOCK_POISONED,
            );
            let store = tasty_agent::TaskStore::new(
                &mut *guard,
                tasty_memory::HOST_OWNER,
                self.agent_seq.as_ref(),
            );
            let found = store.get(workspace_id, task_id).and_then(|t| match t {
                Some(t) => Ok(Some((t, store.revision(workspace_id, task_id)?))),
                None => Ok(None),
            });
            match found {
                Ok(Some((t, revision))) => Some(TerminalSnapshot::of(&t, revision)),
                Ok(None) => None,
                Err(error) => {
                    tracing::warn!(%error, workspace_id, %task_id, "task await lookup failed");
                    None
                }
            }
        };
        self.waker_hub
            .await_terminal(workspace_id, task_id, timeout_ms, load_current)
    }
}
