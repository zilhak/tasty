use super::common::{batch, scenario_batches};
use crate::*;
use std::collections::BTreeMap;

fn execute(model: &mut JournalModel, command: StructuralCommand) -> StructuralDecision {
    let decision = decide_structure(model, &command).expect("valid command");
    evolve(
        model,
        &batch(
            model.applied.batch.unwrap_or(0) + 1,
            model.applied.revision.unwrap_or(0),
            &decision.events,
        ),
    )
    .expect("valid facts");
    decision
}

fn prepared() -> (JournalModel, OperationId, OperationId, OperationId) {
    let mut model = JournalModel::default();
    evolve(&mut model, &scenario_batches()[0]).unwrap();
    let group = OperationId("restore-pair".into());
    let leaf = |id| {
        (
            id,
            Surface {
                tab: 10,
                kind: "terminal".into(),
                data: None,
                creation_seed: None,
                metadata: Default::default(),
                activation: None,
                content_generation: 0,
                snapshot_schema: 0,
            },
        )
    };
    let plan = CreationAssembly {
        snapshot: ClosedSnapshot {
            version: 1,
            presentation: Default::default(),
            pane_position: None,
            root: EntityId {
                kind: IdKind::Tab,
                id: 10,
            },
            origin_workspace: Some(1),
            workspaces: Default::default(),
            panes: Default::default(),
            tab_name: None,
            tabs: BTreeMap::from([(
                10,
                Tab {
                    pane: 1,
                    name: "pair".into(),
                    explicit_name: None,
                    layout: SplitTree::Split {
                        direction: tasty_model::SplitDirection::Horizontal,
                        ratio: Ratio::from_f32(0.5),
                        first: Box::new(SplitTree::Leaf(10)),
                        second: Box::new(SplitTree::Leaf(11)),
                    },
                },
            )]),
            surfaces: BTreeMap::from([leaf(10), leaf(11)]),
        },
        destination: AssemblyDestination::Tab { pane: 1, index: 1 },
        inputs: BTreeMap::from([(10, DataRef(110)), (11, DataRef(111))]),
        undo: None,
        omit_failed: false,
    };
    execute(
        &mut model,
        StructuralCommand::PrepareAssembly {
            operation: group.clone(),
            command_id: "restore-command".into(),
            input: DataRef(100),
            plan,
        },
    );
    let first = CreationAssembly::member(&group, 10);
    let second = CreationAssembly::member(&group, 11);
    (model, group, first, second)
}

#[test]
fn prepared_assembly_round_trips_through_the_durable_event_codec() {
    let (model, group, _, _) = prepared();
    let mut operation = model.operations[&group].clone();
    let plan = operation.assembly.as_mut().expect("assembly coordinator");
    plan.snapshot.presentation = UndoPresentation {
        focused_panes: [(1, 1)].into(),
        selected_tabs: [(1, 10)].into(),
        selected_surfaces: [(10, 11)].into(),
    };
    let event = DomainEvent::OperationPrepared { operation };
    let encoded = encode_event(&event).expect("encode nonempty numeric maps");
    let decoded = decode_event(&encoded.type_tag, encoded.schema_version, &encoded.bytes)
        .unwrap_or_else(|error| {
            panic!("{error}; body={}", String::from_utf8_lossy(&encoded.bytes))
        });
    assert_eq!(decoded, event);
    assert_eq!(encode_event(&decoded).unwrap().bytes, encoded.bytes);
}

fn remove_target(model: &mut JournalModel) {
    evolve(
        model,
        &batch(
            model.applied.batch.unwrap() + 1,
            model.applied.revision.unwrap(),
            &[DomainEvent::WorkspaceClosed { id: 1 }],
        ),
    )
    .unwrap();
}

fn cancel(operation: OperationId) -> StructuralCommand {
    StructuralCommand::CancelUnstartedCreation {
        operation,
        reason: "target closed".into(),
    }
}

#[test]
fn cancelling_the_last_unstarted_member_disposes_its_prepared_peer() {
    let (mut model, group, first, second) = prepared();
    execute(
        &mut model,
        StructuralCommand::FinishCreation {
            operation: first.clone(),
            result: PreparationResult::Ready {
                data: Some(DataRef(120)),
            },
        },
    );
    remove_target(&mut model);
    let decision = execute(&mut model, cancel(second.clone()));
    assert_eq!(
        decision.completed_command, None,
        "private owner still needs its receipt"
    );
    assert!(matches!(
        model.operations[&second].outcome,
        Some(OperationOutcome::Cancelled { .. })
    ));
    assert!(matches!(
        model.operations[&first].cleanup,
        Some(CleanupPlan::DiscardPrepared { surface: 10, .. })
    ));
    assert_eq!(model.operations[&first].prepared_data, Some(DataRef(120)));
    assert_eq!(model.operations[&group].outcome, None);
    let done = execute(
        &mut model,
        StructuralCommand::FinishCleanup { operation: first },
    );
    assert_eq!(done.completed_command.as_deref(), Some("restore-command"));
    assert!(matches!(
        model.operations[&group].outcome,
        Some(OperationOutcome::Failed { .. })
    ));
    assert!(!model.tabs.contains_key(&10));
}

#[test]
fn all_unstarted_cancellations_finish_even_when_the_target_is_gone() {
    let (mut model, group, first, second) = prepared();
    remove_target(&mut model);
    execute(&mut model, cancel(first));
    let done = execute(&mut model, cancel(second));
    assert_eq!(done.completed_command.as_deref(), Some("restore-command"));
    assert!(matches!(
        model.operations[&group].outcome,
        Some(OperationOutcome::Failed { .. })
    ));
}

#[test]
fn an_unknown_prepared_peer_is_not_authorized_again_by_a_cancellation() {
    let (mut model, group, first, second) = prepared();
    execute(
        &mut model,
        StructuralCommand::FinishCreation {
            operation: first.clone(),
            result: PreparationResult::Ready { data: None },
        },
    );
    execute(
        &mut model,
        StructuralCommand::MarkPreparationUncertain {
            operation: first.clone(),
            reason: "owner observation lost".into(),
        },
    );
    remove_target(&mut model);
    let decision = execute(&mut model, cancel(second));
    assert_eq!(decision.completed_command, None);
    assert_eq!(model.operations[&first].cleanup, None);
    assert!(matches!(
        model.operations[&first].outcome,
        Some(OperationOutcome::Uncertain { .. })
    ));
    assert_ne!(
        model.operations[&group].outcome,
        Some(OperationOutcome::Succeeded)
    );
}

#[test]
fn imported_creation_seed_has_a_readable_event_tag() {
    let event = DomainEvent::SurfaceSeedImported {
        id: 10,
        input: DataRef(110),
    };
    let encoded = encode_event(&event).unwrap();
    assert_eq!(
        decode_event(&encoded.type_tag, encoded.schema_version, &encoded.bytes).unwrap(),
        event
    );
}
