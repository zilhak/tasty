//! domain snapshot과 consumer checkpoint.
//!
//! domain snapshot은 이벤트 적용을 빠르게 하는 파생 cache다. 원본은 이벤트이며, 검증에 실패한
//! snapshot은 건너뛰고 이전 snapshot이나 전체 로그로 재구성한다.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{StoreError, StoreResult};
use crate::payload;
use crate::read::{batches_after, cut_at};
use crate::store::{EventStore, to_i64, to_u64};
use crate::types::{BatchId, JournalCut, PayloadRef, StoredBatch, WriterEpoch};

pub type SnapshotId = u64;

/// 저장할 domain snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSnapshot {
    /// 이 batch까지 적용한 모델이다.
    pub batch_id: BatchId,
    pub model_version: u32,
    pub bytes: Vec<u8>,
    /// 모델이 참조하는 surface 저장 payload 등. snapshot이 남아 있는 동안 GC되지 않는다.
    pub referenced_payloads: Vec<PayloadRef>,
}

/// 검증을 통과한 domain snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSnapshot {
    pub snapshot_id: SnapshotId,
    pub cut: JournalCut,
    pub model_version: u32,
    pub bytes: Vec<u8>,
}

/// 검증에 실패해 건너뛴 snapshot과 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedSnapshot {
    pub snapshot_id: SnapshotId,
    pub reason: String,
}

/// snapshot+tail 재구성 자료. snapshot이 없으면 tail은 처음부터의 전체 로그다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub snapshot: Option<DomainSnapshot>,
    pub rejected: Vec<RejectedSnapshot>,
    pub tail: Vec<StoredBatch>,
}

impl EventStore {
    /// snapshot을 저장한다. batch가 없으면 거절한다.
    pub fn save_snapshot(
        &mut self,
        epoch: WriterEpoch,
        snapshot: &NewSnapshot,
    ) -> StoreResult<SnapshotId> {
        let tx = self.write_tx(epoch)?;
        cut_at(&tx, snapshot.batch_id)?;
        let body = payload::insert(&tx, &snapshot.bytes)?;
        tx.execute(
            "INSERT INTO snapshots (batch_id, model_version, payload_id) VALUES (?1, ?2, ?3)",
            params![
                to_i64(snapshot.batch_id)?,
                snapshot.model_version,
                to_i64(body.0)?
            ],
        )?;
        let id = to_u64(tx.last_insert_rowid())?;
        let holder = snapshot_holder(id);
        payload::pin_in(&tx, body, &holder)?;
        for referenced in &snapshot.referenced_payloads {
            payload::pin_in(&tx, *referenced, &holder)?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// snapshot을 지운다. 참조만 풀리고 payload 내용은 GC 때 지워진다.
    pub fn delete_snapshot(&mut self, epoch: WriterEpoch, id: SnapshotId) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        tx.execute(
            "DELETE FROM snapshots WHERE snapshot_id = ?1",
            [to_i64(id)?],
        )?;
        tx.execute(
            "DELETE FROM payload_pins WHERE holder = ?1",
            [snapshot_holder(id)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 가장 최근의 검증된 snapshot과 그 뒤 batch 전체를 한 read transaction에서 읽는다.
    pub fn snapshot_and_tail(&self, model_version: u32) -> StoreResult<Replay> {
        let tx = self.conn.unchecked_transaction()?;
        let (snapshot, rejected) = latest_valid(&tx, model_version)?;
        let after = snapshot.as_ref().and_then(|s| s.cut.last_batch);
        let tail = batches_after(&tx, after, usize::MAX)?;
        Ok(Replay {
            snapshot,
            rejected,
            tail,
        })
    }

    /// consumer의 적용 위치를 저장한다. 뒤로 되돌리는 요청은 거절한다.
    pub fn save_checkpoint(
        &mut self,
        epoch: WriterEpoch,
        consumer_id: &str,
        projection_version: u32,
        batch_id: BatchId,
    ) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        advance_checkpoint(&tx, consumer_id, projection_version, batch_id)?;
        tx.commit()?;
        Ok(())
    }

    /// consumer의 적용 위치와 그 시점의 revision vector.
    pub fn checkpoint(
        &self,
        consumer_id: &str,
        projection_version: u32,
    ) -> StoreResult<Option<JournalCut>> {
        let tx = self.conn.unchecked_transaction()?;
        checkpoint_batch(&tx, consumer_id, projection_version)?
            .map(|batch| cut_at(&tx, batch))
            .transpose()
    }
}

/// snapshot이 payload를 참조할 때 쓰는 pin holder 이름.
pub fn snapshot_holder(id: SnapshotId) -> String {
    format!("snapshot:{id}")
}

fn latest_valid(
    conn: &Connection,
    model_version: u32,
) -> StoreResult<(Option<DomainSnapshot>, Vec<RejectedSnapshot>)> {
    let mut stmt = conn.prepare(
        "SELECT snapshot_id, batch_id, payload_id FROM snapshots
         WHERE model_version = ?1 ORDER BY batch_id DESC, snapshot_id DESC",
    )?;
    let rows = stmt.query_map([model_version], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut rejected = Vec::new();
    for row in rows {
        let (id, batch, body) = row?;
        let snapshot_id = to_u64(id)?;
        match payload::read_verified(conn, PayloadRef(to_u64(body)?)) {
            Ok(bytes) => {
                let snapshot = DomainSnapshot {
                    snapshot_id,
                    cut: cut_at(conn, to_u64(batch)?)?,
                    model_version,
                    bytes,
                };
                return Ok((Some(snapshot), rejected));
            }
            Err(err @ (StoreError::PayloadCorrupt(_) | StoreError::PayloadMissing(_))) => {
                rejected.push(RejectedSnapshot {
                    snapshot_id,
                    reason: err.to_string(),
                });
            }
            Err(err) => return Err(err),
        }
    }
    Ok((None, rejected))
}

/// batch가 있고 뒤로 가지 않을 때만 consumer 위치를 옮긴다. 호출자의 쓰기 transaction 안에서 부른다.
pub(crate) fn advance_checkpoint(
    conn: &Connection,
    consumer_id: &str,
    projection_version: u32,
    batch_id: BatchId,
) -> StoreResult<()> {
    cut_at(conn, batch_id)?;
    if let Some(current) = checkpoint_batch(conn, consumer_id, projection_version)?
        && current > batch_id
    {
        return Err(StoreError::CheckpointRegression {
            consumer_id: consumer_id.to_owned(),
            current,
            requested: batch_id,
        });
    }
    conn.execute(
        "INSERT INTO consumer_checkpoints (consumer_id, projection_version, batch_id)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(consumer_id, projection_version) DO UPDATE SET batch_id = excluded.batch_id",
        params![consumer_id, projection_version, to_i64(batch_id)?],
    )?;
    Ok(())
}

pub(crate) fn checkpoint_batch(
    conn: &Connection,
    consumer_id: &str,
    projection_version: u32,
) -> StoreResult<Option<BatchId>> {
    let batch: Option<i64> = conn
        .query_row(
            "SELECT batch_id FROM consumer_checkpoints
             WHERE consumer_id = ?1 AND projection_version = ?2",
            params![consumer_id, projection_version],
            |r| r.get(0),
        )
        .optional()?;
    batch.map(to_u64).transpose()
}
