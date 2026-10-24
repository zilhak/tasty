//! Revision equality is necessary but does not replace semantic stream equality.
use tasty_core::StructureModels;
use tasty_event_store::{CommitRequest, ExpectedRevision, NewSnapshot, StreamAppend, StreamId};

#[test]
fn maintenance_rebases_only_an_identical_decoded_stream_and_preserves_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut store =
        tasty_event_store::EventStore::open(&dir.path().join("journal.db"), "test").unwrap();
    let epoch = store.acquire_writer().unwrap();
    let mut models = StructureModels::default();
    for batch in 1..=2 {
        let stream = if batch == 1 {
            "structure:slot-2"
        } else {
            "structure:slot-1"
        };
        let mut request = CommitRequest::new(epoch);
        request.appends.push(StreamAppend {
            stream_id: StreamId::new(stream),
            expected: ExpectedRevision::Any,
            events: vec![tasty_event_store::NewEvent {
                event_id: format!("event-{batch}"),
                payload: tasty_event_store::OpaquePayload {
                    type_tag: "test".into(),
                    schema_version: 1,
                    bytes: vec![],
                },
                recorded_at_ms: 0,
                causation_id: None,
                actor: "test".into(),
                origin: "test".into(),
                payload_refs: vec![],
            }],
        });
        store.commit(&request).unwrap();
        models.batch = Some(batch);
        let model = tasty_core::JournalModel {
            applied: tasty_core::Applied {
                batch: Some(batch),
                revision: Some(1),
            },
            ..Default::default()
        };
        models.streams.insert(stream.into(), model);
        if batch == 1 {
            let view = super::super::view_record::StoredView::imported(
                super::super::EngineBinding {
                    journal_id: "test".into(),
                    stream: stream.into(),
                    incarnation: 0,
                    runtime_epoch: epoch.0,
                    published_cut: Some(1),
                    revision: Some(1),
                },
                Default::default(),
            );
            store
                .save_restore_checkpoint(
                    epoch,
                    &snapshot(&models),
                    &tasty_event_store::NewRestoreManifest {
                        restore_key: "view:slot-2".into(),
                        incarnation: 0,
                        runtime_epoch: epoch.0,
                        sequence: 0,
                        snapshot_id: 0,
                        view: serde_json::to_vec(&view).unwrap(),
                        referenced_payloads: vec![],
                    },
                )
                .unwrap();
        }
    }
    models
        .streams
        .get_mut("structure:slot-2")
        .unwrap()
        .applied
        .batch = Some(2);
    let before = store.restore_manifest("view:slot-2").unwrap().unwrap();
    let target = store.save_snapshot(epoch, &snapshot(&models)).unwrap();
    let mut changed = models.clone();
    changed
        .streams
        .get_mut("structure:slot-2")
        .unwrap()
        .activation_high_water
        .insert(99, 3);
    super::manifest_rebase::rebase(&mut store, epoch, &changed, target).unwrap();
    assert_eq!(
        store
            .restore_manifest("view:slot-2")
            .unwrap()
            .unwrap()
            .snapshot
            .snapshot_id,
        before.snapshot.snapshot_id
    );
    super::manifest_rebase::rebase(&mut store, epoch, &models, target).unwrap();
    let after = store.restore_manifest("view:slot-2").unwrap().unwrap();
    assert_eq!(after.snapshot.snapshot_id, target);
    assert_eq!(after.view, before.view);
    super::super::view_record::check_manifest(
        &after,
        &super::super::EngineBinding {
            journal_id: "test".into(),
            stream: "structure:slot-2".into(),
            incarnation: 0,
            runtime_epoch: epoch.0,
            published_cut: Some(2),
            revision: Some(1),
        },
    )
    .unwrap();
}

fn snapshot(models: &StructureModels) -> NewSnapshot {
    NewSnapshot {
        batch_id: models.batch.unwrap(),
        model_version: tasty_core::MODEL_VERSION,
        bytes: tasty_core::encode_snapshot(models).unwrap(),
        referenced_payloads: vec![],
    }
}
