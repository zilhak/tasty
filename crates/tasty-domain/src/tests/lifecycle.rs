use super::common::{batch, scenario_batches};
use crate::*;

fn operation() -> Operation {
    Operation {
        id: OperationId("create/1".into()),
        command_id: "command/1".into(),
        engine_incarnation: 0,
        creation: None,
        targets: vec![EntityId {
            kind: IdKind::Pane,
            id: 1,
        }],
        reserved: vec![EntityId {
            kind: IdKind::Surface,
            id: 99,
        }],
        input: DataRef(11),
        activation_generation: 1,
        outcome: None,
        pending_outcome: None,
        cleanup: None,
        prepared_data: None,
        reconciliation_evidence: None,
    }
}

pub(super) fn examples() -> Vec<DomainEvent> {
    vec![
        DomainEvent::EngineIncarnationStarted {
            previous: 0,
            current: 1,
        },
        DomainEvent::EngineRetired { incarnation: 1 },
        DomainEvent::OperationPrepared {
            operation: operation(),
        },
        DomainEvent::OperationAwaitingCleanup {
            id: operation().id,
            outcome: OperationOutcome::Succeeded,
            cleanup: CleanupPlan::DiscardPrepared {
                surface: 99,
                activation_generation: 1,
            },
            prepared_data: None,
        },
        DomainEvent::OperationFinished {
            id: operation().id,
            outcome: OperationOutcome::Uncertain {
                reason: "worker stopped".into(),
            },
        },
        DomainEvent::OperationReconciled {
            id: operation().id,
            outcome: OperationOutcome::Cancelled {
                reason: "confirmed unstarted".into(),
            },
            evidence: DataRef(12),
        },
        DomainEvent::SurfaceActivationChanged {
            id: 1,
            previous_generation: None,
            activation: Activation {
                generation: 1,
                phase: ActivationPhase::Ready,
            },
        },
        DomainEvent::SurfaceCreationSeeded {
            id: 1,
            activation_generation: 1,
            input: DataRef(11),
        },
        DomainEvent::SurfaceDataRecorded {
            id: 1,
            activation_generation: Some(1),
            content_generation: 1,
            snapshot_schema: 1,
            data: DataRef(13),
        },
        DomainEvent::SurfaceConverted {
            id: 1,
            kind: "markdown".into(),
            data: Some(DataRef(14)),
        },
        DomainEvent::PaneRatioSet {
            workspace: 1,
            path: vec![],
            ratio: Ratio::from_f32(0.4),
        },
        DomainEvent::SurfaceRatioSet {
            tab: 1,
            path: vec![],
            ratio: Ratio::from_f32(0.6),
        },
    ]
}

fn example(tag: &str) -> DomainEvent {
    examples()
        .into_iter()
        .find(|event| event.type_tag() == tag)
        .unwrap()
}

fn initial() -> JournalModel {
    let mut model = JournalModel::default();
    for input in scenario_batches().into_iter().take(2) {
        evolve(&mut model, &input).unwrap();
    }
    model
}

fn apply(model: &mut JournalModel, events: &[DomainEvent]) -> Result<(), EvolveError> {
    let input = batch(
        model.applied.batch.unwrap_or(0) + 1,
        model.applied.revision.unwrap_or(0),
        events,
    );
    evolve(model, &input)
}

#[test]
fn pending_reservations_replay_without_worker_state_or_visible_surfaces() {
    let mut model = initial();
    let visible = model.surfaces.clone();
    apply(
        &mut model,
        &[DomainEvent::OperationPrepared {
            operation: operation(),
        }],
    )
    .unwrap();
    assert_eq!(model.surfaces, visible);
    assert!(model.operations[&operation().id].outcome.is_none());
    let unchanged = model.clone();
    let mut duplicate = operation();
    duplicate.id = OperationId("another-operation".into());
    assert!(
        apply(
            &mut model,
            &[DomainEvent::OperationPrepared {
                operation: duplicate
            }]
        )
        .is_err()
    );
    assert_eq!(
        model, unchanged,
        "reserved identities cannot be reused by another operation"
    );
    apply(
        &mut model,
        &[
            example("operation.finished"),
            example("operation.reconciled"),
        ],
    )
    .unwrap();
    assert_eq!(
        model.operations[&operation().id].reconciliation_evidence,
        Some(DataRef(12))
    );
    let terminal = model.clone();
    assert!(
        apply(
            &mut model,
            &[DomainEvent::OperationFinished {
                id: operation().id,
                outcome: OperationOutcome::Succeeded
            }]
        )
        .is_err()
    );
    assert_eq!(model, terminal);
}

