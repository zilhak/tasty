//! 영속 ID 예약: 겹치지 않는 범위·빈 구간·fencing·값 공간 소진.

use crate::{CommitRequest, EventStore, ExpectedRevision, StoreError};

use super::common::{JOURNAL, append, db_path, fresh, open};

const MAX: u64 = u32::MAX as u64;

#[test]
fn ranges_never_overlap_across_kinds_and_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    assert_eq!(store.next_unreserved_id("pane").expect("next"), 1);
    let first = store.reserve_ids(epoch, "pane", 3, MAX).expect("first");
    assert_eq!(first.ids(), 1..4);
    let other = store.reserve_ids(epoch, "tab", 2, MAX).expect("other kind");
    assert_eq!(other.ids(), 1..3, "each kind has its own space");
    drop(store);

    let (mut store, epoch) = open(&path);
    let second = store
        .reserve_ids(epoch, "pane", 5, MAX)
        .expect("after reopen");
    assert_eq!(second.ids(), 4..9);
    assert!(first.ids().all(|id| !second.contains(id)));
    assert_eq!(store.next_unreserved_id("pane").expect("next"), 9);
}

#[test]
fn a_failed_commit_leaves_its_reserved_ids_unused() {
    let (_dir, mut store, epoch) = fresh();
    let reserved = store
        .reserve_ids(epoch, "surface", 4, MAX)
        .expect("reserve");

    // 예약한 ID를 쓰려던 commit이 revision 충돌로 실패해도 예약은 되돌리지 않는다.
    let mut seed = CommitRequest::new(epoch);
    seed.appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    store.commit(&seed).expect("seed");
    let mut stale = CommitRequest::new(epoch);
    stale.appends.push(append(
        "engine-a",
        ExpectedRevision::NoStream,
        &["uses-reserved"],
    ));
    store.commit(&stale).expect_err("conflict");

    let next = store.reserve_ids(epoch, "surface", 1, MAX).expect("next");
    assert_eq!(
        next.start, reserved.end,
        "the unused range is a gap, not reused"
    );
}

#[test]
fn only_the_current_writer_can_reserve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, old_epoch) = open(&path);
    let new_epoch = store.acquire_writer().expect("re-register");
    let err = store
        .reserve_ids(old_epoch, "pane", 1, MAX)
        .expect_err("fenced");
    assert!(matches!(err, StoreError::Fenced { .. }), "{err:?}");

    let mut reader = EventStore::open(&path, JOURNAL).expect("reader");
    let err = reader
        .reserve_ids(new_epoch, "pane", 1, MAX)
        .expect_err("no lock");
    assert!(matches!(err, StoreError::NotWriter), "{err:?}");
    assert_eq!(store.next_unreserved_id("pane").expect("next"), 1);

    let range = store
        .reserve_ids(new_epoch, "pane", 1, MAX)
        .expect("writer");
    assert_eq!(range.start, 1);
}

#[test]
fn an_exhausted_space_is_refused_without_wrapping() {
    let (_dir, mut store, epoch) = fresh();
    let err = store.reserve_ids(epoch, "pane", 0, MAX).expect_err("zero");
    assert!(matches!(err, StoreError::EmptyReservation(_)), "{err:?}");

    let all = store.reserve_ids(epoch, "pane", 9, 10).expect("up to max");
    assert_eq!(all.ids(), 1..10);
    let last = store
        .reserve_ids(epoch, "pane", 1, 10)
        .expect("exactly max");
    assert_eq!(last.ids(), 10..11);
    let err = store
        .reserve_ids(epoch, "pane", 1, 10)
        .expect_err("past max");
    assert!(
        matches!(err, StoreError::IdSpaceExhausted { next: 11, .. }),
        "{err:?}"
    );
    assert_eq!(store.next_unreserved_id("pane").expect("next"), 11);

    let err = store
        .reserve_ids(epoch, "big", u64::MAX, u64::MAX)
        .expect_err("overflow");
    assert!(
        matches!(err, StoreError::IdSpaceExhausted { .. }),
        "{err:?}"
    );
    assert_eq!(store.next_unreserved_id("big").expect("next"), 1);
}
