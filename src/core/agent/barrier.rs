//! Barrier store wrapper. handler 의 `core.with_memory + BarrierStore::new`
//! 조립을 본 모듈로 흡수.

use tasty_agent::{AgentError, Barrier, BarrierState, BarrierStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;
use crate::core::CoreState;

impl Core {
    /// Barrier 생성.
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

    /// Barrier 신호 1회 누적.
    ///
    /// 이 호출이 요구 수를 채우면 barrier 가 닫히고, 그 사실이 사건 피드에 적힌다.
    /// **닫힘은 여기서만 일어난다** — `BarrierStore::signal` 이 `Closed` 를 쓰는
    /// 유일한 자리이고 그 함수의 호출자도 이것 하나다. 시간 초과는 전이가 일어나는
    /// 순간이 없어(읽는 쪽이 시계를 견줄 때 도장이 찍힌다) 사건이 안 된다.
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

    /// Barrier 현 상태 조회 (timeout 도장 적용 포함이므로 mut store).
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

    /// Workspace 내 모든 Barrier 나열. `now_ms` 가 `Some` 이면 timeout 도장 적용.
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

    /// Barrier 삭제. 존재하지 않으면 no-op.
    pub(crate) fn barrier_delete(&self, workspace_id: u32, name: &str) -> Result<(), AgentError> {
        self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.delete(workspace_id, name)
        })
    }
}
