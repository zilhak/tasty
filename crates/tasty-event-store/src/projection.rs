//! 영속 projection 출력. 출력 행 변경과 consumer 위치를 한 transaction으로 확정해 위치만 앞서는
//! 일이 없게 한다. 출력은 consumer·projection version별 key→바이트 표이며 해석은 호출자가 한다.
//! 행을 가진 consumer는 [`EventStore::save_checkpoint`]로 위치만 옮길 수 없다.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use crate::error::{StoreError, StoreResult};
use crate::read::cut_at;
use crate::snapshot::{advance_checkpoint, checkpoint_batch};
use crate::store::EventStore;
use crate::types::{BatchId, JournalCut, WriterEpoch};

/// 한 번에 확정할 projection 변경. `batch_id`까지 적용한 결과다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionWrite {
    pub consumer_id: String,
    pub projection_version: u32,
    pub batch_id: BatchId,
    /// 새로 쓰거나 바꿀 행.
    pub upserts: Vec<(String, Vec<u8>)>,
    /// 지울 행. 없는 key는 무시한다. 같은 key를 upsert와 함께 줄 수 없다.
    pub deletes: Vec<String>,
}

/// 한 read transaction에서 읽은 projection 출력과 위치.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionState {
    /// 적용 위치. 아직 한 번도 확정하지 않았으면 `None`이다.
    pub cut: Option<JournalCut>,
    pub rows: BTreeMap<String, Vec<u8>>,
}

impl EventStore {
    /// 출력 행 변경과 위치 이동을 한 transaction으로 확정한다. batch가 없거나 위치가 뒤로 가거나
    /// key가 겹치면 아무것도 바꾸지 않는다.
    pub fn commit_projection(
        &mut self,
        epoch: WriterEpoch,
        write: &ProjectionWrite,
    ) -> StoreResult<()> {
        check_keys(write)?;
        let tx = self.write_tx(epoch)?;
        // An incremental delta cannot repair rows whose missing history was already compacted.
        crate::retention::require_cursor(&tx, checkpoint_batch(&tx, &write.consumer_id, write.projection_version)?)?;
        apply_rows(&tx, write)?;
        advance_checkpoint(
            &tx,
            &write.consumer_id,
            write.projection_version,
            write.batch_id,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Replace the complete projection after an explicit snapshot resynchronization. Stale rows
    /// and the old cursor are replaced in the same transaction; incremental writes cannot imply it.
    pub fn replace_projection(
        &mut self,
        epoch: WriterEpoch,
        write: &ProjectionWrite,
    ) -> StoreResult<()> {
        check_keys(write)?;
        let tx = self.write_tx(epoch)?;
        tx.execute("DELETE FROM projection_rows WHERE consumer_id = ?1 AND projection_version = ?2",
            params![write.consumer_id, write.projection_version])?;
        apply_rows(&tx, write)?;
        advance_checkpoint(&tx, &write.consumer_id, write.projection_version, write.batch_id)?;
        tx.commit()?;
        Ok(())
    }

    /// 출력 전체와 위치를 같은 시점으로 읽는다.
    pub fn projection_state(
        &self,
        consumer_id: &str,
        projection_version: u32,
    ) -> StoreResult<ProjectionState> {
        let tx = self.conn.unchecked_transaction()?;
        let cut = checkpoint_batch(&tx, consumer_id, projection_version)?
            .map(|batch| {
                crate::retention::require_cursor(&tx, Some(batch))?;
                cut_at(&tx, batch)
            })
            .transpose()?;
        let mut stmt = tx.prepare(
            "SELECT key, payload FROM projection_rows
             WHERE consumer_id = ?1 AND projection_version = ?2",
        )?;
        let rows = stmt
            .query_map(params![consumer_id, projection_version], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(ProjectionState { cut, rows })
    }
}

fn check_keys(write: &ProjectionWrite) -> StoreResult<()> {
    let deletes: BTreeSet<&str> = write.deletes.iter().map(String::as_str).collect();
    let mut upserts = BTreeSet::new();
    for (key, _) in &write.upserts {
        if deletes.contains(key.as_str()) || !upserts.insert(key.as_str()) {
            return Err(StoreError::ProjectionKeyConflict(key.clone()));
        }
    }
    Ok(())
}

fn apply_rows(conn: &Connection, write: &ProjectionWrite) -> StoreResult<()> {
    for key in &write.deletes {
        conn.execute(
            "DELETE FROM projection_rows
             WHERE consumer_id = ?1 AND projection_version = ?2 AND key = ?3",
            params![write.consumer_id, write.projection_version, key],
        )?;
    }
    for (key, payload) in &write.upserts {
        conn.execute(
            "INSERT INTO projection_rows (consumer_id, projection_version, key, payload)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(consumer_id, projection_version, key) DO UPDATE SET payload = excluded.payload",
            params![write.consumer_id, write.projection_version, key, payload],
        )?;
    }
    Ok(())
}
