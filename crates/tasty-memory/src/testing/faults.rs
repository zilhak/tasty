//! 정한 연산에만 저장 실패를 내는 시험 저장소. 실제 SQLite 저장소(메모리 DB)를 감싼다.
//!
//! 실행 단위 안의 연산도 같은 규칙으로 실패하도록 `run_unit` 본문에 감싼 저장소를 넘긴다. writer는
//! 운영 경로와 같은 분류(저장 실패·결과 불명)로 결과를 정한다. `test-support` 기능(또는 이 크레이트의
//! 시험)에서만 있다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::{
    BatchMode, BatchResult, ImportStats, ListOpts, MemoryChange, MemoryConfig, MemoryEntry,
    MemoryError, MemoryStats, MemoryStorage, MemoryStore, MemoryValue, PurgeStats, PutOpts, Result,
    Scope, WriteOp,
};

/// 남은 실패 횟수. 0이면 실패를 내지 않는다. 접두어는 만들 때 정한다.
#[derive(Default)]
pub struct Faults {
    /// `read_prefix`로 시작하는 키의 `get`.
    pub key_reads: AtomicU32,
    read_prefix: Option<String>,
    /// `list_prefix`로 시작하는 접두어 목록 읽기.
    pub prefix_lists: AtomicU32,
    list_prefix: Option<String>,
    /// 본문을 실행하기 전에 실행 단위를 실패시킨다. 단위는 시작하지 않았으므로 되돌려진다.
    pub units_before_body: AtomicU32,
    /// 본문이 모두 성공한 뒤 실행 단위를 실패시킨다. commit 단계일 수 있어 writer는 결과를 모른다.
    pub units_after_body: AtomicU32,
    /// `write_prefix`로 시작하는 키를 쓴 실행 단위를 본문 뒤에 실패시킨다(결과 불명).
    pub units_after_writing: AtomicU32,
    write_prefix: Option<String>,
}

impl Faults {
    pub fn arm(counter: &AtomicU32, times: u32) {
        counter.store(times, Ordering::SeqCst);
    }

    /// `key_reads`가 대상으로 삼는 키 접두어.
    pub fn reading(mut self, prefix: &str) -> Self {
        self.read_prefix = Some(prefix.to_string());
        self
    }

    /// `prefix_lists`가 대상으로 삼는 목록 접두어.
    pub fn listing(mut self, prefix: &str) -> Self {
        self.list_prefix = Some(prefix.to_string());
        self
    }

    /// `units_after_writing`이 대상으로 삼는 키 접두어.
    pub fn writing(mut self, prefix: &str) -> Self {
        self.write_prefix = Some(prefix.to_string());
        self
    }

    fn take(counter: &AtomicU32) -> bool {
        counter
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }

    fn reads(&self, key: &str) -> bool {
        self.read_prefix
            .as_deref()
            .is_some_and(|p| key.starts_with(p))
    }

    fn lists(&self, opts: &ListOpts) -> bool {
        match (self.list_prefix.as_deref(), opts.prefix.as_deref()) {
            (Some(target), Some(prefix)) => prefix.starts_with(target),
            _ => false,
        }
    }

    fn writes(&self, key: &str) -> bool {
        self.write_prefix
            .as_deref()
            .is_some_and(|p| key.starts_with(p))
    }
}

/// `SQLITE_IOERR` 저장 실패.
fn io_failure() -> MemoryError {
    MemoryError::Db(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
        Some("injected storage failure".into()),
    ))
}

pub struct FaultyStore {
    inner: Box<MemoryStore>,
    faults: Arc<Faults>,
}

impl FaultyStore {
    pub fn new(faults: Arc<Faults>) -> Self {
        Self {
            inner: Box::new(MemoryStore::open_in_memory().expect("in-memory sqlite")),
            faults,
        }
    }

    /// 실행 단위 밖의 쓰기는 단위 실패 대상이 아니다.
    fn note_write(&self, _key: &str) {}
}

struct FaultyView<'a> {
    inner: &'a mut dyn MemoryStorage,
    faults: &'a Faults,
    wrote: &'a AtomicBool,
}

impl FaultyView<'_> {
    fn note_write(&self, key: &str) {
        if self.faults.writes(key) {
            self.wrote.store(true, Ordering::SeqCst);
        }
    }
}

