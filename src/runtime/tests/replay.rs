//! 실제 journal에서의 replay: 전체 로그와 snapshot+tail의 일치, 엔진 stream 분리, 다른 stream
//! 건너뛰기, 모르는 tag 중단.

use tasty_domain::{CodecError, DomainEvent, MODEL_VERSION, StructureModels};
use tasty_event_store::{
    CommitRequest, ExpectedRevision, NewEvent, OpaquePayload, PayloadRef, StreamAppend, StreamId,
};

use super::common::{commit_events, db_path, new_event, open, scenario, stream};
use crate::runtime::journal::{
    JournalError, engine_stream, full_replay, load, load_all, save_snapshot,
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
    for events in &batches[..2] {
        commit_events(&mut store, epoch, events);
    }
    let mid = load_all(&store).expect("load without snapshot");
    save_snapshot(&mut store, epoch, &mid).expect("snapshot");
    for events in &batches[2..] {
        commit_events(&mut store, epoch, events);
    }

    let full = full_replay(&store).expect("full replay");
    let fast = load_all(&store).expect("snapshot + tail");
    assert_eq!(full, fast);
    let revision = store.stream_revision(&stream()).expect("head");
    let engine = fast.stream(stream().as_str());
    assert_eq!(engine.applied.revision, revision);
    let last = store.current_cut().expect("cut").last_batch;
    assert_eq!(engine.applied.batch, last);
    assert_eq!(fast.batch, last);
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
    assert_eq!(load_all(&reopened).expect("reload"), full);
}

#[test]
fn snapshot_at_the_head_needs_no_tail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    for events in scenario() {
        commit_events(&mut store, epoch, &events);
    }
    let models = load_all(&store).expect("load");
    save_snapshot(&mut store, epoch, &models).expect("snapshot");
    let replay = store.snapshot_and_tail(MODEL_VERSION).expect("replay");
    assert!(replay.snapshot.is_some());
    assert!(replay.tail.is_empty());
    assert_eq!(load_all(&store).expect("reload"), models);
}

#[test]
fn empty_model_cannot_be_snapshotted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    assert!(matches!(
        save_snapshot(&mut store, epoch, &StructureModels::default()),
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
        stream_id: stream(),
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
    for result in [load_all(&store), full_replay(&store)] {
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
    let models = full_replay(&store).expect("replay");
    assert_eq!(models.streams.len(), 1);
    let model = models.stream(stream().as_str());
    assert_eq!(model.applied.batch, Some(2));
    assert_eq!(model.applied.revision, Some(3));
    assert!(model.tabs.contains_key(&1));
    assert_eq!(load(&store, &stream()).expect("load"), model);
}

/// 한 batch가 두 엔진 stream을 함께 바꿔도 각 엔진 모델은 자기 stream 이벤트만 담는다.
/// snapshot 하나가 두 엔진을 같은 cut으로 담는다.
#[test]
fn engine_streams_are_separate_models_in_one_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut store, epoch) = open(&db_path(&dir));
    let first = scenario().remove(0);
    let other = engine_stream(2);
    let mut request = CommitRequest::new(epoch);
    for (stream_id, prefix) in [(stream(), "a"), (other.clone(), "b")] {
        request.appends.push(StreamAppend {
            stream_id,
            expected: ExpectedRevision::NoStream,
            events: first
                .iter()
                .enumerate()
                .map(|(i, e)| new_event(format!("{prefix}-{i}"), e))
                .collect(),
        });
    }
    store.commit(&request).expect("commit");
    commit_events(&mut store, epoch, &scenario()[1]);

    let models = load_all(&store).expect("load");
    assert_eq!(models.streams.len(), 2);
    let one = models.stream(stream().as_str());
    let two = models.stream(other.as_str());
    assert_eq!(two.applied.revision, Some(first.len() as u64));
    assert_eq!(two.applied.batch, Some(2));
    assert!(one.panes.len() > two.panes.len());
    save_snapshot(&mut store, epoch, &models).expect("snapshot");
    assert_eq!(load_all(&store).expect("reload"), models);
    assert_eq!(full_replay(&store).expect("replay"), models);
    assert_eq!(load(&store, &other).expect("engine 2"), two);
}
