//! 여러 쓰기를 SQLite transaction 하나로 확정하는 실행 단위와 쓰기 묶음.
//!
//! 실행 단위(`run_unit`)는 직렬화와 commit의 단위다. 단위 안의 각 쓰기는 savepoint로 감싸
//! 거절된 쓰기만 되돌리고 앞선 쓰기는 남긴다. 그래서 여러 번 쓰는 복합 연산을 단위 안에서
//! 실행해도 단독 실행과 같은 부분 반영 의미를 유지하고 commit만 하나로 합친다.
//! 쓰기 묶음(`write_batch`)은 원자 묶음(하나라도 거절되면 전부 되돌림)과 독립 요청 묶음
//! (요청별 판정)을 제공한다.
//!
//! 저장 실패(`MemoryError::Db`)는 거절과 다르게 다룬다. SQLite는 일부 오류에서 transaction을
//! 스스로 되돌리므로 쓰기·읽기에서 저장 실패가 난 단위는 그 뒤의 쓰기를 실행하지 않고 전체를
//! 되돌린다. 단위가 열려 있는데 연결이 autocommit 상태가 됐으면(SQLite가 transaction을 끝냈다)
//! 같은 저장 실패로 다룬다. 그대로 쓰면 savepoint가 새 transaction을 열어 바로 commit하기 때문이다.
//! quota 카운터와 변경 버퍼는 commit이 성공한 뒤에만 공개한다.
//! 계약과 계측 규칙은 docs/design/systems/storage-contract.md의 "실행 단위와 쓰기 묶음" 절에 있다.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use rusqlite::Connection;

use crate::{
    MemoryChange, MemoryError, MemoryStorage, MemoryStore, MemoryValue, PutOpts, Result, Scope,
};

/// 묶음에 넣는 쓰기 요청 하나. 각 요청은 같은 이름의 단독 메서드와 같은 판정을 받는다.
#[derive(Debug, Clone)]
pub enum WriteOp {
    Put {
        owner: String,
        scope: Scope,
        key: String,
        value: MemoryValue,
        opts: PutOpts,
    },
    Delete {
        owner: String,
        scope: Scope,
        key: String,
        cas: Option<u64>,
    },
    PutSecret {
        owner: String,
        scope: Scope,
        key: String,
        value: MemoryValue,
        opts: PutOpts,
    },
    DeleteSecret {
        owner: String,
        scope: Scope,
        key: String,
        cas: Option<u64>,
    },
}

/// 요청 하나의 성공 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutput {
    /// put 계열의 새 version.
    Version(u64),
    /// delete 계열의 성공.
    Deleted,
}

/// 쓰기 묶음의 판정 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchMode {
    /// 하나라도 거절되면 묶음 전체를 되돌린다.
    Atomic,
    /// 요청마다 판정한다. 거절된 요청만 결과가 오류이고 나머지는 앞 요청의 반영을 본 채
    /// 실행된다.
    Independent,
}

/// 묶음 전체가 확정되지 않았다. 어떤 요청도 반영되지 않았다.
#[derive(Debug, thiserror::Error)]
#[error("memory batch not committed (request {index:?}): {error}")]
pub struct BatchFailure {
    /// 원인 요청의 위치. transaction 시작·commit 실패처럼 특정 요청 탓이 아니면 None이다.
    pub index: Option<usize>,
    pub error: MemoryError,
}

/// 확정된 묶음의 요청별 결과(요청 순서) 또는 묶음 전체의 실패.
pub type BatchResult = std::result::Result<Vec<Result<WriteOutput>>, BatchFailure>;

/// 쓰기 본문이 돌려주는 bookkeeping. 단위가 commit된 뒤에만 저장소에 공개한다.
#[derive(Debug, Default)]
pub(crate) struct Effect {
    /// 새 regular 사용량. None이면 그대로다.
    pub regular_used: Option<i64>,
    pub changes: Vec<MemoryChange>,
    /// commit 계측에 셀 쓰기가 있었는지.
    pub wrote: bool,
}

impl Effect {
    pub(crate) fn secret_write() -> Self {
        Self {
            regular_used: None,
            changes: Vec::new(),
            wrote: true,
        }
    }
}

/// 열린 실행 단위가 아직 공개하지 않은 bookkeeping.
#[derive(Debug)]
pub(crate) struct UnitStage {
    regular_used: i64,
    changes: Vec<MemoryChange>,
    writes: u64,
    broken: Option<Broken>,
}

/// 단위를 더 진행할 수 없게 만든 첫 저장 실패.
#[derive(Debug, Clone)]
pub(crate) struct Broken {
    code: Option<rusqlite::ffi::Error>,
    message: String,
}

