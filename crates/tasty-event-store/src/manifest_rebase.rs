//! Move a scoped restore source only across cuts with no events in that scope.
use crate::snapshot::verify_snapshot;
use crate::store::{to_i64, to_u64};
use crate::{EventStore, PayloadRef, SnapshotId, StoreResult, StreamId, WriterEpoch};
use rusqlite::{OptionalExtension, params};

impl EventStore {
    /// Rebind an unchanged stream's manifest to a newer complete-domain snapshot. The caller
    /// must also verify semantic equivalence of the decoded stream models and the View binding.
    /// Frozen transfer aliases retain their original whole-domain cut. Opaque View bytes, ordering
    /// and explicit payload pins are unchanged; snapshot ownership moves atomically.
    pub fn rebase_restore_manifest(
        &mut self,
        epoch: WriterEpoch,
        key: &str,
        stream: &StreamId,
        target: SnapshotId,
    ) -> StoreResult<bool> {
        let tx = self.write_tx(epoch)?;
        let source: Option<i64> = tx
            .query_row(
                "SELECT snapshot_id FROM restore_manifests WHERE restore_key=?1
             AND NOT EXISTS(SELECT 1 FROM restore_alias_inputs WHERE restore_key=?1)",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(source) = source else {
            return Ok(false);
        };
        if to_u64(source)? == target {
            return Ok(false);
        }
        let snapshot = |id: i64| -> StoreResult<(i64, u32, i64)> {
            Ok(tx.query_row(
                "SELECT batch_id,model_version,payload_id FROM snapshots WHERE snapshot_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?)
        };
        let (old_batch, old_version, old_body) = snapshot(source)?;
        let (batch, version, body) = snapshot(to_i64(target)?)?;
        let revision = |batch: i64| -> StoreResult<Option<i64>> {
            Ok(tx.query_row("SELECT MAX(last_revision) FROM batch_revisions WHERE stream_id=?1 AND batch_id<=?2",
                params![stream.as_str(), batch], |row| row.get(0))?)
        };
        let before = revision(old_batch)?;
        if batch < old_batch
            || version != old_version
            || before.is_none()
            || before != revision(batch)?
        {
            return Ok(false);
        }
        crate::retention::require_cursor(&tx, Some(to_u64(batch)?))?;
        verify_snapshot(&tx, to_u64(source)?, PayloadRef(to_u64(old_body)?))?;
        verify_snapshot(&tx, target, PayloadRef(to_u64(body)?))?;
        let holder = format!("restore-manifest:{key}");
        // Pin the successor first. No View payload is inserted or rewritten by maintenance.
        crate::payload::pin_in(&tx, PayloadRef(to_u64(body)?), &holder)?;
        if old_body != body {
            tx.execute(
                "DELETE FROM payload_pins WHERE holder=?1 AND payload_id=?2",
                params![holder, old_body],
            )?;
        }
        tx.execute(
            "UPDATE restore_manifests SET snapshot_id=?1 WHERE restore_key=?2",
            params![to_i64(target)?, key],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
