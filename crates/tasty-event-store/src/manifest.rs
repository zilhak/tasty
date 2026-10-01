//! A View checkpoint remains a source record, bound atomically to an independently verified domain
//! snapshot. Its opaque bytes are never replaced by replayed domain selections.
use rusqlite::{OptionalExtension, params};
use crate::{DomainSnapshot, EventStore, PayloadRef, SnapshotId, StoreError, StoreResult, WriterEpoch};
use crate::store::{to_i64, to_u64};

#[derive(Debug, Clone)]
pub struct NewRestoreManifest {
    pub restore_key: String,
    pub incarnation: u64,
    pub runtime_epoch: u64,
    pub sequence: u64,
    pub snapshot_id: SnapshotId,
    pub view: Vec<u8>,
    pub referenced_payloads: Vec<PayloadRef>,
}
#[derive(Debug, Clone)]
pub struct RestoreManifest {
    pub incarnation: u64,
    pub runtime_epoch: u64,
    pub sequence: u64,
    pub snapshot: DomainSnapshot,
    pub view: Vec<u8>,
}
impl EventStore {
    /// Publish checkpoint bytes and transfer every required pin in the same fenced transaction.
    /// A failure keeps the previous manifest and ownership intact. Callers use an exact slot key.
    pub fn save_restore_manifest(&mut self, epoch: WriterEpoch, manifest: &NewRestoreManifest) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        save_in(&tx, manifest)?;
        tx.commit()?;
        Ok(())
    }

    /// Publish a new domain checkpoint and its View manifest atomically. The input snapshot_id is
    /// replaced by the ID allocated in this transaction; no half-published snapshot becomes a source.
    pub fn save_restore_checkpoint(
        &mut self,
        epoch: WriterEpoch,
        snapshot: &crate::NewSnapshot,
        manifest: &NewRestoreManifest,
    ) -> StoreResult<SnapshotId> {
        let tx = self.write_tx(epoch)?;
        let id = crate::snapshot::insert_snapshot(&tx, snapshot)?;
        let manifest = NewRestoreManifest {snapshot_id: id, ..manifest.clone()};
        save_in(&tx, &manifest)?;
        crate::snapshot::retain_snapshots(&tx, snapshot.model_version, 2)?;
        tx.execute("DELETE FROM payloads WHERE payload_id NOT IN (SELECT payload_id FROM payload_pins)", [])?;
        tx.commit()?;
        Ok(id)
    }

    /// Metadata-only enumeration lets retirement release old slot ownership without decoding a
    /// damaged checkpoint from an incarnation that is no longer a restore target.
    pub fn restore_manifest_bindings(&self) -> StoreResult<Vec<(String, u64)>> {
        let mut stmt = self.conn.prepare("SELECT restore_key,incarnation FROM restore_manifests ORDER BY restore_key")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?,row.get::<_, i64>(1)?)))?;
        rows.map(|row| {let (key, incarnation) = row?; Ok((key,to_u64(incarnation)?))}).collect()
    }

    /// Inspect binding before reading opaque bytes. A new incarnation does not depend on damage in
    /// an older slot's View checkpoint, while matching-incarnation corruption remains an error.
    pub fn restore_manifest_incarnation(&self, restore_key: &str) -> StoreResult<Option<u64>> {
        let value: Option<i64> = self.conn.query_row("SELECT incarnation FROM restore_manifests WHERE restore_key = ?1",
            [restore_key], |row| row.get(0)).optional()?;
        value.map(to_u64).transpose()
    }

    /// Read a complete manifest and its verified snapshot in one read transaction. A damaged View
    /// is an error, not permission to regenerate user selection or silently consult a stale export.
    pub fn restore_manifest(&self, restore_key: &str) -> StoreResult<Option<RestoreManifest>> {
        let tx = self.conn.unchecked_transaction()?;
        let row: Option<(i64,i64,i64,i64,i64)> = tx.query_row(
            "SELECT incarnation,runtime_epoch,sequence,snapshot_id,payload_id FROM restore_manifests WHERE restore_key = ?1",
            [restore_key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).optional()?;
        let Some((incarnation,runtime_epoch,sequence,snapshot_id,view)) = row else {return Ok(None);};
        let snapshot_id = to_u64(snapshot_id)?;
        let (batch, version, body) = snapshot_row(&tx, snapshot_id)?;
        let batch = to_u64(batch)?;
        crate::retention::require_cursor(&tx, Some(batch))?;
        let bytes = crate::snapshot::verify_snapshot(&tx, snapshot_id, PayloadRef(to_u64(body)?))?;
        let view = crate::payload::read_verified(&tx, PayloadRef(to_u64(view)?))?;
        let mut stmt = tx.prepare("SELECT payload_id FROM payload_pins WHERE holder = ?1")?;
        let refs = stmt.query_map([holder(restore_key)], |row| row.get::<_, i64>(0))?;
        for reference in refs {crate::payload::read_verified(&tx, PayloadRef(to_u64(reference?)?))?;}
        Ok(Some(RestoreManifest {
            incarnation: to_u64(incarnation)?, runtime_epoch: to_u64(runtime_epoch)?, sequence: to_u64(sequence)?,
            snapshot: DomainSnapshot {snapshot_id,cut:crate::read::cut_at(&tx,batch)?,model_version:version,bytes}, view,
        }))
    }

    /// A retirement may release only its own incarnation. A late old close cannot remove a new slot.
    pub fn delete_restore_manifest(&mut self, epoch: WriterEpoch, restore_key: &str, incarnation: u64) -> StoreResult<bool> {
        let tx = self.write_tx(epoch)?;
        let removed = tx.execute("DELETE FROM restore_manifests WHERE restore_key = ?1 AND incarnation = ?2",
            params![restore_key,to_i64(incarnation)?])?;
        if removed != 0 {tx.execute("DELETE FROM payload_pins WHERE holder = ?1", [holder(restore_key)])?;}
        tx.commit()?;
        Ok(removed != 0)
    }
}
fn holder(key: &str) -> String {format!("restore-manifest:{key}")}
fn snapshot_row(conn: &rusqlite::Connection, id: SnapshotId) -> StoreResult<(i64,u32,i64)> {
    conn.query_row("SELECT batch_id,model_version,payload_id FROM snapshots WHERE snapshot_id = ?1", [to_i64(id)?],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?.ok_or_else(|| StoreError::Corrupt(format!("restore snapshot {id} is missing")))
}

pub(crate) fn save_in(conn: &rusqlite::Connection, manifest: &NewRestoreManifest) -> StoreResult<()> {
        let previous: Option<(i64, i64, i64)> = conn.query_row(
            "SELECT incarnation,runtime_epoch,sequence FROM restore_manifests WHERE restore_key = ?1",
            [&manifest.restore_key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        let incoming = (to_i64(manifest.incarnation)?,to_i64(manifest.runtime_epoch)?,to_i64(manifest.sequence)?);
        if previous.is_some_and(|previous| previous > incoming) {return Err(StoreError::ManifestRegression(manifest.restore_key.clone()));}
        let (batch, _, body) = snapshot_row(conn, manifest.snapshot_id)?;
        let previous_cut: Option<(i64, i64)> = conn.query_row(
            "SELECT snapshot.batch_id, manifest.payload_id FROM restore_manifests AS manifest
             JOIN snapshots AS snapshot ON snapshot.snapshot_id = manifest.snapshot_id WHERE manifest.restore_key = ?1",
            [&manifest.restore_key], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        if let Some((previous_batch, previous_view)) = previous_cut {
            if previous_batch > batch || (previous == Some(incoming)
                && crate::payload::read_verified(conn, PayloadRef(to_u64(previous_view)?))? != manifest.view) {
                return Err(StoreError::ManifestRegression(manifest.restore_key.clone()));
            }
        }
        crate::retention::require_cursor(conn, Some(to_u64(batch)?))?;
        crate::snapshot::verify_snapshot(conn, manifest.snapshot_id, PayloadRef(to_u64(body)?))?;
        let view = crate::payload::insert(conn, &manifest.view)?;
        let mut references = manifest.referenced_payloads.clone();
        references.extend([PayloadRef(to_u64(body)?), view]);
        crate::payload::replace_holder_in(conn, &holder(&manifest.restore_key), &references)?;
        conn.execute("INSERT INTO restore_manifests(restore_key,incarnation,runtime_epoch,sequence,snapshot_id,payload_id)
            VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(restore_key) DO UPDATE SET incarnation=excluded.incarnation,
            runtime_epoch=excluded.runtime_epoch,sequence=excluded.sequence,snapshot_id=excluded.snapshot_id,payload_id=excluded.payload_id",
            params![manifest.restore_key,incoming.0,incoming.1,incoming.2,to_i64(manifest.snapshot_id)?,to_i64(view.0)?])?;
    Ok(())
}
