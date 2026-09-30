//! pure evolve: 결정성, batch 단위 적용, 순서 검사, 구조 변경 결과.

use std::collections::BTreeMap;

use tasty_event_store::{StoredBatch, StreamId};

use super::common::{batch, scenario, scenario_batches, surface};
use crate::{
    Applied, CategoryId, DomainEvent, EvolveError, JournalModel, PaneId, SplitTree, SurfaceId,
    TabId, WorkspaceId, encode_event, evolve,
};

fn run(batches: &[StoredBatch]) -> JournalModel {
    let mut model = JournalModel::default();
    for b in batches {
        evolve(&mut model, b).expect("evolve");
    }
    model
}

#[test]
fn same_batches_give_the_same_model() {
    let batches = scenario_batches();
    let first = run(&batches);
    let second = run(&batches);
    assert_eq!(first, second);
    let events: usize = scenario().iter().map(Vec::len).sum();
    assert_eq!(
        first.applied,
        Applied {
            batch: Some(batches.len() as u64),
            revision: Some(events as u64),
        }
    );
}

#[test]
fn scenario_ends_in_the_expected_structure() {
    let model = run(&scenario_batches());
    assert_eq!(model.category_order, vec![CategoryId(0)]);
    assert_eq!(model.workspace_order, vec![WorkspaceId(1)]);
    let ws = &model.workspaces[&WorkspaceId(1)];
    assert_eq!(ws.layout, SplitTree::Leaf(PaneId(1)));
    assert!(ws.metadata.is_empty());
    assert_eq!(
        model.panes.keys().copied().collect::<Vec<_>>(),
        vec![PaneId(1)]
    );
    assert_eq!(model.panes[&PaneId(1)].tabs, vec![TabId(1)]);
    assert_eq!(model.tabs[&TabId(1)].name, "build");
    assert_eq!(model.tabs[&TabId(1)].layout, SplitTree::Leaf(SurfaceId(1)));
    assert_eq!(
        model.surfaces.keys().copied().collect::<Vec<_>>(),
        vec![SurfaceId(1)]
    );
    assert_eq!(
        model.surfaces[&SurfaceId(1)].metadata,
        BTreeMap::from([("role".to_owned(), "orchestrator".to_owned())])
    );
}

#[test]
fn intermediate_state_keeps_moves_and_split_ratios() {
    let batches = scenario_batches();
    let model = run(&batches[..5]);
    assert_eq!(model.category_order, vec![CategoryId(1), CategoryId(0)]);
    assert_eq!(model.workspace_order, vec![WorkspaceId(2), WorkspaceId(1)]);
    assert_eq!(model.workspaces[&WorkspaceId(2)].category, CategoryId(0));
    assert_eq!(model.panes[&PaneId(2)].workspace, WorkspaceId(2));
    assert_eq!(
        model.workspaces[&WorkspaceId(2)].layout.leaves(),
        vec![PaneId(2), PaneId(3)]
    );
    let SplitTree::Split { ratio, .. } = model.workspaces[&WorkspaceId(2)].layout else {
        panic!("expected split");
    };
    assert_eq!(ratio.to_f32(), 0.75);
    assert_eq!(model.panes[&PaneId(1)].tabs, vec![TabId(1), TabId(3)]);
    assert_eq!(model.surfaces[&SurfaceId(3)].tab, TabId(2));
    assert_eq!(
        model.tabs[&TabId(2)].layout.leaves(),
        vec![SurfaceId(2), SurfaceId(3)]
    );
}

#[test]
fn failing_batch_leaves_the_model_unchanged() {
    let batches = scenario_batches();
    let mut model = run(&batches[..1]);
    let before = model.clone();
    let bad = batch(
        2,
        4,
        &[
            DomainEvent::TabRenamed {
                id: TabId(1),
                name: "renamed".to_owned(),
            },
            DomainEvent::TabClosed { id: TabId(99) },
        ],
    );
    assert!(matches!(
        evolve(&mut model, &bad),
        Err(EvolveError::Missing(name)) if name == "tab:99"
    ));
    assert_eq!(model, before);
}

#[test]
fn stale_batch_and_revision_gap_are_rejected() {
    let batches = scenario_batches();
    let mut model = run(&batches[..2]);
    let before = model.clone();
    assert!(matches!(
        evolve(&mut model, &batches[1]),
        Err(EvolveError::StaleBatch { last: 2, got: 2 })
    ));
    let gap = batch(3, 99, &scenario()[2]);
    assert!(matches!(
        evolve(&mut model, &gap),
        Err(EvolveError::RevisionGap {
            expected: 8,
            got: 100
        })
    ));
    assert_eq!(model, before);
}

#[test]
fn other_streams_are_skipped_but_the_batch_is_recorded() {
    let mut model = run(&scenario_batches()[..1]);
    let mut other = batch(2, 0, &[DomainEvent::TabClosed { id: TabId(99) }]);
    for event in &mut other.events {
        event.stream_id = StreamId::new("terminal");
        event.payload = encode_event(&DomainEvent::TabClosed { id: TabId(99) }).expect("encode");
    }
    let revision = model.applied.revision;
    evolve(&mut model, &other).expect("evolve");
    assert_eq!(model.applied.batch, Some(2));
    assert_eq!(model.applied.revision, revision);
    assert!(model.tabs.contains_key(&TabId(1)));
}

#[test]
fn structural_invariants_are_enforced() {
    let mut model = run(&scenario_batches()[..1]);
    let cases = [
        (DomainEvent::PaneClosed { id: PaneId(1) }, "last"),
        (DomainEvent::SurfaceClosed { id: SurfaceId(1) }, "last"),
        (
            DomainEvent::CategoryClosed { id: CategoryId(0) },
            "non-empty",
        ),
        (
            DomainEvent::TabCreated {
                id: TabId(1),
                pane: PaneId(1),
                index: 0,
                name: "dup".to_owned(),
                surface: surface(9, "terminal"),
            },
            "duplicate",
        ),
        (
            DomainEvent::TabMoved {
                id: TabId(1),
                pane: PaneId(1),
                index: 5,
            },
            "index",
        ),
    ];
    for (i, (event, what)) in cases.into_iter().enumerate() {
        let before = model.clone();
        let result = evolve(&mut model, &batch(10 + i as u64, 4, &[event]));
        let ok = matches!(
            (what, &result),
            ("last", Err(EvolveError::LastLeaf(_)))
                | ("non-empty", Err(EvolveError::CategoryNotEmpty(_)))
                | ("duplicate", Err(EvolveError::Duplicate(_)))
                | (
                    "index",
                    Err(EvolveError::IndexOutOfRange { index: 5, len: 0 })
                )
        );
        assert!(ok, "{what}: {result:?}");
        assert_eq!(model, before);
    }
}
