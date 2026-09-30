//! 시험 공용 도우미와 여러 batch에 걸친 구조 시나리오.

use std::path::{Path, PathBuf};

use tasty_domain::{
    DataRef, DomainEvent, MetadataTarget, Placement, Ratio, STRUCTURE_STREAM, SplitSpec,
    SurfaceSpec,
};
use tasty_event_store::{
    BatchCut, CommitOutcome, CommitRequest, EventStore, ExpectedRevision, NewEvent, StreamAppend,
    StreamId, WriterEpoch,
};
use tasty_model::SplitDirection;

use crate::runtime::journal::to_payload;

pub(super) const JOURNAL: &str = "journal-runtime-test";

pub(super) fn db_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("journal.db")
}

pub(super) fn open(path: &Path) -> (EventStore, WriterEpoch) {
    let mut store = EventStore::open(path, JOURNAL).expect("open journal");
    let epoch = store.acquire_writer().expect("acquire writer");
    (store, epoch)
}

pub(super) fn new_event(id: String, event: &DomainEvent) -> NewEvent {
    NewEvent {
        event_id: id,
        payload: to_payload(event).expect("encode"),
        recorded_at_ms: 0,
        causation_id: None,
        actor: "test".to_owned(),
        origin: "test".to_owned(),
        payload_refs: Vec::new(),
    }
}

/// 이벤트를 구조 stream에 한 batch로 확정한다.
pub(super) fn commit_events(
    store: &mut EventStore,
    epoch: WriterEpoch,
    events: &[DomainEvent],
) -> BatchCut {
    let stream = StreamId::new(STRUCTURE_STREAM);
    let head = store.stream_revision(&stream).expect("head");
    let start = head.unwrap_or(0);
    let mut request = CommitRequest::new(epoch);
    request.appends.push(StreamAppend {
        stream_id: stream,
        expected: head.map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
        events: events
            .iter()
            .enumerate()
            .map(|(i, e)| new_event(format!("ev-{}", start + i as u64 + 1), e))
            .collect(),
    });
    match store.commit(&request).expect("commit") {
        CommitOutcome::Committed { batch: Some(cut) } => cut,
        other => panic!("unexpected outcome {other:?}"),
    }
}

fn split(direction: SplitDirection, ratio: f32, placement: Placement) -> SplitSpec {
    SplitSpec {
        direction,
        ratio: Ratio::from_f32(ratio),
        placement,
    }
}

fn terminal(id: u32) -> SurfaceSpec {
    SurfaceSpec {
        id,
        kind: "terminal".to_owned(),
        data: None,
    }
}

/// 생성·분할·이동·이름 변경·닫기·metadata를 거치는 batch 목록. surface 2는 payload 1을 참조한다.
pub(super) fn scenario() -> Vec<Vec<DomainEvent>> {
    vec![
        vec![
            DomainEvent::CategoryCreated {
                id: 0,
                name: "normal".to_owned(),
                index: 0,
            },
            DomainEvent::WorkspaceCreated {
                id: 1,
                name: "main".to_owned(),
                category: 0,
                index: 0,
                pane: 1,
            },
            DomainEvent::TabCreated {
                id: 1,
                pane: 1,
                index: 0,
                name: "shell".to_owned(),
                surface: terminal(1),
            },
        ],
        vec![
            DomainEvent::PaneSplit {
                target: 1,
                pane: 2,
                split: split(SplitDirection::Vertical, 0.1 + 0.2, Placement::After),
            },
            DomainEvent::TabCreated {
                id: 2,
                pane: 2,
                index: 0,
                name: "notes".to_owned(),
                surface: SurfaceSpec {
                    id: 2,
                    kind: "markdown".to_owned(),
                    data: Some(DataRef(1)),
                },
            },
        ],
        vec![
            DomainEvent::SurfaceSplit {
                target: 1,
                surface: terminal(3),
                split: split(SplitDirection::Horizontal, 0.75, Placement::Before),
            },
            DomainEvent::MetadataSet {
                target: MetadataTarget::Surface(1),
                key: "role".to_owned(),
                value: "orchestrator".to_owned(),
            },
        ],
        vec![
            DomainEvent::WorkspaceRenamed {
                id: 1,
                name: "build".to_owned(),
            },
            DomainEvent::SurfaceMoved {
                id: 3,
                target: 2,
                split: split(SplitDirection::Vertical, 0.5, Placement::After),
            },
        ],
        vec![
            DomainEvent::SurfaceClosed { id: 3 },
            DomainEvent::TabMoved {
                id: 2,
                pane: 1,
                index: 1,
            },
            DomainEvent::PaneClosed { id: 2 },
        ],
    ]
}
