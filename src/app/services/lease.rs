//! Core의 메모리 저장소에서 자원 점유를 관리한다.

use tasty_agent::lease::{AcquireOutcome, ReleaseOutcome};
use tasty_agent::{AgentError, Lease, LeaseMode, LeaseStore};
use tasty_memory::HOST_OWNER;

pub(crate) fn acquire(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    resource: &str,
    holder: &str,
    ttl_ms: Option<u64>,
    mode: LeaseMode,
    now_ms: u64,
) -> Result<AcquireOutcome, AgentError> {
    let mut store = LeaseStore::new(mem, HOST_OWNER);
    store.acquire(workspace_id, resource, holder, ttl_ms, mode, now_ms)
}

pub(crate) fn release(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    resource: &str,
    holder: &str,
) -> Result<ReleaseOutcome, AgentError> {
    let mut store = LeaseStore::new(mem, HOST_OWNER);
    store.release(workspace_id, resource, holder)
}

/// 목록을 읽으면서 만료된 점유도 제거한다.
pub(crate) fn list(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    now_ms: u64,
) -> Result<Vec<Lease>, AgentError> {
    let mut store = LeaseStore::new(mem, HOST_OWNER);
    store.list(workspace_id, Some(now_ms))
}
