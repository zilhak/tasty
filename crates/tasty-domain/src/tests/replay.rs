//! 실제 journal에서의 replay: 전체 로그와 snapshot+tail의 일치(E01), 모르는 tag 중단.

use tasty_event_store::{
    CommitRequest, ExpectedRevision, NewEvent, OpaquePayload, PayloadRef, StreamAppend, StreamId,
};

use super::common::{commit_events, db_path, open, scenario};
use crate::{
    CodecError, EvolveError, JournalModel, ReplayError, STRUCTURE_STREAM, full_replay, load,
    save_snapshot,
};

#[test]
fn full_replay_equals_snapshot_plus_tail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    // 시나리오의 markdown surface가 참조하는 자료. snapshot이 이를 pin한다.
    let notes = store.put_payload(epoch, b"notes").expect("payload");
    assert_eq!(notes, PayloadRef(1));
    let batches = scenario();
    for events in &batches[..3] {
        commit_events(&mut store, epoch, events);
    }
    let mid = load(&store).expect("load without snapshot");
    save_snapshot(&mut store, epoch, &mid).expect("snapshot");
    for events in &batches[3..] {
        commit_events(&mut store, epoch, events);
    }

    let full = full_replay(&store).expect("full replay");
    let fast = load(&store).expect("snapshot + tail");
    assert_eq!(full, fast);
    let revision = store
        .stream_revision(&StreamId::new(STRUCTURE_STREAM))
        .expect("head");
    assert_eq!(fast.applied.revision, revision);
    assert_eq!(
        fast.applied.batch,
        store.current_cut().expect("cut").last_batch
    );
    assert_ne!(mid, fast);
    assert!(
        store
            .payload_holders(notes)
            .expect("holders")
            .iter()
            .any(|h| h.starts_with("snapshot:"))
    );

    drop(store);
    let (reopened, _) = open(&path);
    assert_eq!(load(&reopened).expect("reload"), full);
}

#[test]
fn snapshot_at_the_head_needs_no_tail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    for events in scenario() {
        commit_events(&mut store, epoch, &events);
    }
    let model = load(&store).expect("load");
    save_snapshot(&mut store, epoch, &model).expect("snapshot");
    let replay = store
        .snapshot_and_tail(crate::MODEL_VERSION)
        .expect("replay");
    assert!(replay.snapshot.is_some());
    assert!(replay.tail.is_empty());
    assert_eq!(load(&store).expect("reload"), model);
}

#[test]
fn empty_model_cannot_be_snapshotted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    assert!(matches!(
        save_snapshot(&mut store, epoch, &JournalModel::default()),
        Err(ReplayError::NothingApplied)
    ));
}

#[test]
fn unknown_tag_in_the_journal_stops_the_replay() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    commit_events(&mut store, epoch, &scenario()[0]);
    let mut request = CommitRequest::new(epoch);
    request.appends.push(StreamAppend {
        stream_id: StreamId::new(STRUCTURE_STREAM),
        expected: ExpectedRevision::Any,
        events: vec![NewEvent {
            event_id: "future".to_owned(),
            payload: OpaquePayload {
                type_tag: "window.created".to_owned(),
                schema_version: 1,
                bytes: b"{}".to_vec(),
            },
            recorded_at_ms: 0,
            causation_id: None,
            actor: "test".to_owned(),
            origin: "test".to_owned(),
            payload_refs: Vec::new(),
        }],
    });
    store.commit(&request).expect("commit");
    for result in [load(&store), full_replay(&store)] {
        assert!(matches!(
            result,
            Err(ReplayError::Evolve(EvolveError::Codec(CodecError::UnknownTag(tag))))
                if tag == "window.created"
        ));
    }
}
