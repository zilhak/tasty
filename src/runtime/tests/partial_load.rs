//! 손상을 그 범위로 한정하는 적재: 한 stream만의 batch, 여러 stream을 함께 바꾸는 batch, 해석하지
//! 못하는 snapshot. 모든 경우에 저장소의 원본 행은 바뀌지 않는다.

use sha2::Digest;
use tasty_core::DomainEvent;
use tasty_event_store::{
    CommitOutcome, CommitRequest, EventStore, ExpectedRevision, NewEvent, StoreError, StreamAppend,
    StreamId, WriterEpoch,
};

use super::common::{db_path, new_event, open, scenario};
use crate::runtime::journal::{
    JournalError, engine_stream, full_replay, load_all, load_recoverable, save_snapshot,
};

fn one() -> StreamId {
    engine_stream(1)
}
fn two() -> StreamId {
    engine_stream(2)
}

/// 이름표는 맞지만 본문을 해석할 수 없는 이벤트.
fn undecodable(id: &str) -> NewEvent {
    let mut event = new_event(id.to_owned(), &DomainEvent::TabClosed { id: 1 });
    event.payload.bytes = b"not an event".to_vec();
    event
}

fn events(prefix: &str, list: &[DomainEvent]) -> Vec<NewEvent> {
    list.iter()
        .enumerate()
        .map(|(i, e)| new_event(format!("{prefix}-{i}"), e))
        .collect()
}

fn append(
    store: &mut EventStore,
    epoch: WriterEpoch,
    parts: Vec<(StreamId, Vec<NewEvent>)>,
) -> u64 {
    let mut request = CommitRequest::new(epoch);
    for (stream_id, events) in parts {
        request.appends.push(StreamAppend {
            stream_id,
            expected: ExpectedRevision::Any,
            events,
        });
    }
    match store.commit(&request).expect("commit") {
        CommitOutcome::Committed { batch: Some(cut) } => cut.batch_id,
        other => panic!("unexpected outcome {other:?}"),
    }
}

/// 저장소의 모든 원본 행. 적재 전후 비교에 쓴다.
type EventRow = (String, i64, Vec<u8>);
type PayloadRow = (i64, Vec<u8>, Vec<u8>);

fn rows(path: &std::path::Path) -> (Vec<EventRow>, Vec<PayloadRow>) {
    let connection = rusqlite::Connection::open(path).expect("sqlite");
    let mut events = connection
        .prepare(
            "SELECT stream_id, stream_revision, payload FROM events ORDER BY batch_id, batch_index",
        )
        .expect("events");
    let events = events
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("events")
        .collect::<Result<_, _>>()
        .expect("events");
    let mut payloads = connection
        .prepare("SELECT payload_id, sha256, bytes FROM payloads ORDER BY payload_id")
        .expect("payloads");
    let payloads = payloads
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("payloads")
        .collect::<Result<_, _>>()
        .expect("payloads");
    (events, payloads)
}

/// snapshot 본문을 checksum은 맞지만 해석할 수 없는 바이트로 바꾼다.
fn spoil_snapshot(path: &std::path::Path, snapshot: u64) {
    let bytes = b"not a snapshot".to_vec();
    let digest = sha2::Sha256::digest(&bytes).to_vec();
    let connection = rusqlite::Connection::open(path).expect("sqlite");
    let changed = connection
        .execute(
            "UPDATE payloads SET bytes = ?1, sha256 = ?2
             WHERE payload_id = (SELECT payload_id FROM snapshots WHERE snapshot_id = ?3)",
            rusqlite::params![bytes, digest, snapshot as i64],
        )
        .expect("spoil snapshot");
    assert_eq!(changed, 1);
}

/// (가) slot-2만 담은 batch가 손상되면 slot-1은 그대로 적재되고 slot-2만 빠진다.
#[test]
fn a_damaged_single_stream_batch_leaves_the_other_stream_loaded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let scenario = scenario();
    append(&mut store, epoch, vec![(one(), events("a0", &scenario[0]))]);
    append(&mut store, epoch, vec![(two(), events("b0", &scenario[0]))]);
    let healthy = load_all(&store)
        .expect("before damage")
        .stream(one().as_str());
    append(&mut store, epoch, vec![(two(), vec![undecodable("b-bad")])]);
    let last = append(&mut store, epoch, vec![(two(), events("b1", &scenario[1]))]);
    let before = rows(&path);

    assert!(load_all(&store).is_err(), "the strict loader still refuses");
    let (models, report) = load_recoverable(&store).expect("recoverable load");
    assert_eq!(
        models.batch,
        Some(last),
        "skipped batches still advance the cut"
    );
    assert_eq!(models.stream(one().as_str()).panes, healthy.panes);
    assert!(!models.streams.contains_key(two().as_str()));
    assert_eq!(
        report.damaged.keys().collect::<Vec<_>>(),
        vec![two().as_str()]
    );
    assert!(
        report.damaged[two().as_str()].contains("batch 3"),
        "{report:?}"
    );
    assert!(report.skipped_snapshots.is_empty());
    assert_eq!(rows(&path), before, "the stored rows are unchanged");
}

