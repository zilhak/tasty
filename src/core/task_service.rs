//! 작업 실행 자원을 소유하고 IPC·App이 부르는 작업 API를 제공한다.
//! 작업 정의·상태·결과의 원본은 memory의 TaskStore이며 이 서비스는 두 번째 사본을 만들지 않는다.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use tasty_ipc::host_call::HostIpcInjector;
use tasty_memory::MemoryStorage;

use crate::core::agent::event_feed::AgentEventQueue;
use crate::core::agent::hook_wait::HookTaskWaits;
use crate::core::agent::runner_host::RunnerContext;
use crate::core::agent::runner_thread::RunnerRegistry;
use crate::core::agent::task_waker::TaskWakerHub;

/// engine 하나의 작업 실행 범위. 완료 대기 허브와 사건 큐는 engine마다 따로 두며,
/// 완료 통지는 task가 속한 workspace를 가진 engine의 범위로만 간다.
pub(crate) struct TaskScope {
    /// 같은 밀리초에 만든 task ID를 구별하는 순번. 창을 새로 열면 기존 engine과 공유한다.
    agent_seq: Arc<AtomicU64>,
    /// task 종결을 대기자에게 알리고 같은 사건 큐에도 기록한다.
    waker_hub: Arc<TaskWakerHub>,
    /// runner 스레드에서 생성한 사건을 메인 루프로 넘기는 큐.
    event_queue: Arc<AgentEventQueue>,
    /// 서비스를 받지 않는 렌더 경로가 runner 상태를 조회하도록 부팅 때 주입한다.
    runner_registry: OnceLock<Arc<RunnerRegistry>>,
}

impl TaskScope {
    pub(crate) fn new() -> Self {
        Self::with_seq(Arc::new(AtomicU64::new(0)))
    }

    /// 허브·사건 큐는 새로 만들고 task ID 순번은 받은 것을 쓴다.
    /// 새 창의 engine은 기존 engine의 순번을 넘겨 ID 발급을 공유한다.
    pub(crate) fn with_seq(agent_seq: Arc<AtomicU64>) -> Self {
        // 허브가 기록하는 큐와 메인 루프가 비우는 큐가 같아야 한다.
        let event_queue = Arc::new(AgentEventQueue::new());
        Self {
            agent_seq,
            waker_hub: Arc::new(TaskWakerHub::with_feed(Arc::clone(&event_queue))),
            event_queue,
            runner_registry: OnceLock::new(),
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

    /// 화면은 표시 중인 DAG의 task만 세므로 러너 실행·crash 여부만 반환한다.
    /// 레지스트리가 주입되지 않았으면 (false, false)다.
    #[cfg(feature = "gui")]
    pub(crate) fn runner_liveness(&self, workspace_id: u32) -> (bool, bool) {
        self.runner_registry
            .get()
            .map(|registry| registry.liveness(workspace_id))
            .unwrap_or((false, false))
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

    pub(crate) fn runner_context(&self, engine: &crate::core::CoreState) -> RunnerContext {
        RunnerContext {
            memory: self.memory.clone(),
            agent_seq: engine.task_scope.agent_seq().clone(),
            host_ipc: self.host_ipc.clone(),
            task_waker_hub: engine.task_scope.waker_hub().clone(),
            hook_task_waits: self.hook_task_waits.clone(),
        }
    }

    /// 현재 engine의 workspace에 남은 runner 상태를 정리한다. runner 스레드를 자동 시작하지는 않는다.
    pub(crate) fn purge_stale_agent_state_on_boot(&self, engine: &crate::core::CoreState) {
        let ctx = self.runner_context(engine);
        let workspace_ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();
        crate::core::agent::runner_thread::purge_stale_agent_state_on_boot(&ctx, &workspace_ids);
    }

    pub(crate) fn runner_registry(&self) -> Arc<RunnerRegistry> {
        self.runner_registry.clone()
    }

    pub(crate) fn hook_task_waits(&self) -> &HookTaskWaits {
        &self.hook_task_waits
    }

    /// 렌더링 등 서비스를 받지 않는 코드가 같은 runner 상태를 조회하도록 Arc를 주입한다.
    /// OnceLock이 이미 차 있으면 덮어쓰지 않고 경고한다.
    pub(crate) fn inject_agent_runner_registry(&self, engine: &crate::core::CoreState) {
        if engine
            .task_scope
            .runner_registry
            .set(self.runner_registry())
            .is_err()
        {
            tracing::warn!("agent runner registry already injected into the task scope");
        }
    }
}
