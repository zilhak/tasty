use super::common::{batch, scenario_batches};
use crate::*;

fn initial() -> JournalModel {
    let mut model = JournalModel::default();
    for input in scenario_batches().into_iter().take(3) {
        evolve(&mut model, &input).unwrap();
    }
    model
}

fn execute(model: &mut JournalModel, cmd: &StructuralCommand) -> StructuralDecision {
    let before = model.clone();
    let decision = decide_structure(model, cmd).unwrap();
    assert_eq!(*model, before, "decide must not publish its candidate");
    let input = batch(
        model.applied.batch.unwrap() + 1,
        model.applied.revision.unwrap(),
        &decision.events,
    );
    evolve(model, &input).unwrap();
    decision
}

#[test]
fn category_delete_preserves_workspace_order_and_moves_members_to_normal() {
    let mut model = initial();
    let order = model.workspace_order.clone();
    execute(&mut model, &StructuralCommand::DeleteCategory { id: 1 });
    assert_eq!(model.workspace_order, order);
    assert!(model.workspaces.values().all(|w| w.category == 0));
    assert!(!model.categories.contains_key(&1));
    assert_eq!(
        decide_structure(&model, &StructuralCommand::DeleteCategory { id: 0 })
            .unwrap_err()
            .0,
        "the 'normal' category cannot be renamed or deleted"
    );
}

#[test]
fn optional_metadata_fields_preserve_unspecified_values_and_empty_names() {
    let mut model = initial();
    let surfaces = model.surfaces.clone();
    execute(
        &mut model,
        &StructuralCommand::UpdateWorkspaceMeta {
            workspace_id: 1,
            name: None,
            subtitle: Some("kept".into()),
            description: Some("old".into()),
        },
    );
    execute(
        &mut model,
        &StructuralCommand::UpdateWorkspaceMeta {
            workspace_id: 1,
            name: Some(String::new()),
            subtitle: None,
            description: Some("new".into()),
        },
    );
    assert_eq!(model.workspaces[&1].name, "");
    assert_eq!(model.workspaces[&1].subtitle, "kept");
    assert_eq!(model.workspaces[&1].description, "new");
    execute(
        &mut model,
        &StructuralCommand::RenameTab {
            tab_id: 1,
            name: Some(String::new()),
        },
    );
    assert_eq!(model.tabs[&1].explicit_name.as_deref(), Some(""));
    assert_eq!(model.surfaces, surfaces);
}

#[test]
fn queued_reordering_keeps_its_explicit_source_and_invalid_moves_stay_noops() {
    let mut model = initial();
    let queued = StructuralCommand::MoveWorkspace {
        workspace_id: 1,
        to_index: 1,
    };
    execute(
        &mut model,
        &StructuralCommand::MoveWorkspace {
            workspace_id: 2,
            to_index: 0,
        },
    );
    let response = execute(&mut model, &queued);
    assert_eq!(model.workspace_order, vec![2, 1]);
    assert_eq!(response.result, StructuralResult::Moved { moved: false });
    let invalid = execute(
        &mut model,
        &StructuralCommand::MoveWorkspace {
            workspace_id: 999,
            to_index: 0,
        },
    );
    assert!(invalid.events.is_empty());
    assert_eq!(invalid.result, StructuralResult::Moved { moved: false });
}

#[test]
fn ratio_target_precondition_prevents_a_delayed_drag_from_touching_another_split() {
    let model = initial();
    let before = model.clone();
    assert!(
        decide_structure(
            &model,
            &StructuralCommand::SetSurfaceRatio {
                tab_id: 1,
                path: vec![],
                expected_revision: model.applied.revision.unwrap(),
                expected_leaves: vec![999, 1],
                ratio: Ratio::from_f32(0.5)
            }
        )
        .is_err()
    );
    assert_eq!(model, before);
    let result = decide_structure(
        &model,
        &StructuralCommand::SetSurfaceRatio {
            tab_id: 1,
            path: vec![],
            expected_revision: model.applied.revision.unwrap(),
            expected_leaves: vec![3, 1],
            ratio: Ratio::from_f32(0.5),
        },
    )
    .unwrap();
    assert_eq!(result.events.len(), 1);
}

#[test]
fn a_recreated_split_with_the_same_leaves_rejects_the_old_revision() {
    let mut model = initial();
    let old = StructuralCommand::SetSurfaceRatio {
        tab_id: 1,
        path: vec![],
        expected_leaves: vec![3, 1],
        expected_revision: model.applied.revision.unwrap(),
        ratio: Ratio::from_f32(0.6),
    };
    assert!(decide_structure(&model, &old).is_ok());
    // A later committed structure with identical leaves must still reject the old drag.
    model.applied.revision = Some(model.applied.revision.unwrap() + 2);
    let before = model.clone();
    assert_eq!(
        decide_structure(&model, &old).unwrap_err().0,
        "split changed since the drag began"
    );
    assert_eq!(model, before);
}
