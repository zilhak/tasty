//! journal 열기·writer 세대·읽기.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::error::{StoreError, StoreResult};
use crate::schema;
use crate::types::{JournalCut, StreamId, WriterEpoch};

/// 다른 연결이 쓰는 동안 기다리는 시간. 넘으면 SQLITE_BUSY 오류로 돌려준다.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 한 journal 파일의 저장소. 연결 하나를 가지며 Sync가 아니다.
///
/// 활성 writer는 [`EventStore::acquire_writer`]가 돌려준 세대로 쓴다. 더 새 세대가 등록되면
/// 이전 세대의 모든 쓰기는 [`StoreError::Fenced`]로 거절된다.
pub struct EventStore {
    pub(crate) conn: Connection,
    journal_id: String,
}

impl EventStore {
    /// journal 파일을 열거나 만든다. 파일에 다른 journal_id가 있으면 열지 않는다.
    /// 최근 파일이나 다른 식별로 대상을 추측하지 않기 위해서다.
    pub fn open(path: &Path, journal_id: &str) -> StoreResult<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        schema::apply_durability(&conn)?;
        schema::migrate(&mut conn)?;
        bind_journal(&mut conn, journal_id)?;
        Ok(Self {
            conn,
            journal_id: journal_id.to_owned(),
        })
    }

    pub fn journal_id(&self) -> &str {
        &self.journal_id
    }

    /// 새 writer 세대를 등록한다. 이후 이전 세대의 쓰기는 거절된다.
    pub fn acquire_writer(&mut self) -> StoreResult<WriterEpoch> {
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

fn bind_journal(conn: &mut Connection, journal_id: &str) -> StoreResult<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored: Option<String> = tx
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
            tx.execute(
                "INSERT INTO journal_meta (singleton, journal_id, writer_epoch, status)
                 VALUES (1, ?1, 0, 'active')",
                [journal_id],
            )?;
        }
    }
    tx.commit()?;
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
