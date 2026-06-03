//! Barrier store wrapper. handler 의 `core.with_memory + BarrierStore::new`
//! 조립을 본 모듈로 흡수.

use tasty_agent::{AgentError, Barrier, BarrierStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

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
    pub(crate) fn barrier_signal(
        &self,
        workspace_id: u32,
        name: &str,
        now_ms: u64,
    ) -> Result<Barrier, AgentError> {
        self.with_memory(|mem| {
            let mut store = BarrierStore::new(mem, HOST_OWNER);
            store.signal(workspace_id, name, now_ms)
        })
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