impl Broken {
    fn from_error(err: &rusqlite::Error) -> Self {
        let code = match err {
            rusqlite::Error::SqliteFailure(code, _) => Some(*code),
            _ => None,
        };
        Self {
            code,
            message: err.to_string(),
        }
    }

    /// 단위가 열려 있는데 SQLite가 transaction을 이미 끝낸 상태.
    fn transaction_ended() -> Self {
        Self {
            code: None,
            message: "the unit's transaction was ended by SQLite".into(),
        }
    }

    /// 원래 오류와 같은 저장 실패 분류를 갖는 오류. 첫 오류는 그 요청이 이미 받았다.
    fn to_error(&self) -> MemoryError {
        let code = self
            .code
            .unwrap_or_else(|| rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR));
        MemoryError::Db(rusqlite::Error::SqliteFailure(
            code,
            Some(format!(
                "memory unit aborted by an earlier storage failure: {}",
                self.message
            )),
        ))
    }
}

/// 본문이 오류를 돌려줬을 때 단위가 할 일.
#[derive(Clone, Copy)]
enum BodyErr {
    /// 단위 전체를 되돌린다.
    Rollback,
    /// 이미 반영된 쓰기를 commit하고 본문의 오류를 돌려준다.
    KeepApplied,
}

const STEP_SAVEPOINT: &str = "tasty_step";
const BATCH_SAVEPOINT: &str = "tasty_batch";

impl MemoryStore {
    /// 본문을 실행 단위 하나로 실행한다. 본문 안의 쓰기는 transaction 하나를 함께 쓰고
    /// 본문이 끝난 뒤 한 번 commit된다. 본문 안의 읽기는 앞선 쓰기를 본다.
    ///
    /// 거절된 쓰기는 그 쓰기만 되돌리므로, 본문이 오류를 돌려줘도 그전까지 반영된 쓰기는
    /// commit된다(단독 실행의 부분 반영과 같다). 쓰기나 읽기에서 저장 실패가 한 번이라도 나면
    /// 단위 전체를 되돌리고 저장 실패를 돌려준다. commit 실패도 같다. 단위가 이미 열려 있으면 그 단위에
    /// 합류한다.
    pub fn run_unit<T>(&mut self, body: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.in_unit(BodyErr::KeepApplied, body)
    }

    /// 쓰기 묶음을 transaction 하나로 실행한다. 확정되면 요청별 결과를 요청 순서로 돌려준다.
    /// 같은 키의 연속 요청은 앞 요청의 반영을 보고 판정한다(version·CAS·owner·quota·TTL).
    /// 저장 실패와 commit 실패는 묶음 전체를 되돌리고 `BatchFailure`로 알린다.
    /// 원자 묶음의 거절도 `BatchFailure`이며 index가 거절된 요청을 가리킨다.
    pub fn write_batch(&mut self, mode: BatchMode, ops: &[WriteOp]) -> BatchResult {
        let joined = self.unit.is_some();
        let mut failed_at = None;
        let out = self.in_unit(BodyErr::Rollback, |s| {
            s.batch_body(mode, ops, joined, &mut failed_at)
        });
        out.map_err(|error| BatchFailure {
            index: failed_at,
            error,
        })
    }

    fn batch_body(
        &mut self,
        mode: BatchMode,
        ops: &[WriteOp],
        joined: bool,
        failed_at: &mut Option<usize>,
    ) -> Result<Vec<Result<WriteOutput>>> {
        // 이미 열린 단위에 합류한 원자 묶음은 묶음만 되돌릴 savepoint가 필요하다.
        let mark = if joined && mode == BatchMode::Atomic {
            Some(self.open_savepoint(BATCH_SAVEPOINT)?)
        } else {
            None
        };
        let mut outs = Vec::with_capacity(ops.len());
        for (i, op) in ops.iter().enumerate() {
            match self.apply_op(op) {
                Ok(o) => outs.push(Ok(o)),
                Err(e) if e.storage_failure().is_some() => {
                    *failed_at = Some(i);
                    return Err(e);
                }
                Err(e) if mode == BatchMode::Atomic => {
                    *failed_at = Some(i);
                    if let Some(mark) = mark {
                        self.rollback_savepoint(BATCH_SAVEPOINT, mark);
                    }
                    return Err(e);
                }
                Err(e) => outs.push(Err(e)),
            }
        }
        if mark.is_some() {
            self.release_savepoint(BATCH_SAVEPOINT)?;
        }
        Ok(outs)
    }

