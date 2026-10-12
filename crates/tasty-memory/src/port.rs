//! 호스트가 MemoryStore 또는 시험용 InMemoryStorage를 사용하는 공통 인터페이스.

use crate::{
    BatchMode, BatchResult, ImportStats, ListOpts, MemoryChange, MemoryConfig, MemoryEntry,
    MemoryStats, MemoryValue, PurgeStats, PutOpts, Result, Scope, WriteOp,
};

/// 공유 저장소 락의 poison 보고 이름. 모든 소비자가 같은 이름과 보고 플래그를 써
/// 한 poison을 중복 보고하지 않는다. 복구는 호출자가 recover_mutex로 수행한다.
pub const STORE_LOCK_WHAT: &str = "memory store";

/// 저장소 락 poison의 최초 보고 여부.
pub static STORE_LOCK_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// SQLite Connection이 Sync가 아니므로 호출자가 mutex로 보호하거나 한 스레드에서 사용한다.
/// Blackboard·Cache·Plan 함수도 이 trait을 받는다.
pub trait MemoryStorage: Send {
    // ─── Config ───
    fn config(&self) -> &MemoryConfig;

    // ─── Regular: 공유 네임스페이스 ───
    fn put(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64>;

    fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>>;
    fn exists(&self, scope: &Scope, key: &str) -> Result<bool>;
    fn delete(&mut self, owner: &str, scope: &Scope, key: &str, cas: Option<u64>) -> Result<()>;
    fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>>;
    fn count(&self, scope: &Scope, prefix: Option<&str>) -> Result<u64>;
    fn scopes(&self) -> Result<Vec<String>>;
    fn stats(&self, scope: Option<&Scope>) -> Result<MemoryStats>;
    fn query(
        &self,
        scope: &Scope,
        path: &str,
        expected: &serde_json::Value,
        opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>>;
    fn export_regular(&self, scope: Option<&Scope>) -> Result<Vec<MemoryEntry>>;
    fn import_regular(
        &mut self,
        caller_owner: &str,
        entries: &[MemoryEntry],
        replace: bool,
    ) -> Result<ImportStats>;

    // ─── Secret: plugin 별 분할 네임스페이스 ───
    fn put_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64>;
    fn get_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>>;
    fn exists_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<bool>;
    fn delete_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        cas: Option<u64>,
    ) -> Result<()>;
    fn list_secret(&self, owner: &str, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>>;
    fn count_secret(&self, owner: &str, scope: &Scope, prefix: Option<&str>) -> Result<u64>;
    fn scopes_secret(&self, owner: &str) -> Result<Vec<String>>;
    fn stats_secret(&self, owner: &str, scope: Option<&Scope>) -> Result<MemoryStats>;

    // ─── 실행 단위·쓰기 묶음 ───
    /// 쓰기 묶음을 transaction 하나로 확정한다. 판정 규칙은 `MemoryStore::write_batch`.
    fn write_batch(&mut self, mode: BatchMode, ops: &[WriteOp]) -> BatchResult;
    /// 본문을 실행 단위 하나(직렬화·commit 단위)로 실행한다. 규칙은 `MemoryStore::run_unit`.
    /// 값을 돌려받으려면 `with_unit`을 쓴다.
    fn run_unit(
        &mut self,
        body: &mut dyn FnMut(&mut dyn MemoryStorage) -> Result<()>,
    ) -> Result<()>;

    // ─── Maintenance ───
    /// 로그 키의 최근 keep_recent개만 남긴다. 부팅과 실행 중 정리가 같은 구현을 사용한다.
    fn prune_prefix_keep_recent(&mut self, prefix: &str, keep_recent: u64) -> Result<u64>;
    /// `prefix` 아래 로그 키 중 `{ts:013}` 이 `cutoff_ms` 미만인 것을 삭제(시간 상한).
    fn prune_prefix_older_than(&mut self, prefix: &str, cutoff_ms: u64) -> Result<u64>;
    fn purge_expired(&mut self) -> Result<PurgeStats>;
    fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats>;
    fn take_pending_changes(&mut self) -> Vec<MemoryChange>;
}
