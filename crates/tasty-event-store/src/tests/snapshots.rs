//! snapshot+tail 재구성·손상 snapshot·불변 payload·checkpoint.

use std::collections::BTreeMap;

use crate::{
    CommitOutcome, CommitRequest, EventStore, ExpectedRevision, NewSnapshot, StoreError,
    StoredBatch, StreamAppend, StreamId, WriterEpoch, event_holder, snapshot_holder,
};

use super::common::{append, db_path, event, fresh, open, raw};

const MODEL: u32 = 1;

/// 시험용 모델: stream별 이벤트 id 목록.
type Model = BTreeMap<String, Vec<String>>;

fn evolve(model: &mut Model, batches: &[StoredBatch]) {
    for batch in batches {
        for event in &batch.events {
            model
                .entry(event.stream_id.0.clone())
                .or_default()
                .push(event.event_id.clone());
        }
    }
}

fn encode(model: &Model) -> Vec<u8> {
    model
        .iter()
        .map(|(stream, ids)| format!("{stream}={}\n", ids.join(",")))
        .collect::<String>()
        .into_bytes()
}

fn decode(bytes: &[u8]) -> Model {
    let text = String::from_utf8(bytes.to_vec()).expect("utf8");
    text.lines()
        .map(|line| {
            let (stream, ids) = line.split_once('=').expect("line");
            (
                stream.to_owned(),
                ids.split(',').map(str::to_owned).collect(),
            )
        })
        .collect()
}

fn commit_round(store: &mut EventStore, epoch: WriterEpoch, round: usize) -> u64 {
    let mut request = CommitRequest::new(epoch);
    let a = format!("a{round}");
    let b = format!("b{round}");
    request
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &[&a]));
    if round.is_multiple_of(2) {
        request
            .appends
            .push(append("engine-b", ExpectedRevision::Any, &[&b]));
    }
    match store.commit(&request).expect("commit") {
        CommitOutcome::Committed { batch: Some(cut) } => cut.batch_id,
        other => panic!("{other:?}"),
    }
}

fn full_replay(store: &EventStore) -> Model {
    let mut model = Model::new();
    evolve(
        &mut model,
        &store.read_batches_after(None, usize::MAX).expect("all"),
    );
    model
}

fn rebuild(store: &EventStore) -> (Model, crate::Replay) {
    let replay = store.snapshot_and_tail(MODEL).expect("replay");
    let mut model = replay
        .snapshot
        .as_ref()
        .map(|s| decode(&s.bytes))
        .unwrap_or_default();
    evolve(&mut model, &replay.tail);
    (model, replay)
}

fn snapshot_now(store: &mut EventStore, epoch: WriterEpoch, batch_id: u64) -> u64 {
    let mut model = Model::new();
    evolve(
        &mut model,
        &store
            .read_batches_after(None, batch_id as usize)
            .expect("prefix"),
    );
    store
        .save_snapshot(
            epoch,
            &NewSnapshot {
                batch_id,
                model_version: MODEL,
                bytes: encode(&model),
                referenced_payloads: Vec::new(),
            },
        )
        .expect("snapshot")
}

#[test]
fn snapshot_plus_tail_equals_full_replay() {
    let (_dir, mut store, epoch) = fresh();
    let mut last = 0;
    for round in 0..6 {
        last = commit_round(&mut store, epoch, round);
    }
    snapshot_now(&mut store, epoch, last - 2);
    for round in 6..9 {
        commit_round(&mut store, epoch, round);
    }

    let (model, replay) = rebuild(&store);
    assert_eq!(model, full_replay(&store));
    let snapshot = replay.snapshot.expect("snapshot used");
    assert_eq!(snapshot.cut, store.cut_at(last - 2).expect("cut"));
    assert_eq!(replay.tail.first().map(|b| b.cut.batch_id), Some(last - 1));
    assert_eq!(model["engine-a"].len(), 9);
}

#[test]
fn corrupt_snapshot_falls_back_to_an_older_verified_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    for round in 0..4 {
        commit_round(&mut store, epoch, round);
    }
    let older = snapshot_now(&mut store, epoch, 2);
    let newer = snapshot_now(&mut store, epoch, 4);
    // 다른 model version의 snapshot은 고르지 않는다.
    store
        .save_snapshot(
            epoch,
            &NewSnapshot {
                batch_id: 4,
                model_version: MODEL + 1,
                bytes: b"future".to_vec(),
                referenced_payloads: Vec::new(),
            },
        )
        .expect("other model");

    raw(&path)
        .execute(
            "UPDATE payloads SET bytes = X'00' WHERE payload_id =
                (SELECT payload_id FROM snapshots WHERE snapshot_id = ?1)",
            [newer as i64],
        )
        .expect("corrupt the newest snapshot");

    let (model, replay) = rebuild(&store);
    assert_eq!(replay.snapshot.as_ref().map(|s| s.snapshot_id), Some(older));
    assert_eq!(replay.rejected.len(), 1);
    assert_eq!(replay.rejected[0].snapshot_id, newer);
    assert_eq!(model, full_replay(&store));

    let missing = store.save_snapshot(
        epoch,
        &NewSnapshot {
            batch_id: 99,
            model_version: MODEL,
            bytes: Vec::new(),
            referenced_payloads: Vec::new(),
        },
    );
    assert!(matches!(missing, Err(StoreError::UnknownBatch(99))));
}

