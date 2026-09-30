//! writer 세대 fencing·archived journal·journal 식별.

use crate::{
    CommitRequest, EffectState, EffectTransition, EventStore, ExpectedRevision, StoreError,
};

use super::common::{JOURNAL, append, db_path, new_effect, open};

#[test]
fn old_writer_is_fenced_after_a_new_writer_registers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut old, old_epoch) = open(&path);
    let mut seed = CommitRequest::new(old_epoch);
    seed.effects
        .push(new_effect("fx-1", 1, EffectState::Pending));
    old.commit(&seed).expect("seed");

    let (mut new, new_epoch) = open(&path);
    assert!(new_epoch > old_epoch);

    let mut late = CommitRequest::new(old_epoch);
    late.appends
        .push(append("engine-a", ExpectedRevision::Any, &["late"]));
    let err = old.commit(&late).expect_err("fenced commit");
    assert!(
        matches!(err, StoreError::Fenced { presented, current } if presented == old_epoch && current == new_epoch),
        "{err:?}"
    );

    let cancel = EffectTransition {
        effect_id: "fx-1".to_owned(),
        from: EffectState::Pending,
        to: EffectState::Cancelled,
        resource_generation: 1,
        attempt: None,
        claim: None,
        result: None,
    };
    assert!(matches!(
        old.transition_effect(old_epoch, &cancel),
        Err(StoreError::Fenced { .. })
    ));
    assert!(matches!(
        old.put_payload(old_epoch, b"x"),
        Err(StoreError::Fenced { .. })
    ));
    assert_eq!(old.current_cut().expect("cut").last_batch, None);

    let mut fresh = CommitRequest::new(new_epoch);
    fresh
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    new.commit(&fresh).expect("current writer");
    new.transition_effect(new_epoch, &cancel)
        .expect("current writer transition");
}

#[test]
fn archived_journal_accepts_reads_but_no_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let mut request = CommitRequest::new(epoch);
    request
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    store.commit(&request).expect("commit");
    store.archive(epoch).expect("archive");
    assert!(store.is_archived().expect("status"));

    assert!(matches!(
        store.commit(&request),
        Err(StoreError::JournalArchived)
    ));
    assert!(matches!(
        store.acquire_writer(),
        Err(StoreError::JournalArchived)
    ));
    drop(store);

    let store = EventStore::open(&path, JOURNAL).expect("reopen archived");
    assert_eq!(store.read_batches_after(None, 10).expect("read").len(), 1);
}

#[test]
fn opening_with_another_journal_id_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    drop(open(&path));
    let err = EventStore::open(&path, "other-journal")
        .err()
        .expect("mismatch");
    assert!(matches!(err, StoreError::JournalMismatch { .. }), "{err:?}");
}
