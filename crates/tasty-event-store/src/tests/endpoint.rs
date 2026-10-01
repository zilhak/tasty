//! Contract fixtures for scoped cursors and admission pressure.
use super::common::{append, fresh, new_effect};
use crate::*;
use std::collections::BTreeSet;

fn batch(store: &mut EventStore, epoch: WriterEpoch, stream: &str, event: &str) {
    let mut request = CommitRequest::new(epoch);
    request
        .appends
        .push(append(stream, ExpectedRevision::Any, &[event]));
    store.commit(&request).unwrap();
}
fn scoped(batch: u64) -> ScopedProjectionWrite {
    ScopedProjectionWrite {
        streams: BTreeSet::from([StreamId::new("a")]),
        write: ProjectionWrite {
            consumer_id: "subset".into(),
            projection_version: 1,
            batch_id: batch,
            upserts: vec![("name".into(), b"a".to_vec())],
            deletes: vec![],
        },
    }
}
#[test]
fn scope_and_same_cut_retry_cannot_change_output() {
    let (_dir, mut store, epoch) = fresh();
    batch(&mut store, epoch, "a", "a1");
    batch(&mut store, epoch, "b", "b1");
    let write = scoped(2);
    store.commit_scoped_projection(epoch, &write).unwrap();
    store.commit_scoped_projection(epoch, &write).unwrap();
    let before = store.scoped_projection_state("subset", 1).unwrap().unwrap();
    assert_eq!(before.cut.revisions.len(), 1);
    assert_eq!(before.cut.revisions[&StreamId::new("a")], 1);
    let mut changed = write.clone();
    changed.write.upserts[0].1 = b"changed".to_vec();
    assert!(matches!(
        store.commit_scoped_projection(epoch, &changed),
        Err(StoreError::ProjectionScope(_))
    ));
    changed = write.clone();
    changed.streams = BTreeSet::from([StreamId::new("b")]);
    assert!(matches!(
        store.replace_scoped_projection(epoch, &changed),
        Err(StoreError::ProjectionScope(_))
    ));
    assert!(matches!(
        store.projection_state("subset", 1),
        Err(StoreError::ProjectionScope(_))
    ));
    assert_eq!(
        store.scoped_projection_state("subset", 1).unwrap().unwrap(),
        before
    );
    changed.write.projection_version = 2;
    store.replace_scoped_projection(epoch, &changed).unwrap();
}
#[test]
fn unrelated_compaction_keeps_scope_but_missing_selected_history_requires_resync() {
    let (_dir, mut store, epoch) = fresh();
    batch(&mut store, epoch, "a", "a1");
    store.commit_scoped_projection(epoch, &scoped(1)).unwrap();
    batch(&mut store, epoch, "b", "b1");
    batch(&mut store, epoch, "a", "a2");
    for batch_id in [2, 3] {
        store
            .save_snapshot(
                epoch,
                &NewSnapshot {
                    batch_id,
                    model_version: 1,
                    bytes: vec![1],
                    referenced_payloads: vec![],
                },
            )
            .unwrap();
    }
    store.compact_history(epoch, 1).unwrap();
    assert!(store.scoped_projection_state("subset", 1).is_ok());
    batch(&mut store, epoch, "a", "a3");
    store
        .save_snapshot(
            epoch,
            &NewSnapshot {
                batch_id: 4,
                model_version: 1,
                bytes: vec![1],
                referenced_payloads: vec![],
            },
        )
        .unwrap();
    store.compact_history(epoch, 1).unwrap();
    assert!(matches!(
        store.scoped_projection_state("subset", 1),
        Err(StoreError::ResyncRequired { .. })
    ));
    assert!(matches!(
        store.commit_scoped_projection(epoch, &scoped(4)),
        Err(StoreError::ResyncRequired { .. })
    ));
    store.replace_scoped_projection(epoch, &scoped(4)).unwrap();
}
#[test]
fn prepared_holder_bound_and_existing_effect_completion_survive_pressure() {
    let (_dir, mut store, epoch) = fresh();
    store.admission_budget.command_credit_bytes = 4;
    store
        .put_admission_payload_pinned(epoch, b"1234", "prepare")
        .unwrap();
    assert!(matches!(
        store.put_admission_payload_pinned(epoch, b"5", "prepare"),
        Err(StoreError::AdmissionPayloadCapacity { .. })
    ));
    store.release_payload_holder(epoch, "prepare").unwrap();
    store.gc_payloads(epoch).unwrap();
    store.admission_budget.max_pending_effects = 1;
    let mut request = CommitRequest::new(epoch);
    request
        .appends
        .push(append("a", ExpectedRevision::Any, &["a1"]));
    request
        .effects
        .push(new_effect("one", 1, EffectState::Pending));
    store.commit(&request).unwrap();
    request.appends = vec![append("a", ExpectedRevision::Any, &["a2"])];
    request.effects = vec![new_effect("two", 1, EffectState::Pending)];
    assert!(matches!(
        store.commit(&request),
        Err(StoreError::PendingEffectCapacity { .. })
    ));
    let mut finish = CommitRequest::new(epoch);
    finish.effect_transitions.push(EffectTransition {
        effect_id: "one".into(),
        from: EffectState::Pending,
        to: EffectState::Cancelled,
        resource_generation: 1,
        attempt: None,
        claim: None,
        result: None,
    });
    store.commit(&finish).unwrap();
    store.commit(&request).unwrap();
}