macro_rules! faulty_storage {
    ($ty:ty) => {
        impl MemoryStorage for $ty {
            fn config(&self) -> &MemoryConfig {
                self.inner.config()
            }
            fn put(
                &mut self,
                owner: &str,
                scope: &Scope,
                key: &str,
                value: &MemoryValue,
                opts: &PutOpts,
            ) -> Result<u64> {
                self.note_write(key);
                self.inner.put(owner, scope, key, value, opts)
            }
            fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
                if self.faults.reads(key) && Faults::take(&self.faults.key_reads) {
                    return Err(io_failure());
                }
                self.inner.get(scope, key)
            }
            fn exists(&self, scope: &Scope, key: &str) -> Result<bool> {
                self.inner.exists(scope, key)
            }
            fn delete(
                &mut self,
                owner: &str,
                scope: &Scope,
                key: &str,
                cas: Option<u64>,
            ) -> Result<()> {
                self.note_write(key);
                self.inner.delete(owner, scope, key, cas)
            }
            fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
                if self.faults.lists(opts) && Faults::take(&self.faults.prefix_lists) {
                    return Err(io_failure());
                }
                self.inner.list(scope, opts)
            }
            fn count(&self, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
                self.inner.count(scope, prefix)
            }
            fn scopes(&self) -> Result<Vec<String>> {
                self.inner.scopes()
            }
            fn stats(&self, scope: Option<&Scope>) -> Result<MemoryStats> {
                self.inner.stats(scope)
            }
            fn query(
                &self,
                scope: &Scope,
                path: &str,
                expected: &serde_json::Value,
                opts: &ListOpts,
            ) -> Result<Vec<MemoryEntry>> {
                self.inner.query(scope, path, expected, opts)
            }
            fn export_regular(&self, scope: Option<&Scope>) -> Result<Vec<MemoryEntry>> {
                self.inner.export_regular(scope)
            }
            fn import_regular(
                &mut self,
                caller_owner: &str,
                entries: &[MemoryEntry],
                replace: bool,
            ) -> Result<ImportStats> {
                self.inner.import_regular(caller_owner, entries, replace)
            }
            fn put_secret(
                &mut self,
                owner: &str,
                scope: &Scope,
                key: &str,
                value: &MemoryValue,
                opts: &PutOpts,
            ) -> Result<u64> {
                self.inner.put_secret(owner, scope, key, value, opts)
            }
            fn get_secret(
                &self,
                owner: &str,
                scope: &Scope,
                key: &str,
            ) -> Result<Option<MemoryEntry>> {
                self.inner.get_secret(owner, scope, key)
            }
            fn exists_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<bool> {
                self.inner.exists_secret(owner, scope, key)
            }
            fn delete_secret(
                &mut self,
                owner: &str,
                scope: &Scope,
                key: &str,
                cas: Option<u64>,
            ) -> Result<()> {
                self.inner.delete_secret(owner, scope, key, cas)
            }
            fn list_secret(
                &self,
                owner: &str,
                scope: &Scope,
                opts: &ListOpts,
            ) -> Result<Vec<MemoryEntry>> {
                self.inner.list_secret(owner, scope, opts)
            }
            fn count_secret(
                &self,
                owner: &str,
                scope: &Scope,
                prefix: Option<&str>,
            ) -> Result<u64> {
                self.inner.count_secret(owner, scope, prefix)
            }
            fn scopes_secret(&self, owner: &str) -> Result<Vec<String>> {
                self.inner.scopes_secret(owner)
            }
            fn stats_secret(&self, owner: &str, scope: Option<&Scope>) -> Result<MemoryStats> {
                self.inner.stats_secret(owner, scope)
            }
            fn write_batch(&mut self, mode: BatchMode, ops: &[WriteOp]) -> BatchResult {
                for op in ops {
                    match op {
                        WriteOp::Put { key, .. }
                        | WriteOp::Delete { key, .. }
                        | WriteOp::PutSecret { key, .. }
                        | WriteOp::DeleteSecret { key, .. } => self.note_write(key),
                    }
                }
                self.inner.write_batch(mode, ops)
            }
            fn run_unit(
                &mut self,
                body: &mut dyn FnMut(&mut dyn MemoryStorage) -> Result<()>,
            ) -> Result<()> {
                if Faults::take(&self.faults.units_before_body) {
                    return Err(io_failure());
                }
                let faults: &Faults = &self.faults;
                // MemoryStore에는 이름이 같은 고유 메서드가 있어 trait 메서드로 부른다.
                let inner: &mut dyn MemoryStorage = &mut *self.inner;
                inner.run_unit(&mut |s| {
                    let wrote = AtomicBool::new(false);
                    body(&mut FaultyView {
                        inner: s,
                        faults,
                        wrote: &wrote,
                    })?;
                    if Faults::take(&faults.units_after_body)
                        || (wrote.load(Ordering::SeqCst)
                            && Faults::take(&faults.units_after_writing))
                    {
                        return Err(io_failure());
                    }
                    Ok(())
                })
            }
            fn prune_prefix_keep_recent(&mut self, prefix: &str, keep_recent: u64) -> Result<u64> {
                self.inner.prune_prefix_keep_recent(prefix, keep_recent)
            }
            fn prune_prefix_older_than(&mut self, prefix: &str, cutoff_ms: u64) -> Result<u64> {
                self.inner.prune_prefix_older_than(prefix, cutoff_ms)
            }
            fn purge_expired(&mut self) -> Result<PurgeStats> {
                self.inner.purge_expired()
            }
            fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats> {
                self.inner.purge_scope(scope)
            }
            fn take_pending_changes(&mut self) -> Vec<MemoryChange> {
                self.inner.take_pending_changes()
            }
        }
    };
}

faulty_storage!(FaultyStore);
faulty_storage!(FaultyView<'_>);
