//! 시험 공용 도우미와 여러 이벤트를 거치는 시나리오.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tasty_event_store::{
    BatchCut, CommitOutcome, CommitRequest, EventStore, ExpectedRevision, NewEvent, StoredBatch,
    StoredEvent, StreamAppend, StreamId, WriterEpoch,
};

use crate::{
    CategoryId, DataRef, DomainEvent, MetadataTarget, PaneId, Placement, Ratio, STRUCTURE_STREAM,
    SplitDirection, SplitSpec, SurfaceId, SurfaceSpec, TabId, WorkspaceId, encode_event,
};

pub const JOURNAL: &str = "journal-domain-test";

pub fn db_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("journal.db")
}

pub fn open(path: &Path) -> (EventStore, WriterEpoch) {
    let mut store = EventStore::open(path, JOURNAL).expect("open journal");
    let epoch = store.acquire_writer().expect("acquire writer");
    (store, epoch)
}

pub fn split(direction: SplitDirection, ratio: f32, placement: Placement) -> SplitSpec {
    SplitSpec {
        direction,
        ratio: Ratio::from_f32(ratio),
        placement,
    }
}

pub fn surface(id: u64, kind: &str) -> SurfaceSpec {
    SurfaceSpec {
        id: SurfaceId(id),
        kind: kind.to_owned(),
        data: None,
    }
}

/// 구조 이벤트만 담은 batch를 저장소 없이 만든다. revision은 `after` 다음부터다.
pub fn batch(batch_id: u64, after: u64, events: &[DomainEvent]) -> StoredBatch {
    let stream = StreamId::new(STRUCTURE_STREAM);
    let stored: Vec<StoredEvent> = events
        .iter()
        .enumerate()
        .map(|(i, e)| StoredEvent {
            stream_id: stream.clone(),
            stream_revision: after + i as u64 + 1,
            event_id: format!("b{batch_id}-{i}"),
            batch_id,
            batch_index: i as u32,
            payload: encode_event(e).expect("encode"),
            recorded_at_ms: 0,
            command_id: None,
            causation_id: None,
            actor: "test".to_owned(),
            origin: "test".to_owned(),
        })
        .collect();
    let mut revisions = BTreeMap::new();
    if !events.is_empty() {
        revisions.insert(stream, after + events.len() as u64);
    }
    StoredBatch {
        cut: BatchCut {
            batch_id,
            revisions,
        },
        command_id: None,
        events: stored,
    }
}

/// 시나리오를 batch 목록으로 만든다. batch 번호는 1부터다.
pub fn scenario_batches() -> Vec<StoredBatch> {
    let mut after = 0;
    scenario()
        .iter()
        .enumerate()
        .map(|(i, events)| {
            let b = batch(i as u64 + 1, after, events);
            after += events.len() as u64;
            b
        })
        .collect()
}

/// 이벤트를 구조 stream에 한 batch로 확정한다.
pub fn commit_events(
    store: &mut EventStore,
    epoch: WriterEpoch,
    events: &[DomainEvent],
) -> BatchCut {
    let head = store
        .stream_revision(&StreamId::new(STRUCTURE_STREAM))
        .expect("head");
    let start = head.unwrap_or(0);
    let mut request = CommitRequest::new(epoch);
    request.appends.push(StreamAppend {
        stream_id: StreamId::new(STRUCTURE_STREAM),
        expected: head.map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
        events: events
            .iter()
            .enumerate()
            .map(|(i, e)| NewEvent {
                event_id: format!("ev-{}", start + i as u64 + 1),
                payload: encode_event(e).expect("encode"),
                recorded_at_ms: 0,
                causation_id: None,
                actor: "test".to_owned(),
                origin: "test".to_owned(),
                payload_refs: Vec::new(),
            })
            .collect(),
    });
    match store.commit(&request).expect("commit") {
        CommitOutcome::Committed { batch: Some(cut) } => cut,
        other => panic!("unexpected outcome {other:?}"),
    }
}

