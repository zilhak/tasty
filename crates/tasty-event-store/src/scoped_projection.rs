//! Partial-stream projections retain one real batch boundary and an immutable stream scope.
use crate::store::to_i64;
use crate::{
    BatchId, EventStore, ProjectionWrite, Revision, StoreError, StoreResult, StreamId, WriterEpoch,
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedProjectionWrite {
    pub write: ProjectionWrite,
    /// Nonempty and fixed for this consumer/version. Change version to change scope.
    pub streams: BTreeSet<StreamId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCut {
    pub batch_id: BatchId,
    /// Only selected streams, derived by the store at batch_id; never a global journal cut.
    pub revisions: BTreeMap<StreamId, Revision>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedProjectionState {
    pub cut: ScopedCut,
    pub rows: BTreeMap<String, Vec<u8>>,
}

impl EventStore {
    /// Atomic output/scope/cursor advance. Repeating the exact same write at the same batch is
    /// idempotent; another write at that batch is rejected. Unknown/empty streams are rejected.
    pub fn commit_scoped_projection(
        &mut self,
        epoch: WriterEpoch,
        write: &ScopedProjectionWrite,
    ) -> StoreResult<()> {
        self.write_scoped_projection(epoch, write, false)
    }
    /// Explicit snapshot resync of the entire selected scope; all old output rows are replaced.
    /// It cannot change scope, move backward or convert a global consumer. Use a new version.
    pub fn replace_scoped_projection(
        &mut self,
        epoch: WriterEpoch,
        write: &ScopedProjectionWrite,
    ) -> StoreResult<()> {
        self.write_scoped_projection(epoch, write, true)
    }
    fn write_scoped_projection(
        &mut self,
        epoch: WriterEpoch,
        scoped: &ScopedProjectionWrite,
        replace: bool,
    ) -> StoreResult<()> {
        let write = &scoped.write;
        crate::write_limits::projection(write,Some(&scoped.streams))?;
        crate::projection::check_keys(write)?;
        let tx = self.write_tx(epoch)?;
        let requested = scoped_cut(&tx, write.batch_id, &scoped.streams)?;
        require_retained(&tx, &requested)?;
        let old_scope = scope(&tx, &write.consumer_id, write.projection_version)?;
        let old_batch =
            crate::snapshot::checkpoint_batch(&tx, &write.consumer_id, write.projection_version)?;
        if let Some(old_scope) = &old_scope {
            if old_scope != &scoped.streams {
                return Err(scope_error(
                    "scope is immutable; use another projection version",
                ));
            }
        } else {
            let rows: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projection_rows WHERE consumer_id=?1 AND projection_version=?2)",
                params![write.consumer_id,write.projection_version], |row| row.get(0))?;
            if old_batch.is_some() || rows {
                return Err(scope_error(
                    "global consumer cannot become scoped; use another projection version",
                ));
            }
        }
        let digest = digest(scoped, replace);
        if let Some(batch) = old_batch {
            if batch > write.batch_id {
                return Err(StoreError::CheckpointRegression {
                    consumer_id: write.consumer_id.clone(),
                    current: batch,
                    requested: write.batch_id,
                });
            }
            // Resync may repair a stale cursor. Ordinary deltas must first prove no selected
            // stream history needed by the previous cursor has been compacted.
            if !replace {
                require_retained(&tx, &scoped_cut(&tx, batch, &scoped.streams)?)?;
            }
            if batch == write.batch_id {
                let previous: Vec<u8> = tx.query_row("SELECT last_write_digest FROM projection_scopes WHERE consumer_id=?1 AND projection_version=?2",
                    params![write.consumer_id,write.projection_version], |row|row.get(0))?;
                if previous != digest {
                    return Err(scope_error("different projection write at the same batch"));
                }
                tx.commit()?;
                return Ok(());
            }
        } else if !replace {
            let initial = ScopedCut {
                batch_id: write.batch_id,
                revisions: scoped
                    .streams
                    .iter()
                    .cloned()
                    .map(|stream| (stream, 0))
                    .collect(),
            };
            require_retained(&tx, &initial)?;
        }
        if old_scope.is_none() {
            tx.execute("INSERT INTO projection_scopes(consumer_id,projection_version,last_write_digest) VALUES(?1,?2,?3)",params![write.consumer_id,write.projection_version,&digest])?;
            for stream in &scoped.streams {
                tx.execute("INSERT INTO projection_scope_streams(consumer_id,projection_version,stream_id) VALUES(?1,?2,?3)",params![write.consumer_id,write.projection_version,stream.as_str()])?;
            }
        }
        if replace {
            tx.execute(
                "DELETE FROM projection_rows WHERE consumer_id=?1 AND projection_version=?2",
                params![write.consumer_id, write.projection_version],
            )?;
        }
        crate::projection::apply_rows(&tx, write)?;
        tx.execute("INSERT INTO consumer_checkpoints(consumer_id,projection_version,batch_id) VALUES(?1,?2,?3) ON CONFLICT(consumer_id,projection_version) DO UPDATE SET batch_id=excluded.batch_id",
            params![write.consumer_id,write.projection_version,to_i64(write.batch_id)?])?;
        tx.execute("UPDATE projection_scopes SET last_write_digest=?3 WHERE consumer_id=?1 AND projection_version=?2",params![write.consumer_id,write.projection_version,&digest])?;
        tx.commit()?;
        Ok(())
    }
    /// Read rows and the selected revision vector at one SQLite snapshot. Compacted selected
    /// history requires explicit resync; unrelated streams do not invalidate this cursor.
    pub fn scoped_projection_state(
        &self,
        consumer: &str,
        version: u32,
    ) -> StoreResult<Option<ScopedProjectionState>> {
        let tx = self.conn.unchecked_transaction()?;
        let Some(streams) = scope(&tx, consumer, version)? else {
            if crate::snapshot::checkpoint_batch(&tx, consumer, version)?.is_some() {
                return Err(scope_error("consumer is global, not scoped"));
            }
            return Ok(None);
        };
        let batch = crate::snapshot::checkpoint_batch(&tx, consumer, version)?
            .ok_or_else(|| scope_error("scoped consumer has no cursor"))?;
        let cut = scoped_cut(&tx, batch, &streams)?;
        require_retained(&tx, &cut)?;
        let mut statement=tx.prepare("SELECT key,payload FROM projection_rows WHERE consumer_id=?1 AND projection_version=?2")?;
        let rows = statement
            .query_map(params![consumer, version], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Some(ScopedProjectionState { cut, rows }))
    }
}
fn scope_error(message: &str) -> StoreError {
    StoreError::ProjectionScope(message.into())
}
pub(crate) fn require_global(conn: &Connection, consumer: &str, version: u32) -> StoreResult<()> {
    if scope(conn, consumer, version)?.is_some() {
        return Err(scope_error(
            "scoped consumer requires scoped projection API",
        ));
    }
    Ok(())
}
fn scope(
    conn: &Connection,
    consumer: &str,
    version: u32,
) -> StoreResult<Option<BTreeSet<StreamId>>> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM projection_scopes WHERE consumer_id=?1 AND projection_version=?2",
            params![consumer, version],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(None);
    }
    let mut statement=conn.prepare("SELECT stream_id FROM projection_scope_streams WHERE consumer_id=?1 AND projection_version=?2")?;
    let streams = statement
        .query_map(params![consumer, version], |row| row.get::<_, String>(0))?
        .map(|row| row.map(StreamId))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if streams.is_empty() {
        return Err(scope_error("stored projection scope is empty"));
    }
    Ok(Some(streams))
}
fn scoped_cut(
    conn: &Connection,
    batch: BatchId,
    streams: &BTreeSet<StreamId>,
) -> StoreResult<ScopedCut> {
    if streams.is_empty() || streams.iter().any(|stream| stream.as_str().is_empty()) {
        return Err(scope_error("projection scope must contain named streams"));
    }
    let global = crate::read::cut_at(conn, batch)?;
    let revisions = streams
        .iter()
        .map(|stream| {
            global
                .heads
                .get(stream)
                .copied()
                .map(|revision| (stream.clone(), revision))
                .ok_or_else(|| scope_error("selected stream does not exist at the batch cut"))
        })
        .collect::<StoreResult<_>>()?;
    Ok(ScopedCut {
        batch_id: batch,
        revisions,
    })
}
fn require_retained(conn: &Connection, cut: &ScopedCut) -> StoreResult<()> {
    for (stream, revision) in &cut.revisions {
        crate::retention::require_stream_cursor(conn, stream, Some(*revision))?;
    }
    Ok(())
}
fn digest(scoped: &ScopedProjectionWrite, replace: bool) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update([u8::from(replace)]);
    let mut field = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    field(b"scoped-projection-write-v1");
    field(&(scoped.streams.len() as u64).to_le_bytes());
    for stream in &scoped.streams {
        field(stream.as_str().as_bytes());
    }
    field(b"upserts");
    let rows: BTreeMap<_, _> = scoped
        .write
        .upserts
        .iter()
        .map(|(key, value)| (key, value))
        .collect();
    field(&(rows.len() as u64).to_le_bytes());
    for (key, value) in rows {
        field(key.as_bytes());
        field(value);
    }
    field(b"deletes");
    let deletes = scoped.write.deletes.iter().collect::<BTreeSet<_>>();
    field(&(deletes.len() as u64).to_le_bytes());
    for key in deletes {
        field(key.as_bytes());
    }
    hash.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collection_boundaries_are_part_of_retry_identity() {
        let mut a = ScopedProjectionWrite {
            streams: BTreeSet::from([StreamId::new("s")]),
            write: ProjectionWrite {
                consumer_id: "p".into(),
                projection_version: 1,
                batch_id: 1,
                upserts: vec![("a".into(), b"b".to_vec())],
                deletes: vec!["c".into(), "deletes".into(), "e".into()],
            },
        };
        let first = digest(&a, false);
        a.write.upserts.push(("deletes".into(), b"c".to_vec()));
        a.write.deletes = vec!["e".into()];
        assert_ne!(first, digest(&a, false));
    }
}
