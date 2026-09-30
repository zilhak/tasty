//! projection 출력과 위치의 동시 갱신: 함께 적용되거나 둘 다 적용되지 않는다.

use std::collections::BTreeMap;

use crate::{
    CommitRequest, EventStore, ExpectedRevision, ProjectionWrite, StoreError, StreamId, WriterEpoch,
};

use super::common::{JOURNAL, append, db_path, fresh, open};

fn seed_batches(store: &mut EventStore, epoch: WriterEpoch, n: usize) {
    for i in 0..n {
        let mut request = CommitRequest::new(epoch);
        let id = format!("e{i}");
        request
            .appends
            .push(append("engine-a", ExpectedRevision::Any, &[&id]));
        store.commit(&request).expect("seed");
    }
}

fn write(batch_id: u64, upserts: &[(&str, &[u8])], deletes: &[&str]) -> ProjectionWrite {
    ProjectionWrite {
        consumer_id: "tab-list".to_owned(),
        projection_version: 1,
        batch_id,
        upserts: upserts
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.to_vec()))
            .collect(),
        deletes: deletes.iter().map(|k| (*k).to_owned()).collect(),
    }
}

fn rows(pairs: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.to_vec()))
        .collect()
}

#[test]
fn rows_and_position_move_together() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    seed_batches(&mut store, epoch, 3);

    let empty = store.projection_state("tab-list", 1).expect("empty");
    assert_eq!(empty.cut, None);
    assert!(empty.rows.is_empty());

    store
        .commit_projection(epoch, &write(1, &[("tab:1", b"a"), ("tab:2", b"b")], &[]))
        .expect("first");
    store
        .commit_projection(
            epoch,
            &write(3, &[("tab:2", b"B"), ("tab:3", b"c")], &["tab:1"]),
        )
        .expect("second");
    drop(store);

    let reader = EventStore::open(&path, JOURNAL).expect("reopen");
    let state = reader.projection_state("tab-list", 1).expect("state");
    let cut = state.cut.expect("position");
    assert_eq!(cut.last_batch, Some(3));
    assert_eq!(cut.heads[&StreamId::new("engine-a")], 3);
    assert_eq!(state.rows, rows(&[("tab:2", b"B"), ("tab:3", b"c")]));
}

#[test]
fn a_rejected_position_leaves_the_rows_untouched() {
    let (_dir, mut store, epoch) = fresh();
    seed_batches(&mut store, epoch, 3);
    store
        .commit_projection(epoch, &write(2, &[("tab:1", b"a")], &[]))
        .expect("first");
    let before = store.projection_state("tab-list", 1).expect("before");

    // 행을 쓴 뒤 위치 검사에서 실패하는 경우들. 행 변경도 함께 되돌아가야 한다.
    let err = store
        .commit_projection(epoch, &write(1, &[("tab:9", b"x")], &["tab:1"]))
        .expect_err("regression");
    assert!(
        matches!(err, StoreError::CheckpointRegression { .. }),
        "{err:?}"
    );
    let err = store
        .commit_projection(epoch, &write(42, &[("tab:9", b"x")], &["tab:1"]))
        .expect_err("unknown batch");
    assert!(matches!(err, StoreError::UnknownBatch(42)), "{err:?}");
    let err = store
        .commit_projection(epoch, &write(3, &[("tab:1", b"x")], &["tab:1"]))
        .expect_err("key conflict");
    assert!(
        matches!(err, StoreError::ProjectionKeyConflict(_)),
        "{err:?}"
    );
    let err = store
        .commit_projection(epoch, &write(3, &[("tab:5", b"x"), ("tab:5", b"y")], &[]))
        .expect_err("duplicate upsert");
    assert!(
        matches!(err, StoreError::ProjectionKeyConflict(_)),
        "{err:?}"
    );

    assert_eq!(
        store.projection_state("tab-list", 1).expect("after"),
        before
    );
}

#[test]
fn only_the_current_writer_can_commit_a_projection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, old_epoch) = open(&path);
    seed_batches(&mut store, old_epoch, 1);
    let new_epoch = store.acquire_writer().expect("re-register");
    let err = store
        .commit_projection(old_epoch, &write(1, &[("tab:1", b"a")], &[]))
        .expect_err("fenced");
    assert!(matches!(err, StoreError::Fenced { .. }), "{err:?}");

    let mut reader = EventStore::open(&path, JOURNAL).expect("reader");
    let err = reader
        .commit_projection(new_epoch, &write(1, &[("tab:1", b"a")], &[]))
        .expect_err("no lock");
    assert!(matches!(err, StoreError::NotWriter), "{err:?}");

    let state = store.projection_state("tab-list", 1).expect("state");
    assert_eq!(state.cut, None);
    assert!(state.rows.is_empty());
}

#[test]
fn projection_versions_keep_separate_rows_and_positions() {
    let (_dir, mut store, epoch) = fresh();
    seed_batches(&mut store, epoch, 2);
    store
        .commit_projection(epoch, &write(2, &[("tab:1", b"v1")], &[]))
        .expect("v1");
    let mut v2 = write(1, &[("tab:1", b"v2")], &[]);
    v2.projection_version = 2;
    store
        .commit_projection(epoch, &v2)
        .expect("v2 starts on its own");

    let one = store.projection_state("tab-list", 1).expect("v1");
    let two = store.projection_state("tab-list", 2).expect("v2");
    assert_eq!(one.rows, rows(&[("tab:1", b"v1")]));
    assert_eq!(two.rows, rows(&[("tab:1", b"v2")]));
    assert_eq!(one.cut.and_then(|c| c.last_batch), Some(2));
    assert_eq!(two.cut.and_then(|c| c.last_batch), Some(1));
}

#[test]
fn a_consumer_with_rows_cannot_move_only_its_position() {
    let (_dir, mut store, epoch) = fresh();
    seed_batches(&mut store, epoch, 3);
    store
        .commit_projection(epoch, &write(1, &[("tab:1", b"a")], &[]))
        .expect("rows");
    let before = store.projection_state("tab-list", 1).expect("before");

    for version in [1, 2] {
        assert!(matches!(
            store.save_checkpoint(epoch, "tab-list", version, 3),
            Err(StoreError::CheckpointOwnedByProjection(ref c)) if c == "tab-list"
        ));
    }
    assert_eq!(
        store.projection_state("tab-list", 1).expect("after"),
        before
    );
    assert_eq!(store.checkpoint("tab-list", 2).expect("v2"), None);

    store
        .save_checkpoint(epoch, "remote-1", 1, 3)
        .expect("a consumer without rows still saves its position");
    assert_eq!(
        store
            .checkpoint("remote-1", 1)
            .expect("remote")
            .and_then(|c| c.last_batch),
        Some(3)
    );
}
