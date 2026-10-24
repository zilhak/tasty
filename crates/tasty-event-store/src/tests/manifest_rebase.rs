//! Explicit payload ownership must survive a manifest's implicit snapshot cut moving.
use super::common::{append, fresh};
use crate::{
    CommitRequest, EventStore, ExpectedRevision, NewRestoreManifest, NewSnapshot, PayloadRef,
    StreamId, WriterEpoch,
};

fn snapshot(store: &mut EventStore, epoch: WriterEpoch, stream: &str, id: &str) -> u64 {
    let mut request = CommitRequest::new(epoch);
    request
        .appends
        .push(append(stream, ExpectedRevision::Any, &[id]));
    store.commit(&request).unwrap();
    store
        .save_snapshot(
            epoch,
            &NewSnapshot {
                batch_id: store.current_cut().unwrap().last_batch.unwrap(),
                model_version: 1,
                bytes: id.as_bytes().to_vec(),
                referenced_payloads: vec![],
            },
        )
        .unwrap()
}
fn body(store: &EventStore, snapshot: u64) -> PayloadRef {
    PayloadRef(
        store
            .conn
            .query_row(
                "SELECT payload_id FROM snapshots WHERE snapshot_id=?1",
                [snapshot as i64],
                |row| row.get(0),
            )
            .unwrap(),
    )
}
#[test]
fn explicit_old_and_successor_snapshot_bodies_survive_rebase_and_gc() {
    let (_dir, mut store, epoch) = fresh();
    let old = snapshot(&mut store, epoch, "dormant", "old");
    let target = snapshot(&mut store, epoch, "active", "target");
    let refs = vec![body(&store, old), body(&store, target)];
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "view:slot-2".into(),
                incarnation: 1,
                runtime_epoch: epoch.0,
                sequence: 1,
                snapshot_id: old,
                view: b"view".to_vec(),
                referenced_payloads: refs.clone(),
            },
        )
        .unwrap();
    store
        .freeze_restore_alias(epoch, "fixed-transfer", "view:slot-2", b"transfer")
        .unwrap();
    assert!(
        store
            .rebase_restore_manifest(epoch, "view:slot-2", &StreamId::new("dormant"), target)
            .unwrap()
    );
    let newest = snapshot(&mut store, epoch, "active", "newest");
    assert!(
        store
            .rebase_restore_manifest(epoch, "view:slot-2", &StreamId::new("dormant"), newest)
            .unwrap()
    );
    for reference in &refs {
        assert!(
            store
                .payload_holders(*reference)
                .unwrap()
                .contains(&"restore-manifest:view:slot-2".into()),
            "explicit old body membership lost"
        );
    }
    assert!(
        store.delete_snapshot(epoch, old).is_err(),
        "fixed alias still owns its complete old cut"
    );
    assert_eq!(
        store
            .restore_manifest("fixed-transfer")
            .unwrap()
            .unwrap()
            .snapshot
            .snapshot_id,
        old
    );
    store
        .delete_restore_manifest(epoch, "fixed-transfer", 1)
        .unwrap();
    store.delete_snapshot(epoch, old).unwrap();
    store.delete_snapshot(epoch, target).unwrap();
    store.gc_payloads(epoch).unwrap();
    assert_eq!(store.read_payload(refs[0]).unwrap(), b"old");
    assert_eq!(store.read_payload(refs[1]).unwrap(), b"target");
    assert_eq!(
        store.restore_manifest("view:slot-2").unwrap().unwrap().view,
        b"view"
    );
    store
        .delete_restore_manifest(epoch, "view:slot-2", 1)
        .unwrap();
    store.gc_payloads(epoch).unwrap();
    assert!(store.read_payload(refs[0]).is_err());
    assert!(store.read_payload(refs[1]).is_err());
}

#[test]
fn implicit_bodies_do_not_accumulate_and_byte_pins_do_not_freeze_compaction() {
    let (_dir, mut store, epoch) = fresh();
    let old = snapshot(&mut store, epoch, "dormant", "old");
    let old_body = body(&store, old);
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "view:slot-2".into(),
                incarnation: 1,
                runtime_epoch: epoch.0,
                sequence: 1,
                snapshot_id: old,
                view: b"view".to_vec(),
                referenced_payloads: vec![],
            },
        )
        .unwrap();
    let target = snapshot(&mut store, epoch, "active", "target");
    let target_body = body(&store, target);
    store
        .rebase_restore_manifest(epoch, "view:slot-2", &StreamId::new("dormant"), target)
        .unwrap();
    let latest = snapshot(&mut store, epoch, "active", "latest");
    store
        .rebase_restore_manifest(epoch, "view:slot-2", &StreamId::new("dormant"), latest)
        .unwrap();
    let compact = store.compact_history(epoch, 1).unwrap().unwrap();
    assert!(compact.retained_after_batch > 1);
    assert!(
        store.read_payload(old_body).is_err(),
        "implicit predecessor is reclaimed"
    );
    assert!(
        !store
            .payload_holders(target_body)
            .unwrap()
            .contains(&"restore-manifest:view:slot-2".into())
    );
    assert_eq!(
        store
            .restore_manifest("view:slot-2")
            .unwrap()
            .unwrap()
            .snapshot
            .snapshot_id,
        latest
    );
}

