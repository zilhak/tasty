//! 확정 이벤트 읽기. batch는 공개 단위이므로 항상 이벤트 전체와 revision vector를 함께 돌려준다.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{StoreError, StoreResult};
use crate::store::{EventStore, to_i64, to_u32, to_u64};
use crate::types::{
    BatchCut, BatchId, JournalCut, OpaquePayload, Revision, StoredBatch, StoredEvent, StreamId,
};

const SELECT_EVENT: &str = "SELECT stream_id, stream_revision, event_id, batch_id, batch_index,
    type_tag, schema_version, payload, recorded_at_ms, command_id, causation_id, actor, origin
    FROM events";

impl EventStore {
    /// stream의 `after` 다음 revision부터 최대 `limit`개를 revision 순으로 읽는다.
    pub fn read_stream(
        &self,
        stream: &StreamId,
        after: Option<Revision>,
        limit: usize,
    ) -> StoreResult<Vec<StoredEvent>> {
        let tx = self.conn.unchecked_transaction()?;
        crate::retention::require_stream_cursor(&tx, stream, after)?;
        let mut stmt = tx.prepare(&format!(
            "{SELECT_EVENT} WHERE stream_id = ?1 AND stream_revision > ?2
             ORDER BY stream_revision LIMIT ?3"
        ))?;
        let rows = stmt.query_map(
            params![
                stream.as_str(),
                to_i64(after.unwrap_or(0))?,
                limit_i64(limit)
            ],
            read_event,
        )?;
        collect_events(rows)
    }

    /// batch 하나를 이벤트 전체와 함께 읽는다.
    pub fn read_batch(&self, batch_id: BatchId) -> StoreResult<StoredBatch> {
        let tx = self.conn.unchecked_transaction()?;
        load_batch(&tx, batch_id)
    }

    /// `after` 다음 batch부터 최대 `limit`개를 한 read transaction에서 읽는다.
    pub fn read_batches_after(
        &self,
        after: Option<BatchId>,
        limit: usize,
    ) -> StoreResult<Vec<StoredBatch>> {
        let tx = self.conn.unchecked_transaction()?;
        batches_after(&tx, after, limit)
    }

    /// batch 확정 직후의 전체 stream head.
    pub fn cut_at(&self, batch_id: BatchId) -> StoreResult<JournalCut> {
        let tx = self.conn.unchecked_transaction()?;
        cut_at(&tx, batch_id)
    }
}

pub(crate) fn batches_after(
    conn: &Connection,
    after: Option<BatchId>,
    limit: usize,
) -> StoreResult<Vec<StoredBatch>> {
    crate::retention::require_cursor(conn, after)?;
    let mut stmt = conn
        .prepare("SELECT batch_id FROM batches WHERE batch_id > ?1 ORDER BY batch_id LIMIT ?2")?;
    let ids = stmt.query_map(
        params![to_i64(after.unwrap_or(0))?, limit_i64(limit)],
        |r| r.get::<_, i64>(0),
    )?;
    let mut out = Vec::new();
    for id in ids {
        out.push(load_batch(conn, to_u64(id?)?)?);
    }
    Ok(out)
}

pub(crate) fn load_batch(conn: &Connection, batch_id: BatchId) -> StoreResult<StoredBatch> {
    if crate::retention::floor(conn)?.is_some_and(|floor| batch_id <= floor) {
        return Err(StoreError::ResyncRequired {
            retained_after_batch: crate::retention::floor(conn)?.expect("checked floor"),
        });
    }
    let id = to_i64(batch_id)?;
    let command_id: Option<Option<String>> = conn
        .query_row(
            "SELECT command_id FROM batches WHERE batch_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(command_id) = command_id else {
        return Err(StoreError::UnknownBatch(batch_id));
    };
    let mut stmt =
        conn.prepare("SELECT stream_id, last_revision FROM batch_revisions WHERE batch_id = ?1")?;
    let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut revisions = BTreeMap::new();
    for row in rows {
        let (stream, revision) = row?;
        revisions.insert(StreamId(stream), to_u64(revision)?);
    }
    let mut stmt = conn.prepare(&format!(
        "{SELECT_EVENT} WHERE batch_id = ?1 ORDER BY batch_index"
    ))?;
    let events = collect_events(stmt.query_map([id], read_event)?)?;
    Ok(StoredBatch {
        cut: BatchCut {
            batch_id,
            revisions,
        },
        command_id,
        events,
    })
}

pub(crate) fn cut_at(conn: &Connection, batch_id: BatchId) -> StoreResult<JournalCut> {
    let id = to_i64(batch_id)?;
    let exists: Option<i64> = conn
        .query_row(
            "SELECT batch_id FROM batches WHERE batch_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(StoreError::UnknownBatch(batch_id));
    }
    let mut stmt = conn.prepare(
        "SELECT stream_id, MAX(last_revision) FROM batch_revisions
         WHERE batch_id <= ?1 GROUP BY stream_id",
    )?;
    let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut heads = BTreeMap::new();
    for row in rows {
        let (stream, revision) = row?;
        heads.insert(StreamId(stream), to_u64(revision)?);
    }
    Ok(JournalCut {
        last_batch: Some(batch_id),
        heads,
    })
}

fn limit_i64(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

type RawEvent = (
    String,
    i64,
    String,
    i64,
    i64,
    String,
    u32,
    Vec<u8>,
    i64,
    Option<String>,
    Option<String>,
    String,
    String,
);

fn read_event(r: &Row<'_>) -> rusqlite::Result<RawEvent> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
    ))
}

fn collect_events(
    rows: impl Iterator<Item = rusqlite::Result<RawEvent>>,
) -> StoreResult<Vec<StoredEvent>> {
    let mut out = Vec::new();
    for row in rows {
        let (
            stream,
            rev,
            event_id,
            batch,
            index,
            tag,
            version,
            bytes,
            at,
            cmd,
            cause,
            actor,
            origin,
        ) = row?;
        out.push(StoredEvent {
            stream_id: StreamId(stream),
            stream_revision: to_u64(rev)?,
            event_id,
            batch_id: to_u64(batch)?,
            batch_index: to_u32(index)?,
            payload: OpaquePayload {
                type_tag: tag,
                schema_version: version,
                bytes,
            },
            recorded_at_ms: to_u64(at)?,
            command_id: cmd,
            causation_id: cause,
            actor,
            origin,
        });
    }
    Ok(out)
}
