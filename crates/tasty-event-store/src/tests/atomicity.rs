//! 원자 commit·revision 충돌·다중 stream batch 공개.

use std::thread;
use std::time::{Duration, Instant};

use crate::{
    CommitOutcome, CommitRequest, EffectState, EventStore, ExpectedRevision, PayloadRef,
    StoreError, StreamId,
};

use super::common::{JOURNAL, append, command, db_path, event, fresh, key, new_effect, open};

fn committed(outcome: CommitOutcome) -> crate::BatchCut {
    match outcome {
        CommitOutcome::Committed { batch: Some(cut) } => cut,
        other => panic!("expected a committed batch, got {other:?}"),
    }
}

#[test]
fn failure_after_events_leaves_nothing_after_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let mut seed = CommitRequest::new(epoch);
    seed.effects
        .push(new_effect("fx-1", 1, EffectState::Pending));
    store.commit(&seed).expect("seed effect");

    // 이벤트·명령을 쓴 뒤 effect 삽입에서 기본키 충돌이 나도록 만든다.
    let mut request = CommitRequest::new(epoch);
    request.command = Some(command("cmd-1", Some(key("agent", "k1")), b"d1"));
    request.appends.push(append(
        "engine-a",
        ExpectedRevision::NoStream,
        &["e1", "e2"],
    ));
    request
        .effects
        .push(new_effect("fx-1", 1, EffectState::Pending));
    let err = store
        .commit(&request)
        .expect_err("duplicate effect id must fail");
    assert!(matches!(err, StoreError::Sqlite(_)), "{err:?}");
    drop(store);

    let store = EventStore::open(&path, JOURNAL).expect("reopen");
    let engine = StreamId::new("engine-a");
    assert_eq!(store.stream_revision(&engine).expect("head"), None);
    assert!(
        store
            .read_stream(&engine, None, 10)
            .expect("read")
            .is_empty()
    );
    assert_eq!(store.command("cmd-1").expect("command"), None);
    assert_eq!(store.current_cut().expect("cut").last_batch, None);
}

#[test]
fn revision_conflict_rejects_and_reports_actual() {
    let (_dir, mut store, epoch) = fresh();
    let mut first = CommitRequest::new(epoch);
    first
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    store.commit(&first).expect("first");

    for expected in [
        ExpectedRevision::NoStream,
        ExpectedRevision::Exact(0),
        ExpectedRevision::Exact(2),
    ] {
        let mut stale = CommitRequest::new(epoch);
        stale
            .appends
            .push(append("engine-a", expected, &["e-stale"]));
        let err = store.commit(&stale).expect_err("stale expected revision");
        assert!(
            matches!(
                err,
                StoreError::RevisionConflict {
                    actual: Some(1),
                    ..
                }
            ),
            "{err:?}"
        );
    }

    let mut next = CommitRequest::new(epoch);
    next.appends.push(append(
        "engine-a",
        ExpectedRevision::Exact(1),
        &["e2", "e3"],
    ));
    let cut = committed(store.commit(&next).expect("next"));
    assert_eq!(cut.revisions[&StreamId::new("engine-a")], 3);

    let mut any = CommitRequest::new(epoch);
    any.appends
        .push(append("engine-a", ExpectedRevision::Any, &["e4"]));
    let cut = committed(store.commit(&any).expect("any"));
    assert_eq!(cut.revisions[&StreamId::new("engine-a")], 4);
}