#[test]
fn legacy_migration_preserves_ambiguous_ownership_without_accumulating_new_bodies() {
    let (dir, mut store, epoch) = fresh();
    let old = snapshot(&mut store, epoch, "dormant", "old");
    let old_body = body(&store, old);
    let explicit = store.put_payload(epoch, b"explicit legacy bytes").unwrap();
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "view:slot-2".into(),
                incarnation: 1,
                runtime_epoch: epoch.0,
                sequence: 1,
                snapshot_id: old,
                view: b"view".to_vec(),
                referenced_payloads: vec![explicit],
            },
        )
        .unwrap();
    // Construct the exact prior schema: it has only the combined holder set, with no indication
    // whether the current body was also explicitly declared. Migration must not infer absence.
    store
        .conn
        .execute_batch(
            "DROP TABLE restore_manifest_refs; DELETE FROM schema_migrations WHERE version=9;",
        )
        .unwrap();
    drop(store);
    let (mut store, epoch) = super::common::open(&super::common::db_path(&dir));
    let target = snapshot(&mut store, epoch, "active", "target");
    let target_body = body(&store, target);
    store
        .rebase_restore_manifest(epoch, "view:slot-2", &StreamId::new("dormant"), target)
        .unwrap();
    let latest = snapshot(&mut store, epoch, "active", "latest");
    store
        .rebase_restore_manifest(epoch, "view:slot-2", &StreamId::new("dormant"), latest)
        .unwrap();
    let compact = store.compact_history(epoch, 1).unwrap().unwrap();
    assert!(
        compact.retained_after_batch > 1,
        "legacy bytes must not retain the old event tail"
    );
    assert_eq!(
        store.read_payload(old_body).unwrap(),
        b"old",
        "one-time conservative compatibility hold"
    );
    assert_eq!(
        store.read_payload(explicit).unwrap(),
        b"explicit legacy bytes"
    );
    assert!(
        !store
            .payload_holders(target_body)
            .unwrap()
            .contains(&"restore-manifest:view:slot-2".into()),
        "later implicit bodies are not promoted"
    );
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "view:slot-2".into(),
                incarnation: 1,
                runtime_epoch: epoch.0,
                sequence: 2,
                snapshot_id: latest,
                view: b"new view".to_vec(),
                referenced_payloads: vec![],
            },
        )
        .unwrap();
    store.gc_payloads(epoch).unwrap();
    assert!(
        store.read_payload(old_body).is_err(),
        "explicit replacement releases compatibility hold"
    );
    assert!(store.read_payload(explicit).is_err());
}

#[test]
fn legacy_missing_payload_remains_a_manifest_error_instead_of_migration_failure() {
    let (dir, mut store, epoch) = fresh();
    let old = snapshot(&mut store, epoch, "dormant", "old");
    let explicit = store.put_payload(epoch, b"lost legacy bytes").unwrap();
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "view:slot-2".into(),
                incarnation: 1,
                runtime_epoch: epoch.0,
                sequence: 1,
                snapshot_id: old,
                view: b"view".to_vec(),
                referenced_payloads: vec![explicit],
            },
        )
        .unwrap();
    store
        .conn
        .execute_batch(
            "DROP TABLE restore_manifest_refs; DELETE FROM schema_migrations WHERE version=9;",
        )
        .unwrap();
    drop(store);
    let raw = super::common::raw(&super::common::db_path(&dir));
    raw.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    raw.execute(
        "DELETE FROM payloads WHERE payload_id=?1",
        [explicit.0 as i64],
    )
    .unwrap();
    drop(raw);
    let (store, _epoch) = super::common::open(&super::common::db_path(&dir));
    assert!(
        matches!(store.restore_manifest("view:slot-2"), Err(crate::StoreError::PayloadMissing(id)) if id == explicit.0)
    );
}
