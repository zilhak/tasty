//! 엔진별 구조 stream: 한 batch가 여러 stream을 함께 바꾸고, 실패하면 어느 stream도 바뀌지 않는다.

use super::common::{batch, surface};
use crate::{
    DomainEvent, EvolveError, RecordedEvent, StreamBatch, StructureModels, evolve_streams,
    is_structure_stream,
};

const A: &str = "structure:slot-1";
const B: &str = "structure:slot-2";

fn workspace(id: u32, pane: u32) -> Vec<DomainEvent> {
    vec![
        DomainEvent::CategoryCreated {
            id: 0,
            name: "normal".to_owned(),
            index: 0,
        },
        DomainEvent::WorkspaceCreated {
            id,
            name: format!("ws{id}"),
            category: 0,
            index: 0,
            pane,
        },
        DomainEvent::TabCreated {
            id,
            pane,
            index: 0,
            name: "shell".to_owned(),
            surface: surface(id, "terminal"),
        },
    ]
}

fn events(after: u64, list: &[DomainEvent]) -> Vec<RecordedEvent> {
    batch(0, after, list).events
}

#[test]
fn one_batch_changes_several_engine_streams() {
    let mut models = StructureModels::default();
    let first = StreamBatch {
        batch_id: 1,
        streams: [
            (A.to_owned(), events(0, &workspace(1, 1))),
            (B.to_owned(), events(0, &workspace(2, 2))),
        ]
        .into(),
    };
    evolve_streams(&mut models, &first).expect("evolve");
    assert_eq!(models.batch, Some(1));
    assert_eq!(models.stream(A).workspace_order, vec![1]);
    assert_eq!(models.stream(B).workspace_order, vec![2]);

    // B의 이벤트가 없는 batch도 B의 적용 위치를 옮긴다.
    let second = StreamBatch {
        batch_id: 2,
        streams: [(
            A.to_owned(),
            events(
                3,
                &[DomainEvent::TabRenamed {
                    id: 1,
                    name: "build".to_owned(),
                }],
            ),
        )]
        .into(),
    };
    evolve_streams(&mut models, &second).expect("evolve");
    assert_eq!(models.stream(A).applied.revision, Some(4));
    assert_eq!(models.stream(B).applied.revision, Some(3));
    assert_eq!(models.stream(B).applied.batch, Some(2));
    assert_eq!(models.stream("structure:slot-9"), Default::default());
}

#[test]
fn a_failing_stream_leaves_every_stream_unchanged() {
    let mut models = StructureModels::default();
    evolve_streams(
        &mut models,
        &StreamBatch {
            batch_id: 1,
            streams: [(A.to_owned(), events(0, &workspace(1, 1)))].into(),
        },
    )
    .expect("evolve");
    let before = models.clone();
    let bad = StreamBatch {
        batch_id: 2,
        streams: [
            (A.to_owned(), events(3, &[DomainEvent::TabClosed { id: 1 }])),
            (B.to_owned(), events(0, &[DomainEvent::TabClosed { id: 5 }])),
        ]
        .into(),
    };
    assert!(matches!(
        evolve_streams(&mut models, &bad),
        Err(EvolveError::Missing(_))
    ));
    assert_eq!(models, before);
    assert!(matches!(
        evolve_streams(
            &mut models,
            &StreamBatch {
                batch_id: 1,
                streams: Default::default(),
            }
        ),
        Err(EvolveError::StaleBatch { last: 1, got: 1 })
    ));
}

#[test]
fn only_prefixed_streams_are_structure_streams() {
    assert!(is_structure_stream(A));
    assert!(!is_structure_stream("structure"));
    assert!(!is_structure_stream("tasks"));
}

#[test]
fn unchanged_streams_advance_the_cut_without_replacing_their_content() {
    let mut models = StructureModels::default();
    evolve_streams(
        &mut models,
        &StreamBatch {
            batch_id: 1,
            streams: [
                (A.into(), events(0, &workspace(1, 1))),
                (B.into(), events(0, &workspace(2, 2))),
            ]
            .into(),
        },
    )
    .unwrap();
    let name = models.streams[B].workspaces[&2].name.as_ptr();
    evolve_streams(
        &mut models,
        &StreamBatch {
            batch_id: 2,
            streams: [(
                A.into(),
                events(
                    3,
                    &[DomainEvent::TabRenamed {
                        id: 1,
                        name: "next".into(),
                    }],
                ),
            )]
            .into(),
        },
    )
    .unwrap();
    assert_eq!(models.streams[B].workspaces[&2].name.as_ptr(), name);
    assert_eq!(models.streams[B].applied.batch, Some(2));
    assert_eq!(models.streams[B].applied.revision, Some(3));
    evolve_streams(
        &mut models,
        &StreamBatch {
            batch_id: 3,
            streams: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(models.streams[B].workspaces[&2].name.as_ptr(), name);
    assert_eq!(models.streams[A].applied.batch, Some(3));
}

#[test]
fn failing_batch_does_not_advance_an_untouched_stream() {
    let mut models = StructureModels::default();
    evolve_streams(
        &mut models,
        &StreamBatch {
            batch_id: 1,
            streams: [(A.into(), events(0, &workspace(1, 1)))].into(),
        },
    )
    .unwrap();
    let before = models.clone();
    assert!(
        evolve_streams(
            &mut models,
            &StreamBatch {
                batch_id: 2,
                streams: [(B.into(), events(0, &[DomainEvent::TabClosed { id: 999 }]))].into()
            }
        )
        .is_err()
    );
    assert_eq!(models, before);
}
