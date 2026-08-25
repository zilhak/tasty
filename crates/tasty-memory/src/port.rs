//! `MemoryStorage` trait — Hexagonal architecture 의 *internal port*.
//!
//! `MemoryStore` 가 자체 impl. Core 가 `Arc<Mutex<dyn MemoryStorage>>` 또는 *single-thread*
//! 시 `&mut dyn MemoryStorage` 형식으로 보유한다. test 시 `testing::InMemoryStorage`
//! 로 swap.
//!
//! 위치 결정: `tasty-memory` 가 internal crate (워크스페이스) 라 *trait 정의도 crate
//! 안*. bin 의 wrap layer 회피.

use crate::{
    ImportStats, ListOpts, MemoryChange, MemoryConfig, MemoryEntry, MemoryStats, MemoryValue,
    PurgeStats, PutOpts, Result, Scope,
};

/// Memory store 의 동작 인터페이스. `MemoryStore` 와 mock 모두 impl.
///
/// `Sync` 아님 — `MemoryStore` 내부의 SQLite `Connection` 이 `!Sync` (`RefCell` 캐시).
/// 호출자가 `Arc<Mutex<dyn MemoryStorage>>` 또는 single-thread 보유.
///
/// Blackboard / Cache / Plan sub-system 의 함수들은 *별 free function* (`crate::blackboard::*`
/// 등) — `&mut dyn MemoryStorage` 받으면 자연 동작.
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

    // ─── Maintenance ───
    /// `prefix` 아래 로그 키 중 최근 `keep_recent` 개만 남기고 삭제(개수 상한).
    ///
    /// 로그 retention 이 **부팅 경로와 런타임 경로 양쪽**에서 집행돼야 해서 port 에
    /// 있다. 부팅만 있으면 재시작 전까지 무제한으로 자라고, 런타임만 있으면 트래픽이
    /// 끊긴 인스턴스에 이미 쌓인 것이 영원히 남는다. 두 경로가 같은 구현을 부르도록
    /// 여기에 둔다.
    fn prune_prefix_keep_recent(&mut self, prefix: &str, keep_recent: u64) -> Result<u64>;
    /// `prefix` 아래 로그 키 중 `{ts:013}` 이 `cutoff_ms` 미만인 것을 삭제(시간 상한).
    fn prune_prefix_older_than(&mut self, prefix: &str, cutoff_ms: u64) -> Result<u64>;
    fn purge_expired(&mut self) -> Result<PurgeStats>;
    fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats>;
    fn take_pending_changes(&mut self) -> Vec<MemoryChange>;
}
