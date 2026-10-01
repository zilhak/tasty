//! journal 열기·writer 세대·읽기.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::error::{StoreError, StoreResult};
use crate::schema;
use crate::types::{JournalCut, StreamId, WriterEpoch};

/// 다른 연결이 쓰는 동안 기다리는 시간. 넘으면 SQLITE_BUSY 오류로 돌려준다.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// writer 잠금을 다시 시도하는 총 시간과 간격. 자식 프로세스는 fork·spawn부터 exec까지 잠금 파일의
/// 열린 파일 설명을 공유하므로, 방금 놓은 잠금이 그동안 남아 있을 수 있다.
const WRITER_LOCK_WAIT: Duration = Duration::from_secs(2);
const WRITER_LOCK_FIRST_BACKOFF: Duration = Duration::from_millis(10);
const WRITER_LOCK_MAX_BACKOFF: Duration = Duration::from_millis(100);

/// writer 잠금 파일 이름에 붙이는 접미사. journal 파일과 같은 디렉터리에 둔다.
const WRITER_LOCK_SUFFIX: &str = ".writer-lock";

/// 한 journal 파일의 저장소. 연결 하나를 가지며 Sync가 아니다.
///
/// 쓰기는 [`EventStore::acquire_writer`]로 독점 파일 잠금과 writer 세대를 얻은 저장소만 한다.
/// 잠금은 다른 프로세스·다른 저장소를 막고, 세대는 같은 저장소 안의 이전 writer·worker가
/// 늦게 보낸 쓰기를 [`StoreError::Fenced`]로 막는다. 잠금 없이 연 저장소는 읽기만 한다.
pub struct EventStore {
    pub(crate) conn: Connection,
    pub(crate) database_path:PathBuf,
    pub(crate) admission_budget:crate::AdmissionBudget,
    journal_id: String,
    lock_path: PathBuf,
    /// 이 저장소가 가진 writer 잠금. drop하면 OS가 잠금을 푼다.
    writer_lock: Option<File>,
}

impl EventStore {
    /// journal 파일을 열거나 만든다. 파일에 다른 journal_id가 있으면 열지 않는다.
    /// 최근 파일이나 다른 식별로 대상을 추측하지 않기 위해서다.
    pub fn open(path: &Path, journal_id: &str) -> StoreResult<Self> {
        Self::open_with_admission_budget(path,journal_id,crate::AdmissionBudget::default())
    }

    /// Configure internal admission accounting without changing durable schema or imposing a
    /// hard SQLite page limit on already accepted effects and maintenance.
    pub fn open_with_admission_budget(path:&Path,journal_id:&str,budget:crate::AdmissionBudget)->StoreResult<Self> {
        budget.validate()?;
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // 파일을 바꾸는 설정·migration보다 먼저 journal인지 확인한다.
        schema::ensure_journal_or_empty(&conn)?;
        schema::apply_durability(&conn, BUSY_TIMEOUT)?;
        // 버전 확인·migration·journal 바인딩을 한 transaction으로 묶는다. 나눠 두면 동시에 처음
        // 여는 연결이 서로의 중간 상태를 보고 같은 migration을 다시 적용하다 실패한다.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // 첫 검사 뒤 다른 연결이 이 파일에 무엇을 만들었을 수 있어 transaction 안에서 다시 본다.
        schema::ensure_journal_or_empty(&tx)?;
        schema::migrate(&tx)?;
        bind_journal(&tx, journal_id)?;
        tx.commit()?;
        Ok(Self {
            database_path:path.to_owned(),admission_budget:budget,
            conn,
            journal_id: journal_id.to_owned(),
            lock_path: writer_lock_path(path),
            writer_lock: None,
        })
    }

    pub fn journal_id(&self) -> &str {
        &self.journal_id
    }

    /// 독점 writer 잠금을 얻고 새 writer 세대를 등록한다. 이후 이전 세대의 쓰기는 거절된다.
    /// 잠금이 잡혀 있으면 최대 2초 동안 다시 시도하고, 그래도 잡혀 있으면 [`StoreError::WriterLocked`],
    /// 잠금을 쓸 수 없는 환경이면 [`StoreError::WriterLockUnavailable`]로 실패하며 writer가 되지 않는다.
    /// 이미 잠금을 가진 저장소가 다시 부르면 세대만 올린다.
    pub fn acquire_writer(&mut self) -> StoreResult<WriterEpoch> {
        let fresh_lock = match self.writer_lock {
            Some(_) => None,
            None => Some(lock_exclusive_with_retry(&self.lock_path)?),
        };
        let epoch = self.register_epoch()?;
        if let Some(lock) = fresh_lock {
            self.writer_lock = Some(lock);
        }
        Ok(epoch)
    }

    /// writer 잠금을 놓는다. 이후 이 저장소의 쓰기는 [`StoreError::NotWriter`]로 거절된다.
    pub fn release_writer(&mut self) {
        self.writer_lock = None;
    }

    pub fn is_writer(&self) -> bool {
        self.writer_lock.is_some()
    }

