//! Core의 메모리 저장소로 barrier를 생성·조회·갱신한다.

use tasty_agent::{AgentError, Barrier, BarrierStore};
use tasty_memory::HOST_OWNER;

pub(crate) fn create(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    name: String,
    count_required: u32,
    timeout_ms: Option<u64>,
    now_ms: u64,
) -> Result<Barrier, AgentError> {
    let mut store = BarrierStore::new(mem, HOST_OWNER);
    store.create(workspace_id, name, count_required, timeout_ms, now_ms)
}

/// 신호를 더한다. 요구 수를 채우면 Closed 상태를 돌려주며, 이벤트는 호출자가 commit 뒤에 넣는다.
pub(crate) fn signal(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    name: &str,
    now_ms: u64,
) -> Result<Barrier, AgentError> {
    let mut store = BarrierStore::new(mem, HOST_OWNER);
    store.signal(workspace_id, name, now_ms)
}

/// 조회하면서 만료 상태도 저장할 수 있다.
pub(crate) fn state(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    name: &str,
    now_ms: u64,
) -> Result<Barrier, AgentError> {
    let mut store = BarrierStore::new(mem, HOST_OWNER);
    store.state(workspace_id, name, now_ms)
}

/// now_ms가 있으면 만료 상태를 반영한 목록을 반환한다.
pub(crate) fn list(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    now_ms: Option<u64>,
) -> Result<Vec<Barrier>, AgentError> {
    let mut store = BarrierStore::new(mem, HOST_OWNER);
    store.list(workspace_id, now_ms)
}

pub(crate) fn delete(
    mem: &mut dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
    name: &str,
) -> Result<(), AgentError> {
    let mut store = BarrierStore::new(mem, HOST_OWNER);
    store.delete(workspace_id, name)
}
