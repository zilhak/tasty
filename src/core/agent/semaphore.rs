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
}
