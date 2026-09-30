//! writer 세대 fencing·archived journal·journal 식별.

use crate::{
    CommitRequest, EffectState, EffectTransition, EventStore, ExpectedRevision, StoreError,
};

use super::common::{JOURNAL, append, db_path, fresh, new_effect, open};

fn cancel(effect_id: &str) -> EffectTransition {
    EffectTransition {
        effect_id: effect_id.to_owned(),
        from: EffectState::Pending,
        to: EffectState::Cancelled,
        resource_generation: 1,
        attempt: None,
        claim: None,
        result: None,
    }
}

/// 같은 저장소 안의 이전 writer·worker가 늦게 보낸 쓰기는 세대 검사가 막는다.
#[test]
fn old_epoch_is_fenced_after_the_writer_registers_again() {
    let (_dir, mut store, old_epoch) = fresh();
    let mut seed = CommitRequest::new(old_epoch);
    seed.effects
        .push(new_effect("fx-1", 1, EffectState::Pending));
    store.commit(&seed).expect("seed");

    let new_epoch = store.acquire_writer().expect("re-register");
    assert!(new_epoch > old_epoch);

    let mut late = CommitRequest::new(old_epoch);
    late.appends
        .push(append("engine-a", ExpectedRevision::Any, &["late"]));
    let err = store.commit(&late).expect_err("fenced commit");
    assert!(
        matches!(err, StoreError::Fenced { presented, current } if presented == old_epoch && current == new_epoch),
        "{err:?}"
    );
    assert!(matches!(
        store.transition_effect(old_epoch, &cancel("fx-1")),
        Err(StoreError::Fenced { .. })
    ));
    assert!(matches!(
        store.put_payload(old_epoch, b"x"),
        Err(StoreError::Fenced { .. })
    ));
    assert_eq!(store.current_cut().expect("cut").last_batch, None);

    let mut current = CommitRequest::new(new_epoch);
    current
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    store.commit(&current).expect("current writer");
    store
        .transition_effect(new_epoch, &cancel("fx-1"))
        .expect("current writer transition");
}

/// 다른 저장소(다른 프로세스 포함)는 잠금을 가진 writer가 있는 동안 writer가 될 수 없다.
#[test]
fn second_store_cannot_become_writer_while_the_lock_is_held() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut first, first_epoch) = open(&path);
    let mut other = EventStore::open(&path, JOURNAL).expect("open for reading");
    assert!(!other.is_writer());

    let err = other.acquire_writer().expect_err("lock is held");
    assert!(matches!(err, StoreError::WriterLocked), "{err:?}");
    assert_eq!(other.current_writer().expect("epoch"), first_epoch);

    // 잠금 없는 저장소는 현재 세대 값을 알아도 쓰지 못한다.
    let mut request = CommitRequest::new(first_epoch);
    request
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    assert!(matches!(other.commit(&request), Err(StoreError::NotWriter)));
    first.commit(&request).expect("lock holder writes");
    assert_eq!(other.read_batches_after(None, 10).expect("read").len(), 1);

    first.release_writer();
    assert!(matches!(first.commit(&request), Err(StoreError::NotWriter)));
    let second_epoch = other.acquire_writer().expect("lock released");
    assert!(second_epoch > first_epoch);
    assert!(matches!(
        first.acquire_writer(),
        Err(StoreError::WriterLocked)
    ));

    drop(other);
    first.acquire_writer().expect("lock released by drop");
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
