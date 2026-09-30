//! 작업 실행 자원을 소유하고 IPC·App이 부르는 작업 API를 제공한다.
//! 작업 정의·상태·결과의 원본은 memory의 TaskStore이며 이 서비스는 두 번째 사본을 만들지 않는다.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use tasty_ipc::host_call::HostIpcInjector;
use tasty_memory::MemoryStorage;

use crate::core::agent::event_feed::AgentEventQueue;
use crate::core::agent::hook_wait::HookTaskWaits;
use crate::core::agent::runner_host::RunnerContext;
use crate::core::agent::runner_thread::{RunnerRegistry, RunnerStatus};
use crate::core::agent::task_waker::{AwaitOutcome, TaskWakerHub, TerminalSnapshot};

/// engine 하나의 작업 실행 범위. 완료 대기 허브와 사건 큐는 engine마다 따로 두며,
/// 완료 통지는 task가 속한 workspace를 가진 engine의 범위로만 간다.
pub(crate) struct TaskScope {
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
    pub(crate) fn new(runner_registry: Arc<RunnerRegistry>) -> Self {
        Self::with_seq(Arc::new(AtomicU64::new(0)), runner_registry)
    }

    /// 허브·사건 큐는 새로 만들고 task ID 순번은 받은 것을 쓴다.
    /// 새 창의 engine은 기존 engine의 순번을 넘겨 ID 발급을 공유한다.
    pub(crate) fn with_seq(
        agent_seq: Arc<AtomicU64>,
        runner_registry: Arc<RunnerRegistry>,
    ) -> Self {
        // 허브가 기록하는 큐와 메인 루프가 비우는 큐가 같아야 한다.
        let event_queue = Arc::new(AgentEventQueue::new());
        Self {
            agent_seq,
            waker_hub: Arc::new(TaskWakerHub::with_feed(Arc::clone(&event_queue))),
            event_queue,
            runner_registry,
        }
    }

    pub(crate) fn agent_seq(&self) -> &Arc<AtomicU64> {
        &self.agent_seq
    }

    pub(crate) fn waker_hub(&self) -> &Arc<TaskWakerHub> {
        &self.waker_hub
    }

    pub(crate) fn event_queue(&self) -> &Arc<AgentEventQueue> {
        &self.event_queue
    }

    #[cfg(test)]
    pub(crate) fn runner_registry(&self) -> &Arc<RunnerRegistry> {
        &self.runner_registry
    }

    /// 화면은 표시 중인 DAG의 task만 세므로 러너 실행·crash 여부만 반환한다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(dead_code, reason = "runner 상태를 읽는 DAG 화면은 gui 빌드에만 있다")
    )]
    pub(crate) fn runner_liveness(&self, workspace_id: u32) -> (bool, bool) {
        self.runner_registry.liveness(workspace_id)
    }
}

/// 러너 등록부와 훅-작업 연결은 프로세스에 하나다. engine별 자원은 engine이 가진 범위로 받는다.
pub(crate) struct TaskService {
    memory: Arc<Mutex<dyn MemoryStorage>>,
    /// Core와 같은 Arc다. IPC 서버 시작 뒤 주입된 값을 러너 스레드가 읽는다.
    host_ipc: Arc<OnceLock<HostIpcInjector>>,
    runner_registry: Arc<RunnerRegistry>,
    hook_task_waits: Arc<HookTaskWaits>,
}

impl TaskService {
    pub(crate) fn new(
        memory: Arc<Mutex<dyn MemoryStorage>>,
        host_ipc: Arc<OnceLock<HostIpcInjector>>,
    ) -> Self {
        Self {
            memory,
            host_ipc,
            runner_registry: Arc::new(RunnerRegistry::new()),
            hook_task_waits: Arc::new(HookTaskWaits::new()),
        }
    }

    /// Core와 같은 poison 복구 헬퍼로 저장소 락을 얻는다. 콜백이 끝날 때까지 락을 유지한다.
    pub(crate) fn with_memory<R>(&self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        let mut guard = crate::poison::recover_mutex(
            self.memory.lock(),
            crate::core::MEMORY_WHAT,
            &crate::core::MEMORY_POISONED,
        );
        f(&mut *guard)
    }

    pub(crate) fn memory(&self) -> &Mutex<dyn MemoryStorage> {
        &self.memory
    }

