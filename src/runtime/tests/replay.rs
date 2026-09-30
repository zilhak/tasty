//! 실제 journal에서의 replay: 전체 로그와 snapshot+tail의 일치, 다른 stream 건너뛰기,
//! 모르는 tag 중단.

use tasty_domain::{CodecError, DomainEvent, JournalModel, MODEL_VERSION, STRUCTURE_STREAM};
use tasty_event_store::{
    CommitRequest, ExpectedRevision, NewEvent, OpaquePayload, PayloadRef, StreamAppend, StreamId,
};

use super::common::{commit_events, db_path, new_event, open, scenario};
use crate::runtime::journal::{JournalError, full_replay, load, save_snapshot};

#[test]
fn full_replay_equals_snapshot_plus_tail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    // 시나리오의 markdown surface가 참조하는 자료. snapshot이 이를 pin한다.
    let notes = store.put_payload(epoch, b"notes").expect("payload");
    assert_eq!(notes, PayloadRef(1));
    let batches = scenario();
    for events in &batches[..2] {
        commit_events(&mut store, epoch, events);
    }
    let mid = load(&store).expect("load without snapshot");
    save_snapshot(&mut store, epoch, &mid).expect("snapshot");
    for events in &batches[2..] {
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
    let replay = store.snapshot_and_tail(MODEL_VERSION).expect("replay");
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
        Err(JournalError::NothingApplied)
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
            Err(JournalError::Codec(CodecError::UnknownTag(tag))) if tag == "window.created"
        ));
    }
}

#[test]
fn other_streams_are_skipped_but_the_batch_is_recorded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    commit_events(&mut store, epoch, &scenario()[0]);
    // 구조 이벤트처럼 보이는 본문이라도 다른 stream이면 해석하지 않는다.
    let mut request = CommitRequest::new(epoch);
    request.appends.push(StreamAppend {
        stream_id: StreamId::new("terminal"),
        expected: ExpectedRevision::NoStream,
        events: vec![new_event(
            "terminal-1".to_owned(),
            &DomainEvent::TabClosed { id: 1 },
        )],
    });
    store.commit(&request).expect("commit");
    let model = full_replay(&store).expect("replay");
    assert_eq!(model.applied.batch, Some(2));
    assert_eq!(model.applied.revision, Some(3));
    assert!(model.tabs.contains_key(&1));
    assert_eq!(load(&store).expect("load"), model);
}
