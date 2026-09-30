//! 작업 실행 자원을 소유하고 IPC·App이 부르는 작업 API를 제공한다.
//! 작업 정의·상태·결과의 원본은 memory의 TaskStore이며 이 서비스는 두 번째 사본을 만들지 않는다.

use std::sync::{Arc, Mutex, OnceLock};

use tasty_ipc::host_call::HostIpcInjector;
use tasty_memory::MemoryStorage;

use crate::core::agent::hook_wait::HookTaskWaits;
use crate::core::agent::runner_host::RunnerContext;
use crate::core::agent::runner_thread::RunnerRegistry;

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
            agent_seq: engine.agent_seq.clone(),
            host_ipc: self.host_ipc.clone(),
            task_waker_hub: engine.task_waker_hub.clone(),
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
            .agent_runner_registry
            .set(self.runner_registry())
            .is_err()
        {
            tracing::warn!("agent runner registry already injected into CoreState");
        }
    }
}