#[test]
fn multi_stream_batch_is_all_or_nothing() {
    let (_dir, mut store, epoch) = fresh();
    let mut seed = CommitRequest::new(epoch);
    seed.appends
        .push(append("engine-b", ExpectedRevision::NoStream, &["b1"]));
    store.commit(&seed).expect("seed");

    // 첫 stream은 유효하고 두 번째 stream의 전제가 틀렸다.
    let mut bad = CommitRequest::new(epoch);
    bad.appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["a1"]));
    bad.appends
        .push(append("engine-b", ExpectedRevision::NoStream, &["b2"]));
    store.commit(&bad).expect_err("second stream conflict");
    assert_eq!(
        store
            .stream_revision(&StreamId::new("engine-a"))
            .expect("head"),
        None
    );

    let mut good = CommitRequest::new(epoch);
    good.appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["a1"]));
    good.appends.push(append(
        "engine-b",
        ExpectedRevision::Exact(1),
        &["b2", "b3"],
    ));
    let cut = committed(store.commit(&good).expect("good"));
    assert_eq!(cut.revisions.len(), 2);
    assert_eq!(cut.revisions[&StreamId::new("engine-a")], 1);
    assert_eq!(cut.revisions[&StreamId::new("engine-b")], 3);

    let batch = store.read_batch(cut.batch_id).expect("batch");
    let ids: Vec<_> = batch.events.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(ids, ["a1", "b2", "b3"]);
    let indexes: Vec<_> = batch.events.iter().map(|e| e.batch_index).collect();
    assert_eq!(indexes, [0, 1, 2]);

    let whole = store.cut_at(cut.batch_id).expect("cut at");
    assert_eq!(whole.heads, store.current_cut().expect("current").heads);
}

#[test]
fn malformed_appends_are_rejected() {
    let (_dir, mut store, epoch) = fresh();
    let mut empty = CommitRequest::new(epoch);
    empty
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &[]));
    assert!(matches!(
        store.commit(&empty),
        Err(StoreError::EmptyAppend(_))
    ));

    let mut twice = CommitRequest::new(epoch);
    twice
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &["a1"]));
    twice
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &["a2"]));
    assert!(matches!(
        store.commit(&twice),
        Err(StoreError::DuplicateStream(_))
    ));

    // 준비되지 않은 payload를 참조하면 이벤트까지 모두 되돌린다.
    let mut missing = CommitRequest::new(epoch);
    let mut ev = event("a1");
    ev.payload_refs.push(PayloadRef(999));
    missing.appends.push(crate::StreamAppend {
        stream_id: StreamId::new("engine-a"),
        expected: ExpectedRevision::NoStream,
        events: vec![ev],
    });
    assert!(matches!(
        store.commit(&missing),
        Err(StoreError::PayloadMissing(999))
    ));
    assert_eq!(store.current_cut().expect("cut").last_batch, None);
}

/// 다른 연결의 reader는 batch를 전부 보거나 전혀 보지 않는다.
#[test]
fn concurrent_reader_never_sees_a_partial_batch() {
    const BATCHES: usize = 200;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut writer, epoch) = open(&path);
    let reader = EventStore::open(&path, JOURNAL).expect("reader");

    let handle = thread::spawn(move || {
        for i in 0..BATCHES {
            let mut request = CommitRequest::new(epoch);
            let a = format!("a{i}");
            let b = format!("b{i}");
            request
                .appends
                .push(append("engine-a", ExpectedRevision::Any, &[&a]));
            request
                .appends
                .push(append("engine-b", ExpectedRevision::Any, &[&b]));
            writer.commit(&request).expect("writer commit");
        }
    });

    let a = StreamId::new("engine-a");
    let b = StreamId::new("engine-b");
    let mut observations = 0;
    // writer가 panic하거나 멈추면 무한 대기하지 않고 실패한다.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let writer_done = handle.is_finished();
        let cut = reader.current_cut().expect("cut");
        assert_eq!(
            cut.heads.get(&a),
            cut.heads.get(&b),
            "partial batch: {cut:?}"
        );
        let recent = cut.last_batch.map(|last| last.saturating_sub(3));
        for batch in reader.read_batches_after(recent, 8).expect("batches") {
            assert_eq!(batch.events.len(), 2);
            assert_eq!(batch.cut.revisions.len(), 2);
        }
        observations += 1;
        if cut.heads.get(&a) == Some(&(BATCHES as u64)) || writer_done {
            break;
        }
        assert!(Instant::now() < deadline, "writer did not finish: {cut:?}");
    }
    handle.join().expect("writer thread");
    let last = reader.current_cut().expect("final cut");
    assert_eq!(last.heads.get(&a), Some(&(BATCHES as u64)));
    assert!(observations > 0);
}