/// (나) 두 stream을 함께 바꾸는 batch가 손상되면 그 batch를 일부만 적용하지 않고 두 stream을 모두
/// 손상으로 둔다. 손상된 stream을 다시 바꾸는 뒤 batch도 적용하지 않는다. 손상과 무관한 stream은
/// 계속 적재된다.
#[test]
fn a_damaged_shared_batch_marks_every_stream_it_touches() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let scenario = scenario();
    let three = engine_stream(3);
    append(&mut store, epoch, vec![(one(), events("a0", &scenario[0]))]);
    append(&mut store, epoch, vec![(two(), events("b0", &scenario[0]))]);
    append(
        &mut store,
        epoch,
        vec![(three.clone(), events("c0", &scenario[0]))],
    );
    let untouched = load_all(&store)
        .expect("before damage")
        .stream(three.as_str());
    append(
        &mut store,
        epoch,
        vec![
            (one(), events("a1", &scenario[1])),
            (two(), vec![undecodable("b-bad")]),
        ],
    );
    append(&mut store, epoch, vec![(one(), events("a2", &scenario[2]))]);
    let before = rows(&path);

    let (models, report) = load_recoverable(&store).expect("recoverable load");
    assert!(!models.streams.contains_key(one().as_str()));
    assert!(!models.streams.contains_key(two().as_str()));
    assert_eq!(models.stream(three.as_str()).panes, untouched.panes);
    assert_eq!(report.damaged.len(), 2, "{report:?}");
    assert!(
        report.damaged[one().as_str()].contains("batch 4"),
        "{report:?}"
    );
    assert_eq!(models.batch, Some(5));
    assert_eq!(rows(&path), before);
}

/// (다-1) 최신 snapshot을 해석할 수 없으면 이전 snapshot이나 로그 처음부터 다시 만든다.
#[test]
fn an_undecodable_snapshot_falls_back_to_an_older_base() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let scenario = scenario();
    append(&mut store, epoch, vec![(one(), events("a0", &scenario[0]))]);
    let models = load_all(&store).unwrap();
    let first = save_snapshot(&mut store, epoch, &models).expect("snapshot 1");
    append(&mut store, epoch, vec![(two(), events("b0", &scenario[0]))]);
    let models = load_all(&store).unwrap();
    let second = save_snapshot(&mut store, epoch, &models).expect("snapshot 2");
    append(&mut store, epoch, vec![(one(), events("a1", &scenario[1]))]);
    let expected = full_replay(&store).expect("full replay");

    spoil_snapshot(&path, second);
    let before = rows(&path);
    let (models, report) = load_recoverable(&store).expect("older snapshot");
    assert_eq!(models, expected);
    assert_eq!(report.skipped_snapshots.len(), 1, "{report:?}");
    assert_eq!(report.skipped_snapshots[0].0, second);
    assert!(report.damaged.is_empty());

    spoil_snapshot(&path, first);
    let (models, report) = load_recoverable(&store).expect("full log");
    assert_eq!(models, expected);
    assert_eq!(report.skipped_snapshots.len(), 2, "{report:?}");
    assert!(matches!(load_all(&store), Err(JournalError::Codec(_))));
    let after = rows(&path);
    assert_eq!(after.0, before.0, "events are unchanged");
}

/// (다-2) 남은 기반이 모두 해석할 수 없고 로그 앞부분도 정리됐으면 저장소 전체를 읽을 수 없다.
#[test]
fn no_readable_base_above_the_retention_floor_fails_the_whole_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let scenario = scenario();
    append(&mut store, epoch, vec![(one(), events("a0", &scenario[0]))]);
    let models = load_all(&store).unwrap();
    let first = save_snapshot(&mut store, epoch, &models).expect("snapshot 1");
    append(&mut store, epoch, vec![(one(), events("a1", &scenario[1]))]);
    let models = load_all(&store).unwrap();
    let second = save_snapshot(&mut store, epoch, &models).expect("snapshot 2");
    store
        .compact_history(epoch, tasty_core::MODEL_VERSION)
        .expect("compact")
        .expect("history before the older snapshot is removed");
    spoil_snapshot(&path, first);
    spoil_snapshot(&path, second);
    let before = rows(&path);

    assert!(matches!(
        load_recoverable(&store),
        Err(JournalError::Store(StoreError::ResyncRequired { .. }))
    ));
    assert_eq!(rows(&path), before);
}
