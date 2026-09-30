//! 통합 시험 공용 도우미. 모든 시험은 임시 디렉터리의 실제 SQLite 파일을 쓴다.

use std::path::{Path, PathBuf};

use crate::{
    CommandKey, CommandStatus, EffectState, EventStore, ExpectedRevision, NewCommand, NewEffect,
    NewEvent, OpaquePayload, StreamAppend, StreamId, WriterEpoch,
};

pub const JOURNAL: &str = "journal-test";

pub fn db_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("journal.db")
}

pub fn open(path: &Path) -> (EventStore, WriterEpoch) {
    let mut store = EventStore::open(path, JOURNAL).expect("open journal");
    let epoch = store.acquire_writer().expect("acquire writer");
    (store, epoch)
}

pub fn fresh() -> (tempfile::TempDir, EventStore, WriterEpoch) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, epoch) = open(&db_path(&dir));
    (dir, store, epoch)
}

pub fn payload(tag: &str, bytes: &[u8]) -> OpaquePayload {
    OpaquePayload {
        type_tag: tag.to_owned(),
        schema_version: 1,
        bytes: bytes.to_vec(),
    }
}

pub fn event(id: &str) -> NewEvent {
    NewEvent {
        event_id: id.to_owned(),
        payload: payload("TestEvent", id.as_bytes()),
        recorded_at_ms: 0,
        causation_id: None,
        actor: "agent".to_owned(),
        origin: "test".to_owned(),
        payload_refs: Vec::new(),
    }
}

pub fn append(stream: &str, expected: ExpectedRevision, ids: &[&str]) -> StreamAppend {
    StreamAppend {
        stream_id: StreamId::new(stream),
        expected,
        events: ids.iter().map(|id| event(id)).collect(),
    }
}

pub fn key(scope: &str, k: &str) -> CommandKey {
    CommandKey {
        caller_scope: scope.to_owned(),
        idempotency_key: k.to_owned(),
    }
}

pub fn command(id: &str, key: Option<CommandKey>, digest: &[u8]) -> NewCommand {
    NewCommand {
        command_id: id.to_owned(),
        key,
        request_digest: digest.to_vec(),
        resolved: b"target=pane:1".to_vec(),
        status: CommandStatus::Completed,
        response: Some(b"ok".to_vec()),
    }
}

pub fn new_effect(id: &str, generation: u64, initial: EffectState) -> NewEffect {
    NewEffect {
        effect_id: id.to_owned(),
        operation_id: format!("op-{id}"),
        resource_generation: generation,
        payload: payload("SurfaceCreate", b"{}"),
        initial,
    }
}

/// 테스트가 파일을 직접 조작할 때 쓰는 별도 연결.
pub fn raw(path: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).expect("raw connection")
}
