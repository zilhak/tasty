//! Semaphore store wrapper. handler 의 `core.with_memory + SemaphoreStore::new`
//! 조립을 본 모듈로 흡수.

use tasty_agent::{AcquireOutcome, AgentError, ReleaseOutcome, Semaphore, SemaphoreStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

impl Core {
    /// Semaphore 생성.
    pub(crate) fn semaphore_create(
        &self,
        workspace_id: u32,
        name: String,
        permits: u32,
        now_ms: u64,
    ) -> Result<Semaphore, AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.create(workspace_id, name, permits, now_ms)
        })
    }

    /// Semaphore permit 점유 시도.
    pub(crate) fn semaphore_acquire(
        &self,
        workspace_id: u32,
        name: &str,
        holder: &str,
    ) -> Result<AcquireOutcome, AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.acquire(workspace_id, name, holder)
        })
    }

    /// Semaphore permit 반환.
    pub(crate) fn semaphore_release(
        &self,
        workspace_id: u32,
        name: &str,
        holder: &str,
    ) -> Result<ReleaseOutcome, AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.release(workspace_id, name, holder)
        })
    }

    /// Workspace 내 모든 Semaphore 나열.
    pub(crate) fn semaphore_list(&self, workspace_id: u32) -> Result<Vec<Semaphore>, AgentError> {
        self.with_memory(|mem| {
            let store = SemaphoreStore::new(mem, HOST_OWNER);
            store.list(workspace_id)
        })
    }

    /// Semaphore 삭제. 존재하지 않으면 no-op.
    pub(crate) fn semaphore_delete(&self, workspace_id: u32, name: &str) -> Result<(), AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.delete(workspace_id, name)
        })
    }
}
