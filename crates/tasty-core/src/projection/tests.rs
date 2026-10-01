use super::*;
use tasty_model::{EmptySurface, SplitDirection, SurfaceLayout};
use crate::{DomainEvent as E, Placement, Ratio, RecordedEvent, SplitSpec, SurfaceSpec};

fn batch(model: &JournalModel, events: Vec<E>) -> DomainBatch {
    DomainBatch {
        batch_id: model.applied.batch.unwrap_or(0) + 1,
        events: events
            .into_iter()
            .enumerate()
            .map(|(index, event)| RecordedEvent {
                revision: model.applied.revision.unwrap_or(0) + index as u64 + 1,
                event,
            })
            .collect(),
    }
}

fn prepared(id: u32) -> (u32, PreparedLeaf) {
    (
        id,
        PreparedLeaf {
            logical_kind: "empty".into(),
            surface: Box::new(EmptySurface::new(id)),
        },
    )
}

fn seeded() -> (crate::runtime::engine_session::EngineSession, JournalModel) {
    let (_view, mut session) = crate::state::tests::test_state();
    session.core_state.replace_local_workspaces(Vec::new());
    session.core_state.categories.clear();
    let mut model = JournalModel::default();
    let events = vec![
        E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        },
        E::WorkspaceCreated {
            id: 1,
            name: "one".into(),
            category: 0,
            index: 0,
            pane: 1,
        },
        E::TabCreated {
            id: 1,
            pane: 1,
            index: 0,
            name: "leaf".into(),
            surface: SurfaceSpec {
                id: 1,
                kind: "empty".into(),
                data: None,
            },
        },
        E::SurfaceSplit {
            target: 1,
            surface: SurfaceSpec {
                id: 2,
                kind: "empty".into(),
                data: None,
            },
            split: SplitSpec {
                direction: SplitDirection::Horizontal,
                ratio: Ratio::from_f32(0.4),
                placement: Placement::Before,
            },
        },
    ];
    let input = batch(&model, events);
    apply(
        &mut session.core_state,
        &model,
        &input,
        &mut HashMap::from([prepared(1), prepared(2)]),
        &mut Vec::new(),
    )
    .unwrap();
    crate::evolve(&mut model, &input).unwrap();
    (session, model)
}

#[test]
fn metadata_and_nested_split_keep_the_surviving_instance_and_split_identity() {
    let (mut session, mut model) = seeded();
    let core = &mut session.core_state;
    let pointer = core.find_surface_by_id(1).unwrap() as *const dyn Surface as *const ();
    let node = match core.find_pane_by_id(1).unwrap().tabs[0].layout() {
        SurfaceLayout::Split { node_id, .. } => *node_id,
        _ => panic!("split"),
    };
    let input = batch(
        &model,
        vec![
            E::WorkspaceRenamed {
                id: 1,
                name: "renamed".into(),
            },
            E::TabExplicitNameSet {
                id: 1,
                name: Some("custom".into()),
            },
            E::SurfaceSplit {
                target: 1,
                surface: SurfaceSpec {
                    id: 3,
                    kind: "empty".into(),
                    data: None,
                },
                split: SplitSpec {
                    direction: SplitDirection::Vertical,
                    ratio: Ratio::from_f32(0.7),
                    placement: Placement::After,
                },
            },
        ],
    );
    let mut retired = Vec::new();
    apply(
        core,
        &model,
        &input,
        &mut HashMap::from([prepared(3)]),
        &mut retired,
    )
    .unwrap();
    crate::evolve(&mut model, &input).unwrap();
    assert!(retired.is_empty());
    assert_eq!(
        core.find_surface_by_id(1).unwrap() as *const dyn Surface as *const (),
        pointer
    );
    assert!(
        matches!(core.find_pane_by_id(1).unwrap().tabs[0].layout(), SurfaceLayout::Split { node_id, .. } if *node_id == node)
    );
    assert_eq!(core.local_workspaces[0].name, "renamed");
    assert_eq!(
        core.find_pane_by_id(1).unwrap().tabs[0]
            .explicit_name
            .as_deref(),
        Some("custom")
    );
}

#[test]
fn missing_preparation_rejects_before_an_earlier_rename_is_published() {
    let (mut session, model) = seeded();
    let before = live::core_canonical(&session.core_state);
    let input = batch(
        &model,
        vec![
            E::WorkspaceRenamed {
                id: 1,
                name: "must not appear".into(),
            },
            E::SurfaceConverted {
                id: 1,
                kind: "empty".into(),
                data: None,
            },
        ],
    );
    assert!(
        apply(
            &mut session.core_state,
            &model,
            &input,
            &mut HashMap::new(),
            &mut Vec::new()
        )
        .unwrap_err()
        .contains("no prepared")
    );
    assert_eq!(live::core_canonical(&session.core_state), before);
}

#[test]
fn converted_and_closed_objects_remain_owned_until_cleanup_receives_them() {
    let (mut session, model) = seeded();
    let core = &mut session.core_state;
    let input = batch(
        &model,
        vec![
            E::SurfaceConverted {
                id: 1,
                kind: "empty".into(),
                data: None,
            },
            E::SurfaceClosed { id: 2 },
            E::TabClosed { id: 1 },
            E::WorkspaceClosed { id: 1 },
        ],
    );
    let mut retired = Vec::new();
    apply(
        core,
        &model,
        &input,
        &mut HashMap::from([prepared(1)]),
        &mut retired,
    )
    .unwrap();
    assert!(core.local_workspaces.is_empty());
    assert_eq!(retired.len(), 4);
    assert!(matches!(&retired[0], Retired::Surface(surface) if surface.surface_id() == Some(1)));
    assert!(matches!(&retired[1], Retired::Surface(surface) if surface.surface_id() == Some(2)));
    assert!(matches!(&retired[2], Retired::Tab(tab) if tab.id == 1));
    assert!(matches!(&retired[3], Retired::Workspace(workspace) if workspace.id == 1));
}

#[test]
fn a_retired_pane_keeps_its_kind_objects_until_cleanup() {
    let (mut session, mut model) = seeded();
    let input = batch(
        &model,
        vec![
            E::PaneSplit {
                target: 1,
                pane: 2,
                split: SplitSpec {
                    direction: SplitDirection::Vertical,
                    ratio: Ratio::from_f32(0.5),
                    placement: Placement::After,
                },
            },
            E::TabCreated {
                id: 2,
                pane: 2,
                index: 0,
                name: "second".into(),
                surface: SurfaceSpec {
                    id: 3,
                    kind: "empty".into(),
                    data: None,
                },
            },
        ],
    );
    apply(
        &mut session.core_state,
        &model,
        &input,
        &mut HashMap::from([prepared(3)]),
        &mut Vec::new(),
    )
    .unwrap();
    crate::evolve(&mut model, &input).unwrap();
    let input = batch(&model, vec![E::PaneClosed { id: 2 }]);
    let mut retired = Vec::new();
    apply(
        &mut session.core_state,
        &model,
        &input,
        &mut HashMap::new(),
        &mut retired,
    )
    .unwrap();
    assert!(
        matches!(&retired[0], Retired::Pane(pane) if pane.id == 2 && pane.all_surface_ids() == [3])
    );
    assert!(session.core_state.find_surface_by_id(1).is_some());
}
