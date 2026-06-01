//! Lease store wrapper. handler 의 `core.with_memory + LeaseStore::new` 조립을
//! 본 모듈로 흡수.

use tasty_agent::lease::{AcquireOutcome, ReleaseOutcome};
use tasty_agent::{AgentError, Lease, LeaseMode, LeaseStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

impl Core {
    /// Lease 점유 시도.
    pub(crate) fn lease_acquire(
        &self,
        workspace_id: u32,
        resource: &str,
        holder: &str,
        ttl_ms: Option<u64>,
        mode: LeaseMode,
        now_ms: u64,
    ) -> Result<AcquireOutcome, AgentError> {
        self.with_memory(|mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            store.acquire(workspace_id, resource, holder, ttl_ms, mode, now_ms)
        })
    }

    /// Lease 반환.
    pub(crate) fn lease_release(
        &self,
        workspace_id: u32,
        resource: &str,
        holder: &str,
    ) -> Result<ReleaseOutcome, AgentError> {
        self.with_memory(|mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            store.release(workspace_id, resource, holder)
        })
    }

    /// Lease 목록 (만료 evict 포함이므로 mut store).
    pub(crate) fn lease_list(
        &self,
        workspace_id: u32,
        now_ms: u64,
    ) -> Result<Vec<Lease>, AgentError> {
        self.with_memory(|mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            store.list(workspace_id, Some(now_ms))
        })
    }
}