    fn apply_op(&mut self, op: &WriteOp) -> Result<WriteOutput> {
        match op {
            WriteOp::Put {
                owner,
                scope,
                key,
                value,
                opts,
            } => self
                .put(owner, scope, key, value, opts)
                .map(WriteOutput::Version),
            WriteOp::Delete {
                owner,
                scope,
                key,
                cas,
            } => self
                .delete(owner, scope, key, *cas)
                .map(|()| WriteOutput::Deleted),
            WriteOp::PutSecret {
                owner,
                scope,
                key,
                value,
                opts,
            } => self
                .put_secret(owner, scope, key, value, opts)
                .map(WriteOutput::Version),
            WriteOp::DeleteSecret {
                owner,
                scope,
                key,
                cas,
            } => self
                .delete_secret(owner, scope, key, *cas)
                .map(|()| WriteOutput::Deleted),
        }
    }

    /// 쓰기 본문 하나. 열린 단위가 없으면 자기 transaction을 열고 commit한다.
    pub(crate) fn write_step<T>(
        &mut self,
        op: impl FnOnce(&Connection, i64) -> Result<(T, Effect)>,
    ) -> Result<T> {
        if self.unit.is_some() {
            return self.step(op, true);
        }
        self.in_unit(BodyErr::Rollback, |s| s.step(op, false))
    }

    /// 정리 본문 하나. 쓰기 본문과 같이 이벤트 대상 조회와 여러 DELETE를 transaction 하나로
    /// 확정하므로, 실패하면 아무것도 지우지 않고 카운터·변경 버퍼도 그대로다.
    pub(crate) fn maintenance_step<T>(
        &mut self,
        op: impl FnOnce(&Connection) -> Result<(T, Effect)>,
    ) -> Result<T> {
        self.write_step(|conn, _| op(conn))
    }

    /// 열린 단위 안에서 본문 하나를 실행하고 bookkeeping을 단위에 쌓는다.
    fn step<T>(
        &mut self,
        op: impl FnOnce(&Connection, i64) -> Result<(T, Effect)>,
        savepoint: bool,
    ) -> Result<T> {
        if let Some(broken) = self.unit_broken() {
            return Err(broken.to_error());
        }
        let used = match &self.unit {
            Some(stage) => stage.regular_used,
            None => self.regular_used_bytes,
        };
        let mark = if savepoint {
            Some(self.open_savepoint(STEP_SAVEPOINT)?)
        } else {
            None
        };
        match op(&self.conn, used) {
            Ok((value, effect)) => {
                if mark.is_some() {
                    self.release_savepoint(STEP_SAVEPOINT)?;
                }
                if let Some(stage) = self.unit.as_mut() {
                    if let Some(used) = effect.regular_used {
                        stage.regular_used = used;
                    }
                    stage.changes.extend(effect.changes);
                    if effect.wrote {
                        stage.writes += 1;
                    }
                }
                Ok(value)
            }
            Err(MemoryError::Db(e)) => {
                self.mark_broken(&e);
                Err(MemoryError::Db(e))
            }
            Err(e) => {
                if let Some(mark) = mark {
                    self.rollback_savepoint(STEP_SAVEPOINT, mark);
                }
                Err(e)
            }
        }
    }

    fn in_unit<T>(
        &mut self,
        on_body_err: BodyErr,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if self.unit.is_some() {
            return body(self);
        }
        self.begin_unit()?;
        let out = match catch_unwind(AssertUnwindSafe(|| body(&mut *self))) {
            Ok(out) => out,
            Err(panic) => {
                self.abort_unit();
                resume_unwind(panic);
            }
        };
        match (out, on_body_err) {
            (Ok(value), _) => self.commit_unit().map(|()| value),
            (Err(e), BodyErr::Rollback) => {
                self.abort_unit();
                Err(e)
            }
            (Err(e), BodyErr::KeepApplied) => match self.commit_unit() {
                Ok(()) => Err(e),
                // 본문이 받은 저장 실패가 원래 오류다.
                Err(_) if e.storage_failure().is_some() => Err(e),
                Err(commit_err) => Err(commit_err),
            },
        }
    }

    fn begin_unit(&mut self) -> Result<()> {
        if !self.conn.is_autocommit() {
            tracing::warn!("memory store: an unfinished transaction was rolled back");
            self.rollback_quietly();
        }
        self.conn.execute_batch("BEGIN")?;
        self.unit_read_failure.replace(None);
        self.unit = Some(UnitStage {
            regular_used: self.regular_used_bytes,
            changes: Vec::new(),
            writes: 0,
            broken: None,
        });
        Ok(())
    }

    /// 단위를 commit하고 성공했을 때만 bookkeeping을 공개한다.
    fn commit_unit(&mut self) -> Result<()> {
        let broken = self.unit_broken();
        let Some(stage) = self.unit.take() else {
            return Ok(());
        };
        if let Some(broken) = broken {
            self.rollback_quietly();
            return Err(broken.to_error());
        }
        let commit_started = std::time::Instant::now();
        if let Err(e) = self.conn.execute_batch("COMMIT") {
            // COMMIT이 실패해도 transaction이 열려 있을 수 있다(busy 등). 확정하지 않는다.
            self.rollback_quietly();
            return Err(e.into());
        }
        if stage.writes > 0 {
            self.db_latency.record_commit(commit_started.elapsed());
        }
        self.regular_used_bytes = stage.regular_used;
        self.pending_changes.extend(stage.changes);
        Ok(())
    }

