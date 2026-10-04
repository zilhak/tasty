//! Event-body retention. Batch identity/revision headers, commands and effect evidence survive.
use crate::snapshot::{snapshot_holder, verify_snapshot};
use crate::store::{to_i64, to_u64};
use crate::{EventStore, PayloadRef, StoreError, StoreResult, StreamId, WriterEpoch};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    pub retained_after_batch: u64,
    pub removed_events: usize,
    pub removed_payloads: usize,
}
impl EventStore {
    /// Compact only behind two checksum-valid snapshots of the caller's complete domain model.
    /// Callers must include all live, undo, recovery and in-flight reader refs in those snapshots.
    /// Other holders on a snapshot body keep its complete restore cut available.
    pub fn compact_history(
        &mut self,
        epoch: WriterEpoch,
        model_version: u32,
    ) -> StoreResult<Option<Compaction>> {
        let tx = self.write_tx(epoch)?;
        let mut stmt = tx.prepare("SELECT snapshot_id, batch_id, payload_id, model_version FROM snapshots ORDER BY batch_id DESC, snapshot_id DESC")?;
        let snapshots = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut valid = Vec::new();
        for &(id, batch, body, version) in &snapshots {
            if version != model_version {
                continue;
            }
            match verify_snapshot(&tx, to_u64(id)?, PayloadRef(to_u64(body)?)) {
                Ok(_) => valid.push((id, batch)),
                Err(StoreError::PayloadCorrupt(_) | StoreError::PayloadMissing(_)) => {}
                Err(error) => return Err(error),
            }
            if valid.len() == 2 {
                break;
            }
        }
        let Some(&(mut anchor_id, mut boundary)) = valid.get(1) else {
            return Ok(None);
        };
        // External restore/import manifests require their older snapshot and its tail to survive.
        for &(id, batch, body, version) in &snapshots {
            if batch >= boundary {
                continue;
            }
            let held: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM payload_pins WHERE payload_id = ?1 AND holder != ?2)",
                params![body, snapshot_holder(to_u64(id)?)],
                |row| row.get(0),
            )?;
            if held {
                if version != model_version {
                    return Ok(None);
                }
                verify_snapshot(&tx, to_u64(id)?, PayloadRef(to_u64(body)?))?;
                anchor_id = id;
                boundary = batch;
            }
        }
        let boundary = to_u64(boundary)?;
        if floor(&tx)?.is_some_and(|current| current >= boundary) {
            return Ok(None);
        }
        tx.execute("INSERT INTO retention_anchor(singleton,batch_id,snapshot_id) VALUES(1,?1,?2)
            ON CONFLICT(singleton) DO UPDATE SET batch_id=excluded.batch_id,snapshot_id=excluded.snapshot_id",
            params![to_i64(boundary)?, anchor_id])?;
        tx.execute("DELETE FROM retained_stream_revisions", [])?;
        tx.execute("INSERT INTO retained_stream_revisions(stream_id,revision)
            SELECT stream_id, MAX(last_revision) FROM batch_revisions WHERE batch_id <= ?1 GROUP BY stream_id", [to_i64(boundary)?])?;
        for &(id, batch, _, _) in &snapshots {
            if to_u64(batch)? < boundary {
                tx.execute("DELETE FROM snapshots WHERE snapshot_id = ?1", [id])?;
                tx.execute(
                    "DELETE FROM payload_pins WHERE holder = ?1",
                    [snapshot_holder(to_u64(id)?)],
                )?;
            }
        }
        tx.execute("INSERT INTO retained_event_ids(event_id) SELECT event_id FROM events WHERE batch_id <= ?1", [to_i64(boundary)?])?;
        tx.execute("DELETE FROM payload_pins WHERE holder IN (SELECT 'event:' || event_id FROM events WHERE batch_id <= ?1)", [to_i64(boundary)?])?;
        let removed_events = tx.execute(
            "DELETE FROM events WHERE batch_id <= ?1",
            [to_i64(boundary)?],
        )?;
        let removed_payloads = tx.execute(
            "DELETE FROM payloads WHERE payload_id NOT IN (SELECT payload_id FROM payload_pins)",
            [],
        )?;
        tx.commit()?;
        Ok(Some(Compaction {
            retained_after_batch: boundary,
            removed_events,
            removed_payloads,
        }))
    }
}

pub(crate) fn floor(conn: &Connection) -> StoreResult<Option<u64>> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT batch_id FROM retention_anchor WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    value.map(to_u64).transpose()
}
pub(crate) fn anchor_snapshot(conn: &Connection) -> StoreResult<Option<u64>> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT snapshot_id FROM retention_anchor WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    value.map(to_u64).transpose()
}
pub(crate) fn require_cursor(conn: &Connection, after: Option<u64>) -> StoreResult<()> {
    if let Some(retained_after_batch) = floor(conn)?
        && after.is_none_or(|after| after < retained_after_batch)
    {
        return Err(StoreError::ResyncRequired {
            retained_after_batch,
        });
    }
    Ok(())
}
pub(crate) fn require_stream_cursor(
    conn: &Connection,
    stream: &StreamId,
    after: Option<u64>,
) -> StoreResult<()> {
    let revision: Option<i64> = conn
        .query_row(
            "SELECT revision FROM retained_stream_revisions WHERE stream_id = ?1",
            [stream.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(revision) = revision
        && after.unwrap_or(0) < to_u64(revision)?
    {
        return Err(StoreError::ResyncRequired {
            retained_after_batch: floor(conn)?
                .ok_or_else(|| StoreError::Corrupt("retention revision has no anchor".into()))?,
        });
    }
    Ok(())
}