    fn register_epoch(&mut self) -> StoreResult<WriterEpoch> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_active(&tx)?;
        let next = read_epoch(&tx)?.0 + 1;
        tx.execute(
            "UPDATE journal_meta SET writer_epoch = ?1 WHERE singleton = 1",
            [to_i64(next)?],
        )?;
        tx.commit()?;
        Ok(WriterEpoch(next))
    }

    /// 현재 등록된 writer 세대.
    pub fn current_writer(&self) -> StoreResult<WriterEpoch> {
        read_epoch(&self.conn)
    }

    /// journal을 archived로 바꾼다. 이후 쓰기는 거절되고 읽기는 유지된다.
    pub fn archive(&mut self, epoch: WriterEpoch) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        tx.execute(
            "UPDATE journal_meta SET status = 'archived' WHERE singleton = 1",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn is_archived(&self) -> StoreResult<bool> {
        Ok(read_status(&self.conn)? == "archived")
    }

    /// stream의 마지막 확정 revision. 이벤트가 없으면 `None`이다.
    pub fn stream_revision(&self, stream: &StreamId) -> StoreResult<Option<u64>> {
        stream_head(&self.conn, stream)
    }

    /// 마지막 batch와 모든 stream head를 한 read transaction에서 읽는다.
    pub fn current_cut(&self) -> StoreResult<JournalCut> {
        let tx = self.conn.unchecked_transaction()?;
        let last_batch: Option<i64> =
            tx.query_row("SELECT MAX(batch_id) FROM batches", [], |r| r.get(0))?;
        let mut cut = JournalCut {
            last_batch: last_batch.map(to_u64).transpose()?,
            ..JournalCut::default()
        };
        let mut stmt = tx.prepare("SELECT stream_id, revision FROM stream_heads")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (stream, revision) = row?;
            cut.heads.insert(StreamId(stream), to_u64(revision)?);
        }
        Ok(cut)
    }

    /// writer 세대와 활성 여부를 확인한 쓰기 transaction을 연다.
    pub(crate) fn write_tx(&mut self, epoch: WriterEpoch) -> StoreResult<Transaction<'_>> {
        if self.writer_lock.is_none() {
            return Err(StoreError::NotWriter);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_active(&tx)?;
        let current = read_epoch(&tx)?;
        if current != epoch {
            return Err(StoreError::Fenced {
                presented: epoch,
                current,
            });
        }
        Ok(tx)
    }
}

fn writer_lock_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(WRITER_LOCK_SUFFIX);
    PathBuf::from(name)
}

fn lock_exclusive_with_retry(path: &Path) -> StoreResult<File> {
    let deadline = Instant::now() + WRITER_LOCK_WAIT;
    let mut backoff = WRITER_LOCK_FIRST_BACKOFF;
    loop {
        match lock_exclusive(path) {
            Err(StoreError::WriterLocked) if Instant::now() < deadline => {
                std::thread::sleep(backoff.min(deadline.saturating_duration_since(Instant::now())));
                backoff = (backoff * 2).min(WRITER_LOCK_MAX_BACKOFF);
            }
            other => return other,
        }
    }
}

/// 잠금 파일에 독점 잠금을 건다. 기다리지 않는다. SQLite 파일 자체를 잠그지 않는 이유는
/// Windows의 파일 잠금이 같은 파일에 대한 SQLite 자신의 읽기·쓰기까지 막기 때문이다.
/// 잠금 파일은 지우지 않는다. 지우면 다른 프로세스가 새 파일에 따로 잠금을 걸 수 있다.
fn lock_exclusive(path: &Path) -> StoreResult<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(StoreError::WriterLockUnavailable)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(StoreError::WriterLocked),
        Err(TryLockError::Error(err)) => Err(StoreError::WriterLockUnavailable(err)),
    }
}

fn bind_journal(conn: &Connection, journal_id: &str) -> StoreResult<()> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT journal_id FROM journal_meta WHERE singleton = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match stored {
        Some(stored) if stored != journal_id => {
            return Err(StoreError::JournalMismatch {
                requested: journal_id.to_owned(),
                stored,
            });
        }
        Some(_) => {}
        None => {
            conn.execute(
                "INSERT INTO journal_meta (singleton, journal_id, writer_epoch, status)
                 VALUES (1, ?1, 0, 'active')",
                [journal_id],
            )?;
        }
    }
    Ok(())
}

fn read_epoch(conn: &Connection) -> StoreResult<WriterEpoch> {
    let epoch: i64 = conn.query_row(
        "SELECT writer_epoch FROM journal_meta WHERE singleton = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(WriterEpoch(to_u64(epoch)?))
}

fn read_status(conn: &Connection) -> StoreResult<String> {
    Ok(conn.query_row(
        "SELECT status FROM journal_meta WHERE singleton = 1",
        [],
        |r| r.get(0),
    )?)
}

fn ensure_active(conn: &Connection) -> StoreResult<()> {
    if read_status(conn)? == "archived" {
        return Err(StoreError::JournalArchived);
    }
    Ok(())
}

pub(crate) fn stream_head(conn: &Connection, stream: &StreamId) -> StoreResult<Option<u64>> {
    let revision: Option<i64> = conn
        .query_row(
            "SELECT revision FROM stream_heads WHERE stream_id = ?1",
            [stream.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    revision.map(to_u64).transpose()
}

pub(crate) fn to_i64(value: u64) -> StoreResult<i64> {
    i64::try_from(value).map_err(|_| StoreError::Corrupt(format!("{value} exceeds i64")))
}

pub(crate) fn to_u64(value: i64) -> StoreResult<u64> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(format!("negative value {value}")))
}

pub(crate) fn to_u32(value: i64) -> StoreResult<u32> {
    u32::try_from(value).map_err(|_| StoreError::Corrupt(format!("{value} exceeds u32")))
}