/// 모든 이벤트 종류를 한 번 이상 거치는 batch 목록.
pub fn scenario() -> Vec<Vec<DomainEvent>> {
    let ws = WorkspaceId(1);
    let ws2 = WorkspaceId(2);
    vec![
        vec![
            DomainEvent::CategoryCreated {
                id: CategoryId(0),
                name: "normal".to_owned(),
                index: 0,
            },
            DomainEvent::CategoryCreated {
                id: CategoryId(1),
                name: "work".to_owned(),
                index: 1,
            },
            DomainEvent::WorkspaceCreated {
                id: ws,
                name: "main".to_owned(),
                category: CategoryId(0),
                index: 0,
                pane: PaneId(1),
            },
            DomainEvent::TabCreated {
                id: TabId(1),
                pane: PaneId(1),
                index: 0,
                name: "shell".to_owned(),
                surface: surface(1, "terminal"),
            },
        ],
        vec![
            DomainEvent::PaneSplit {
                target: PaneId(1),
                pane: PaneId(2),
                split: split(SplitDirection::Vertical, 0.3, Placement::After),
            },
            DomainEvent::TabCreated {
                id: TabId(2),
                pane: PaneId(2),
                index: 0,
                name: "notes".to_owned(),
                surface: SurfaceSpec {
                    id: SurfaceId(2),
                    kind: "markdown".to_owned(),
                    data: Some(DataRef(1)),
                },
            },
            DomainEvent::SurfaceSplit {
                target: SurfaceId(1),
                surface: surface(3, "terminal"),
                split: split(SplitDirection::Horizontal, 0.1 + 0.2, Placement::Before),
            },
        ],
        vec![
            DomainEvent::WorkspaceCreated {
                id: ws2,
                name: "side".to_owned(),
                category: CategoryId(1),
                index: 1,
                pane: PaneId(3),
            },
            DomainEvent::TabCreated {
                id: TabId(3),
                pane: PaneId(3),
                index: 0,
                name: "logs".to_owned(),
                surface: surface(4, "terminal"),
            },
            DomainEvent::MetadataSet {
                target: MetadataTarget::Surface(SurfaceId(1)),
                key: "role".to_owned(),
                value: "orchestrator".to_owned(),
            },
            DomainEvent::MetadataSet {
                target: MetadataTarget::Workspace(ws),
                key: "project".to_owned(),
                value: "tasty".to_owned(),
            },
        ],
        vec![
            DomainEvent::CategoryRenamed {
                id: CategoryId(1),
                name: "jobs".to_owned(),
            },
            DomainEvent::WorkspaceRenamed {
                id: ws2,
                name: "logs".to_owned(),
            },
            DomainEvent::TabRenamed {
                id: TabId(1),
                name: "build".to_owned(),
            },
            DomainEvent::WorkspaceMoved {
                id: ws2,
                category: CategoryId(0),
                index: 0,
            },
            DomainEvent::CategoryMoved {
                id: CategoryId(1),
                index: 0,
            },
        ],
        vec![
            DomainEvent::SurfaceMoved {
                id: SurfaceId(3),
                target: SurfaceId(2),
                split: split(SplitDirection::Vertical, 0.5, Placement::After),
            },
            DomainEvent::TabMoved {
                id: TabId(3),
                pane: PaneId(1),
                index: 1,
            },
            DomainEvent::PaneMoved {
                id: PaneId(2),
                target: PaneId(3),
                split: split(SplitDirection::Horizontal, 0.75, Placement::Before),
            },
            DomainEvent::MetadataRemoved {
                target: MetadataTarget::Workspace(ws),
                key: "project".to_owned(),
            },
        ],
        vec![
            DomainEvent::SurfaceClosed { id: SurfaceId(3) },
            DomainEvent::TabClosed { id: TabId(3) },
            DomainEvent::PaneClosed { id: PaneId(3) },
            DomainEvent::CategoryClosed { id: CategoryId(1) },
        ],
        vec![DomainEvent::WorkspaceClosed { id: ws2 }],
    ]
}