#[test]
fn payload_is_deleted_only_after_every_reference_is_released() {
    let (_dir, mut store, epoch) = fresh();
    let first = store.put_payload(epoch, b"scrollback v1").expect("put");
    let mut ev = event("recorded-1");
    ev.payload_refs.push(first);
    let mut request = CommitRequest::new(epoch);
    request.appends.push(StreamAppend {
        stream_id: StreamId::new("engine-a"),
        expected: ExpectedRevision::NoStream,
        events: vec![ev],
    });
    store.commit(&request).expect("record");

    // 다시 캡처하면 새 generation이 생기고 이전 내용은 바뀌지 않는다.
    let second = store
        .put_payload(epoch, b"scrollback v2")
        .expect("recapture");
    assert_ne!(first, second);
    let snapshot = store
        .save_snapshot(
            epoch,
            &NewSnapshot {
                batch_id: 1,
                model_version: MODEL,
                bytes: b"model".to_vec(),
                referenced_payloads: vec![second],
            },
        )
        .expect("snapshot");
    store
        .pin_payload(epoch, second, "undo:1")
        .expect("undo pin");
    let orphan = store
        .put_payload(epoch, b"never referenced")
        .expect("orphan");

    assert_eq!(store.gc_payloads(epoch).expect("gc"), 1);
    assert!(matches!(
        store.read_payload(orphan),
        Err(StoreError::PayloadMissing(_))
    ));
    assert_eq!(store.read_payload(first).expect("v1"), b"scrollback v1");
    assert_eq!(
        store.payload_holders(first).expect("holders"),
        [event_holder("recorded-1")]
    );

    store
        .delete_snapshot(epoch, snapshot)
        .expect("drop snapshot");
    assert_eq!(
        store.gc_payloads(epoch).expect("gc"),
        1,
        "only the snapshot body"
    );
    assert_eq!(
        store.read_payload(second).expect("still pinned by undo"),
        b"scrollback v2"
    );

    store
        .unpin_payload(epoch, second, "undo:1")
        .expect("undo consumed");
    assert!(store.payload_holders(second).expect("holders").is_empty());
    assert_eq!(store.gc_payloads(epoch).expect("gc"), 1);
    assert!(matches!(
        store.read_payload(second),
        Err(StoreError::PayloadMissing(_))
    ));
    assert_eq!(snapshot_holder(snapshot), format!("snapshot:{snapshot}"));
}

#[test]
fn checkpoints_carry_a_revision_vector_and_never_move_back() {
    let (_dir, mut store, epoch) = fresh();
    for round in 0..3 {
        commit_round(&mut store, epoch, round);
    }
    assert_eq!(store.checkpoint("remote-1", 1).expect("none"), None);
    store
        .save_checkpoint(epoch, "remote-1", 1, 2)
        .expect("save");
    let cut = store
        .checkpoint("remote-1", 1)
        .expect("read")
        .expect("exists");
    assert_eq!(cut.last_batch, Some(2));
    assert_eq!(cut.heads[&StreamId::new("engine-a")], 2);
    assert_eq!(cut.heads[&StreamId::new("engine-b")], 1);

    let err = store
        .save_checkpoint(epoch, "remote-1", 1, 1)
        .expect_err("regression");
    assert!(
        matches!(err, StoreError::CheckpointRegression { .. }),
        "{err:?}"
    );
    // projection version이 다르면 별도 위치다.
    store
        .save_checkpoint(epoch, "remote-1", 2, 1)
        .expect("other version");
    assert!(matches!(
        store.save_checkpoint(epoch, "remote-1", 1, 42),
        Err(StoreError::UnknownBatch(42))
    ));
}

