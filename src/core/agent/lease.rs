//! Core의 메모리 저장소에서 자원 점유를 관리한다.

use tasty_agent::lease::{AcquireOutcome, ReleaseOutcome};
use tasty_agent::{AgentError, Lease, LeaseMode, LeaseStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

impl Core {
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

    /// 목록을 읽으면서 만료된 점유도 제거한다.
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