    fn abort_unit(&mut self) {
        self.unit = None;
        self.rollback_quietly();
    }

    fn rollback_quietly(&mut self) {
        if self.conn.is_autocommit() {
            return;
        }
        if let Err(e) = self.conn.execute_batch("ROLLBACK") {
            tracing::warn!("memory store: rollback failed: {e}");
        }
    }

    /// 열린 단위가 더 진행할 수 없는 상태면 그 원인. 조회의 저장 실패와 SQLite가 끝낸
    /// transaction을 단위의 실패로 옮긴다.
    fn unit_broken(&mut self) -> Option<Broken> {
        let read_failure = self.unit_read_failure.borrow_mut().take();
        let autocommit = self.conn.is_autocommit();
        let stage = self.unit.as_mut()?;
        if stage.broken.is_none() {
            stage.broken = read_failure.or_else(|| autocommit.then(Broken::transaction_ended));
        }
        stage.broken.clone()
    }

    /// 열린 단위 안의 조회가 낸 저장 실패를 기록한다. 단위 밖에서는 결과를 그대로 돌려준다.
    pub(crate) fn record_read<T>(&self, result: Result<T>) -> Result<T> {
        if let Err(MemoryError::Db(e)) = &result
            && self.unit.is_some()
        {
            let mut slot = self.unit_read_failure.borrow_mut();
            if slot.is_none() {
                *slot = Some(Broken::from_error(e));
            }
        }
        result
    }

    fn mark_broken(&mut self, err: &rusqlite::Error) {
        if let Some(stage) = self.unit.as_mut()
            && stage.broken.is_none()
        {
            stage.broken = Some(Broken::from_error(err));
        }
    }

    /// savepoint를 열고 되돌릴 때 복원할 bookkeeping 위치를 돌려준다.
    fn open_savepoint(&mut self, name: &str) -> Result<StageMark> {
        if let Some(broken) = self.unit_broken() {
            return Err(broken.to_error());
        }
        if let Err(e) = self.conn.execute_batch(&format!("SAVEPOINT {name}")) {
            self.mark_broken(&e);
            return Err(e.into());
        }
        Ok(self
            .unit
            .as_ref()
            .map(|s| StageMark {
                regular_used: s.regular_used,
                changes: s.changes.len(),
                writes: s.writes,
            })
            .unwrap_or_default())
    }

    fn release_savepoint(&mut self, name: &str) -> Result<()> {
        if let Err(e) = self.conn.execute_batch(&format!("RELEASE {name}")) {
            self.mark_broken(&e);
            return Err(e.into());
        }
        Ok(())
    }

    fn rollback_savepoint(&mut self, name: &str, mark: StageMark) {
        if let Err(e) = self
            .conn
            .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"))
        {
            self.mark_broken(&e);
            return;
        }
        if let Some(stage) = self.unit.as_mut() {
            stage.regular_used = mark.regular_used;
            stage.changes.truncate(mark.changes);
            stage.writes = mark.writes;
        }
    }
}

/// savepoint를 열 때의 bookkeeping 위치.
#[derive(Debug, Default, Clone, Copy)]
struct StageMark {
    regular_used: i64,
    changes: usize,
    writes: u64,
}

/// `MemoryStorage::run_unit`을 값을 돌려주는 본문으로 쓰는 도우미.
/// 본문의 오류와 단위의 결과를 [`MemoryStore::run_unit`]과 같은 규칙으로 합친다.
pub fn with_unit<S, T>(
    store: &mut S,
    body: impl FnOnce(&mut dyn MemoryStorage) -> Result<T>,
) -> Result<T>
where
    S: MemoryStorage + ?Sized,
{
    let mut body = Some(body);
    let mut value = None;
    store.run_unit(&mut |s| {
        if let Some(body) = body.take() {
            value = Some(body(s)?);
        }
        Ok(())
    })?;
    match value {
        Some(value) => Ok(value),
        None => unreachable!("run_unit returned Ok without running its body"),
    }
}

/// 조회하면 행 변환 오류(`MemoryError::Db`)를 내는 행을 심는다. 다른 모듈의 시험이 단위 안 조회
/// 실패를 만들 때 쓴다.
#[cfg(test)]
pub(crate) fn plant_unreadable_row_for_tests(s: &MemoryStore) {
    batch_tests::plant_unreadable_row(s);
}

#[cfg(test)]
mod batch_tests;
