//! Core의 메모리 저장소로 barrier를 생성·조회·갱신한다.

use tasty_agent::{AgentError, Barrier, BarrierState, BarrierStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;
use crate::core::CoreState;

impl Core {
    pub(crate) fn barrier_create(
        &self,
        workspace_id: u32,
        name: String,
        count_required: u32,
        timeout_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<Barrier, AgentError> {
        self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.create(workspace_id, name, count_required, timeout_ms, now_ms)
        })
    }

    /// 신호가 요구 수를 채우면 Closed 이벤트를 큐에 넣는다. timeout 갱신은 이벤트를 만들지 않는다.
    pub(crate) fn barrier_signal(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        name: &str,
        now_ms: u64,
    ) -> Result<Barrier, AgentError> {
        let result = self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.signal(workspace_id, name, now_ms)
        });
        if let Ok(b) = &result
            && b.state == BarrierState::Closed
        {
            engine.agent_event_queue.push(
                crate::core::agent::event_feed::AgentEvent::BarrierClosed {
                    workspace_id,
                    name: b.name.clone(),
                    count_required: b.count_required,
                },
            );
        }
        result
    }

    /// 조회하면서 만료 상태도 저장할 수 있다.
    pub(crate) fn barrier_state(
        &self,
        workspace_id: u32,
        name: &str,
        now_ms: u64,
    ) -> Result<Barrier, AgentError> {
        self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.state(workspace_id, name, now_ms)
        })
    }

    /// now_ms가 있으면 만료 상태를 반영한 목록을 반환한다.
    pub(crate) fn barrier_list(
        &self,
        workspace_id: u32,
        now_ms: Option<u64>,
    ) -> Result<Vec<Barrier>, AgentError> {
        self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.list(workspace_id, now_ms)
        })
    }

    pub(crate) fn barrier_delete(&self, workspace_id: u32, name: &str) -> Result<(), AgentError> {
        self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.delete(workspace_id, name)
        })
    }
}