#[test]
fn stale_activation_and_capture_facts_fail_without_partial_publication() {
    let mut model = initial();
    apply(&mut model, &[example("surface.activation_changed")]).unwrap();
    apply(
        &mut model,
        &[DomainEvent::SurfaceActivationChanged {
            id: 1,
            previous_generation: Some(1),
            activation: Activation {
                generation: 2,
                phase: ActivationPhase::Ready,
            },
        }],
    )
    .unwrap();
    let before = model.clone();
    assert!(
        apply(
            &mut model,
            &[
                DomainEvent::TabExplicitNameSet {
                    id: 1,
                    name: Some("must roll back".into())
                },
                DomainEvent::SurfaceDataRecorded {
                    id: 1,
                    activation_generation: Some(1),
                    content_generation: 1,
                    snapshot_schema: 1,
                    data: DataRef(13)
                },
            ]
        )
        .is_err()
    );
    assert_eq!(model, before);
    assert!(
        apply(
            &mut model,
            &[DomainEvent::SurfaceActivationChanged {
                id: 1,
                previous_generation: Some(1),
                activation: Activation {
                    generation: 3,
                    phase: ActivationPhase::Ready
                }
            }]
        )
        .is_err()
    );
    assert_eq!(model, before);
}

#[test]
fn split_ratio_facts_target_a_node_and_reject_nonfinite_values_atomically() {
    let mut model = initial();
    apply(
        &mut model,
        &[example("pane.ratio_set"), example("surface.ratio_set")],
    )
    .unwrap();
    let before = model.clone();
    for (path, ratio) in [(vec![false], 0.5), (vec![], f32::NAN), (vec![], 1.0)] {
        assert!(
            apply(
                &mut model,
                &[DomainEvent::PaneRatioSet {
                    workspace: 1,
                    path,
                    ratio: Ratio::from_f32(ratio)
                }]
            )
            .is_err()
        );
        assert_eq!(model, before);
    }
}

#[test]
fn lifecycle_facts_and_payload_references_survive_snapshot_replay() {
    let mut model = initial();
    apply(&mut model, &examples()).unwrap();
    let mut models = StructureModels {
        batch: model.applied.batch,
        ..Default::default()
    };
    models.streams.insert("structure:engine-test".into(), model);
    assert_eq!(
        decode_snapshot(MODEL_VERSION, &encode_snapshot(&models).unwrap()).unwrap(),
        models
    );
    assert_eq!(example("operation.prepared").data_refs(), vec![DataRef(11)]);
    assert_eq!(
        example("operation.reconciled").data_refs(),
        vec![DataRef(12)]
    );
    assert_eq!(
        example("surface.data_recorded").data_refs(),
        vec![DataRef(13)]
    );
    assert_eq!(example("surface.converted").data_refs(), vec![DataRef(14)]);
}

#[test]
fn older_capture_completion_cannot_replace_newer_content_in_the_same_activation() {
    let mut model = initial();
    apply(&mut model, &[example("surface.activation_changed")]).unwrap();
    apply(
        &mut model,
        &[DomainEvent::SurfaceDataRecorded {
            id: 1,
            activation_generation: Some(1),
            content_generation: 2,
            snapshot_schema: 1,
            data: DataRef(20),
        }],
    )
    .unwrap();
    let before = model.clone();
    assert!(
        apply(
            &mut model,
            &[DomainEvent::SurfaceDataRecorded {
                id: 1,
                activation_generation: Some(1),
                content_generation: 1,
                snapshot_schema: 1,
                data: DataRef(19)
            }]
        )
        .is_err()
    );
    assert_eq!(model, before);
    assert_eq!(model.surfaces[&1].data, Some(DataRef(20)));
}

#[test]
fn retired_binding_cannot_resume_its_old_structure_or_rewind_generations() {
    let mut model = initial();
    model.engine_incarnation = 7;
    model.activation_high_water.insert(1, 19);
    let old_ids = model.workspace_order.clone();
    let retirement = decide_structure(
        &model,
        &StructuralCommand::RetireEngine {
            expected_incarnation: 7,
        },
    )
    .unwrap();
    let revision = model.applied.revision.unwrap_or(0);
    evolve(&mut model, &batch(100, revision, &retirement.events)).unwrap();
    assert!(model.engine_retired);
    assert_eq!(
        model.workspace_order, old_ids,
        "retirement preserves reconciliation evidence"
    );
    assert!(
        decide_structure(
            &model,
            &StructuralCommand::RetireEngine {
                expected_incarnation: 6
            }
        )
        .is_err()
    );
    let reset = decide_structure(
        &model,
        &StructuralCommand::OpenEngine {
            expected_incarnation: 7,
            reset_structure: false,
            normal_category_name: "Normal".into(),
        },
    )
    .unwrap();
    let revision = model.applied.revision.unwrap_or(0);
    evolve(&mut model, &batch(101, revision, &reset.events)).unwrap();
    assert_eq!(model.engine_incarnation, 8);
    assert!(!model.engine_retired);
    assert!(model.workspace_order.is_empty());
    assert_eq!(model.activation_high_water[&1], 19);
}
