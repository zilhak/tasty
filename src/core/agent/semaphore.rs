//! Core의 메모리 저장소에서 동시 점유 수를 제한한다.

use tasty_agent::{AcquireOutcome, AgentError, ReleaseOutcome, Semaphore, SemaphoreStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

impl Core {
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

    /// 한도를 줄여도 기존 점유자를 강제로 내보내지 않는다.
    pub(crate) fn semaphore_set_permits(
        &self,
        workspace_id: u32,
        name: &str,
        permits: u32,
        now_ms: u64,
    ) -> Result<Semaphore, AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.set_permits(workspace_id, name, permits, now_ms)
        })
    }

    /// ttl_ms가 있으면 만료 시각을 저장하고 이후 acquire·list 등에서 회수한다.
    pub(crate) fn semaphore_acquire(
        &self,
        workspace_id: u32,
        name: &str,
        holder: &str,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<AcquireOutcome, AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.acquire(workspace_id, name, holder, ttl_ms, now_ms)
        })
    }

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

    /// 목록을 읽으면서 만료된 점유자를 제거한다.
    pub(crate) fn semaphore_list(
        &self,
        workspace_id: u32,
        now_ms: u64,
    ) -> Result<Vec<Semaphore>, AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.list(workspace_id, Some(now_ms))
        })
    }

    pub(crate) fn semaphore_delete(&self, workspace_id: u32, name: &str) -> Result<(), AgentError> {
        self.with_memory(|mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store.delete(workspace_id, name)
        })
    }
}
