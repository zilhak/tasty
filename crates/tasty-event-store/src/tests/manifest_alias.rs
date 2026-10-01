//! A transfer identity and its source cut must survive or roll back together.
use super::common::{append, db_path, fresh, open};
use crate::{CommitRequest, ExpectedRevision, NewRestoreManifest, NewSnapshot, StoreError};

fn selected(store: &mut crate::EventStore, epoch: crate::WriterEpoch) -> NewRestoreManifest {
    let mut commit = CommitRequest::new(epoch);
    commit
        .appends
        .push(append("engine", ExpectedRevision::NoStream, &["created"]));
    store.commit(&commit).unwrap();
    let external = store
        .put_payload(epoch, b"external manifest dependency")
        .unwrap();
    let mut manifest = NewRestoreManifest {
        restore_key: "A".into(),
        incarnation: 1,
        runtime_epoch: epoch.0,
        sequence: 1,
        snapshot_id: 0,
        view: b"original A".to_vec(),
        referenced_payloads: vec![external],
    };
    manifest.snapshot_id = store
        .save_restore_checkpoint(
            epoch,
            &NewSnapshot {
                batch_id: 1,
                model_version: 1,
                bytes: b"domain cut".to_vec(),
                referenced_payloads: vec![],
            },
            &manifest,
        )
        .unwrap();
    manifest
}

#[test]
fn retry_keeps_original_source_cut_and_rejects_changed_input_after_reopen() {
    let (dir, mut store, epoch) = fresh();
    let original = selected(&mut store, epoch);
    let frozen = store
        .freeze_restore_alias(epoch, "transfer", "A", b"request A")
        .unwrap();
    let pinned: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM payload_pins WHERE holder='restore-manifest:transfer' AND payload_id=?1)",
        [original.referenced_payloads[0].0 as i64], |row| row.get(0),
    ).unwrap();
    assert!(
        pinned,
        "alias retains explicit source manifest dependencies"
    );
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                sequence: 2,
                view: b"later A".to_vec(),
                ..original.clone()
            },
        )
        .unwrap();
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "B".into(),
                view: b"B".to_vec(),
                ..original.clone()
            },
        )
        .unwrap();
    drop(store); // Destination can still be uncommitted; the source identity is already durable.
    let (mut store, epoch) = open(&db_path(&dir));
    let retry = store
        .freeze_restore_alias(epoch, "transfer", "A", b"request A")
        .unwrap();
    assert_eq!(retry.view, frozen.view);
    assert_eq!(retry.snapshot.snapshot_id, frozen.snapshot.snapshot_id);
    assert_eq!(retry.snapshot.cut, frozen.snapshot.cut);
    for (source, digest) in [
        ("B", b"request B".as_slice()),
        ("A", b"changed destination slot".as_slice()),
    ] {
        assert!(matches!(
            store.freeze_restore_alias(epoch, "transfer", source, digest),
            Err(StoreError::RestoreAliasConflict(_))
        ));
    }
    assert!(matches!(
        store.save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "transfer".into(),
                ..original
            }
        ),
        Err(StoreError::RestoreAliasConflict(_))
    ));
    assert_eq!(
        store.restore_manifest("transfer").unwrap().unwrap().view,
        b"original A"
    );
    assert!(!store.delete_restore_manifest(epoch, "transfer", 2).unwrap());
    assert!(store.delete_restore_manifest(epoch, "transfer", 1).unwrap());
    let inputs: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM restore_alias_inputs", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        inputs, 0,
        "releasing the alias also releases its input identity"
    );
}

#[test]
fn alias_and_pins_roll_back_when_input_identity_cannot_be_written() {
    let (_dir, mut store, epoch) = fresh();
    selected(&mut store, epoch);
    let payloads: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM payloads", [], |r| r.get(0))
        .unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER reject_alias_input BEFORE INSERT ON restore_alias_inputs
        BEGIN SELECT RAISE(ABORT, 'injected identity write failure'); END;",
        )
        .unwrap();
    assert!(
        store
            .freeze_restore_alias(epoch, "transfer", "A", b"request A")
            .is_err()
    );
    assert!(store.restore_manifest("transfer").unwrap().is_none());
    let pins: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM payload_pins WHERE holder='restore-manifest:transfer'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pins, 0);
    let after: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM payloads", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, payloads);
    assert_eq!(
        store.restore_manifest("A").unwrap().unwrap().view,
        b"original A"
    );
    store
        .conn
        .execute_batch("DROP TRIGGER reject_alias_input")
        .unwrap();
    store
        .freeze_restore_alias(epoch, "transfer", "A", b"request A")
        .unwrap();
}

#[test]
fn unbound_legacy_alias_is_not_attributed_to_a_new_request() {
    let (_dir, mut store, epoch) = fresh();
    let original = selected(&mut store, epoch);
    store
        .save_restore_manifest(
            epoch,
            &NewRestoreManifest {
                restore_key: "transfer".into(),
                ..original
            },
        )
        .unwrap();
    assert!(matches!(
        store.freeze_restore_alias(epoch, "transfer", "A", b"request A"),
        Err(StoreError::RestoreAliasConflict(_))
    ));
    assert_eq!(
        store.restore_manifest("transfer").unwrap().unwrap().view,
        b"original A"
    );
}