    pub(crate) fn runner_context(&self, scope: &TaskScope) -> RunnerContext {
        RunnerContext {
            memory: self.memory.clone(),
            agent_seq: scope.agent_seq().clone(),
            host_ipc: self.host_ipc.clone(),
            task_waker_hub: scope.waker_hub().clone(),
            hook_task_waits: self.hook_task_waits.clone(),
        }
    }

    /// 받은 workspace에 남은 runner 상태를 정리한다. runner 스레드를 자동 시작하지는 않는다.
    pub(crate) fn purge_stale_agent_state_on_boot(&self, scope: &TaskScope, workspace_ids: &[u32]) {
        let ctx = self.runner_context(scope);
        crate::core::agent::runner_thread::purge_stale_agent_state_on_boot(&ctx, workspace_ids);
    }

    pub(crate) fn hook_task_waits(&self) -> &HookTaskWaits {
        &self.hook_task_waits
    }

    /// engine의 TaskScope를 만들 때 넘길 등록부. 모든 engine이 이 Arc 하나를 공유한다.
    pub(crate) fn runner_registry(&self) -> &Arc<RunnerRegistry> {
        &self.runner_registry
    }

    /// 이미 실행 중이면 false다. 작업 취소나 OS 자식 종료와는 별개의 계약이다.
    pub(crate) fn runner_start(&self, scope: &TaskScope, workspace_id: u32) -> bool {
        self.runner_registry
            .start(self.runner_context(scope), workspace_id)
    }

    /// runner 스레드만 멈춘다. 작업 상태를 바꾸거나 실행 중인 자식 프로세스를 종료하지 않는다.
    pub(crate) fn runner_stop(&self, workspace_id: u32) -> bool {
        self.runner_registry.stop(workspace_id)
    }

    /// 러너가 꺼져 있어도 저장소를 조회해 실제 작업 수를 채운다.
    pub(crate) fn runner_status(&self, scope: &TaskScope, workspace_id: u32) -> RunnerStatus {
        self.runner_registry
            .status(&self.runner_context(scope), workspace_id)
    }

    /// 외부 완료 신호를 기다리는 작업의 저장된 dispatch handle을 읽는다.
    pub(crate) fn dispatch_handle(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &str,
    ) -> Option<tasty_agent::DispatchHandle> {
        crate::core::agent::runner_host::load_dispatch_handle(
            &self.runner_context(scope),
            workspace_id,
            task_id,
        )
    }

    /// 메인 루프 밖의 워커로 옮겨 기다릴 수 있도록 이 engine 범위의 대기 계약을 떼어 준다.
    pub(crate) fn awaiter(&self, scope: &TaskScope) -> TaskAwaiter {
        TaskAwaiter {
            memory: self.memory.clone(),
            agent_seq: scope.agent_seq().clone(),
            waker_hub: scope.waker_hub().clone(),
        }
    }
}

/// 한 engine 범위의 task 종결 대기. 허브에 대기자를 먼저 등록한 뒤 저장소를 읽어
/// 조회와 등록 사이에 지나간 완료를 놓치지 않는다.
pub(crate) struct TaskAwaiter {
    memory: Arc<Mutex<dyn MemoryStorage>>,
    agent_seq: Arc<AtomicU64>,
    waker_hub: Arc<TaskWakerHub>,
}

impl TaskAwaiter {
    /// None·Some(0)은 무기한 대기다. 저장소 조회가 실패하면 작업이 없는 것으로 본다.
    pub(crate) fn await_terminal(
        &self,
        workspace_id: u32,
        task_id: &tasty_agent::TaskId,
        timeout_ms: Option<u64>,
    ) -> AwaitOutcome {
        let load_current = || -> Option<TerminalSnapshot> {
            let mut guard = crate::poison::recover_mutex(
                self.memory.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            let store = tasty_agent::TaskStore::new(
                &mut *guard,
                tasty_memory::HOST_OWNER,
                self.agent_seq.as_ref(),
            );
            match store.get(workspace_id, task_id) {
                Ok(Some(t)) => Some(TerminalSnapshot {
                    state: t.state,
                    result: t.result,
                }),
                Ok(None) | Err(_) => None,
            }
        };
        self.waker_hub
            .await_terminal(workspace_id, task_id, timeout_ms, load_current)
    }
}