#[test]
fn live_snapshot_transfer_rolls_back_invalid_references_and_keeps_history() {
    let (_dir, mut store, epoch) = fresh();
    let first = store.put_payload_pinned(epoch, b"old", "admission/1/1").expect("put");
    let second = store.put_payload_pinned(epoch, b"new", "import/source").expect("put");
    let consumer = "structure-maintenance";
    let first_batch = commit_round(&mut store, epoch, 0);
    let snapshot = NewSnapshot {
        batch_id: first_batch, model_version: MODEL, bytes: b"first".to_vec(),
        referenced_payloads: vec![first],
    };
    let first_snapshot = store.save_live_snapshot(epoch, &snapshot, consumer).expect("save");
    store.release_payload_holder(epoch, "admission/1/1").expect("release");
    let second_batch = commit_round(&mut store, epoch, 1);
    let invalid = NewSnapshot {
        batch_id: second_batch, bytes: b"invalid".to_vec(),
        referenced_payloads: vec![crate::PayloadRef(u64::MAX / 2)], ..snapshot.clone()
    };
    assert!(matches!(store.save_live_snapshot(epoch, &invalid, consumer), Err(StoreError::PayloadMissing(_))));
    assert_eq!(store.checkpoint(consumer, MODEL).expect("cursor").expect("cut").last_batch, Some(first_batch));
    assert!(store.payload_holders(first).expect("pins").contains(&format!("live:{consumer}")));
    let valid = NewSnapshot { referenced_payloads: vec![second], ..invalid };
    store.save_live_snapshot(epoch, &valid, consumer).expect("replace");
    assert!(store.payload_holders(first).expect("pins").contains(&snapshot_holder(first_snapshot)));
    assert!(!store.payload_holders(first).expect("pins").contains(&format!("live:{consumer}")));
    assert!(store.payload_holders(second).expect("pins").contains(&"import/source".to_owned()));
    let third_batch = commit_round(&mut store, epoch, 2);
    store.save_live_snapshot(epoch, &NewSnapshot { batch_id: third_batch, ..valid }, consumer).expect("retention");
    assert!(matches!(store.read_payload(first), Err(StoreError::PayloadMissing(_))));
    assert_eq!(store.read_batches_after(None, usize::MAX).expect("history").len(), 3);
}

#[test]
fn damaged_snapshot_dependency_uses_previous_snapshot() {
    let dir = tempfile::tempdir().expect("directory");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    commit_round(&mut store, epoch, 0);
    let older = snapshot_now(&mut store, epoch, 1);
    let dependency = store.put_payload(epoch, b"surface").expect("payload");
    commit_round(&mut store, epoch, 1);
    store.save_snapshot(epoch, &NewSnapshot {
        batch_id: 2, model_version: MODEL, bytes: b"newer".to_vec(), referenced_payloads: vec![dependency],
    }).expect("snapshot");
    raw(&path).execute("UPDATE payloads SET bytes = X'00' WHERE payload_id = ?1", [dependency.0 as i64]).expect("corrupt");
    let replay = store.snapshot_and_tail(MODEL).expect("replay");
    assert_eq!(replay.snapshot.expect("fallback").snapshot_id, older);
    assert_eq!(replay.rejected.len(), 1);
}

#[test]
fn compaction_retains_fallback_tail_and_rejects_old_cursors() {
    let (dir, mut store, epoch) = fresh();
    for round in 0..5 {commit_round(&mut store, epoch, round);}
    store.save_checkpoint(epoch, "slow", MODEL, 1).expect("slow cursor");
    let older = snapshot_now(&mut store, epoch, 2);
    let newer = snapshot_now(&mut store, epoch, 4);
    let before = full_replay(&store);
    let result = store.compact_history(epoch, MODEL).expect("compact").expect("boundary");
    assert_eq!(result.retained_after_batch, 2);
    assert!(matches!(store.read_batches_after(None, 10), Err(StoreError::ResyncRequired {..})));
    assert!(matches!(store.read_stream(&StreamId::new("engine-a"), Some(1), 10), Err(StoreError::ResyncRequired {..})));
    assert!(matches!(store.checkpoint("slow", MODEL), Err(StoreError::ResyncRequired {..})));
    assert!(store.delete_snapshot(epoch, older).is_err());
    assert_eq!(rebuild(&store).0, before);
    raw(&db_path(&dir)).execute("UPDATE payloads SET bytes = X'00' WHERE payload_id = (SELECT payload_id FROM snapshots WHERE snapshot_id = ?1)", [newer as i64]).expect("damage new");
    assert_eq!(rebuild(&store).0, before);
    raw(&db_path(&dir)).execute("UPDATE payloads SET bytes = X'00' WHERE payload_id = (SELECT payload_id FROM snapshots WHERE snapshot_id = ?1)", [older as i64]).expect("damage anchor");
    assert!(matches!(store.snapshot_and_tail(MODEL), Err(StoreError::ResyncRequired {..})));
}
