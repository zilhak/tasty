use super::common::{batch, scenario_batches};
use crate::*;

fn initial(split: bool) -> JournalModel {
    let mut model = JournalModel::default();
    for input in scenario_batches()
        .into_iter()
        .take(if split { 2 } else { 1 })
    {
        evolve(&mut model, &input).unwrap();
    }
    model
}

fn convert(operation: &str, kind: &str, name: Option<Option<String>>) -> StructuralCommand {
    StructuralCommand::PrepareCreation {
        operation: OperationId(operation.into()),
        command_id: format!("command-{operation}"),
        input: DataRef(77),
        plan: CreationPlan {
            destination: CreationDestination::Convert {
                surface: 1,
                previous_activation: None,
                explicit_name: name,
            },
            surface: SurfaceSpec {
                id: 1,
                kind: kind.into(),
                data: None,
            },
            tab_name: "unused for conversion".into(),
            explicit_name: None,
        },
    }
}

fn execute(model: &mut JournalModel, command: &StructuralCommand) -> StructuralDecision {
    let decision = decide_structure(model, command).unwrap();
    let input = batch(
        model.applied.batch.unwrap() + 1,
        model.applied.revision.unwrap(),
        &decision.events,
    );
    evolve(model, &input).unwrap();
    decision
}

#[test]
fn failed_and_compacted_preparations_never_reuse_an_activation_generation() {
    let mut model = initial(false);
    for (index, id) in ["one", "two", "three"].into_iter().enumerate() {
        let decision = execute(&mut model, &convert(id, "markdown", None));
        assert!(
            matches!(&decision.effects[0], StructuralEffect::PrepareSurface { activation_generation, .. } if *activation_generation == index as u64 + 1)
        );
        execute(
            &mut model,
            &StructuralCommand::FinishCreation {
                operation: OperationId(id.into()),
                result: PreparationResult::Failed {
                    reason: "prepare failed".into(),
                },
            },
        );
        assert_eq!(model.surfaces[&1].activation, None);
        model.operations.clear();
    }
    assert_eq!(model.activation_high_water[&1], 3);
}

#[test]
fn conversion_preserves_the_old_instance_until_commit_and_waits_for_cleanup() {
    let mut model = initial(false);
    let old_surface = model.surfaces[&1].clone();
    execute(
        &mut model,
        &convert("convert", "markdown", Some(Some("notes.md".into()))),
    );
    assert_eq!(
        model.surfaces[&1], old_surface,
        "preparation publishes no replacement"
    );
    let operation = OperationId("convert".into());
    let result = execute(
        &mut model,
        &StructuralCommand::FinishCreation {
            operation: operation.clone(),
            result: PreparationResult::Ready {
                data: Some(DataRef(88)),
            },
        },
    );
    assert_eq!(
        model.surfaces[&1], old_surface,
        "installation authorization preserves the old visible structure"
    );
    assert_eq!(model.tabs[&1].explicit_name, None);
    assert_eq!(result.completed_command, None);
    assert!(
        result.effects.is_empty(),
        "retirement continues under the original Running effect claim"
    );
    assert_eq!(model.operations[&operation].outcome, None);
    assert!(matches!(
        model.operations[&operation].cleanup,
        Some(CleanupPlan::InstallPrepared {
            surface: 1,
            previous_activation: Some(None)
        })
    ));
    assert_eq!(model.surfaces[&1].activation, old_surface.activation);
    let complete = execute(
        &mut model,
        &StructuralCommand::FinishCleanup {
            operation: operation.clone(),
        },
    );
    assert_eq!(
        complete.completed_command.as_deref(),
        Some("command-convert")
    );
    assert_eq!(
        model.operations[&operation].outcome,
        Some(OperationOutcome::Succeeded)
    );
    assert_eq!(model.operations[&operation].cleanup, None);
    assert_eq!(model.surfaces[&1].kind, "markdown");
    assert_eq!(model.surfaces[&1].data, Some(DataRef(88)));
    assert_eq!(model.tabs[&1].explicit_name.as_deref(), Some("notes.md"));
    assert_eq!(
        model.surfaces[&1].activation.unwrap().phase,
        ActivationPhase::Ready
    );
}

#[test]
fn single_terminal_conversion_clears_explicit_name_but_split_conversion_keeps_it() {
    for split in [false, true] {
        let mut model = initial(split);
        execute(
            &mut model,
            &StructuralCommand::RenameTab {
                tab_id: 1,
                name: Some("kept".into()),
            },
        );
        execute(&mut model, &convert("convert", "terminal", Some(None)));
        execute(
            &mut model,
            &StructuralCommand::FinishCreation {
                operation: OperationId("convert".into()),
                result: PreparationResult::Ready { data: None },
            },
        );
        execute(
            &mut model,
            &StructuralCommand::FinishCleanup {
                operation: OperationId("convert".into()),
            },
        );
        assert_eq!(
            model.tabs[&1].explicit_name.as_deref(),
            split.then_some("kept")
        );
    }
}

#[test]
fn deleted_target_discards_prepared_resources_before_original_command_completion() {
    let mut model = initial(false);
    execute(&mut model, &convert("convert", "markdown", None));
    let remove = batch(
        model.applied.batch.unwrap() + 1,
        model.applied.revision.unwrap(),
        &[DomainEvent::WorkspaceClosed { id: 1 }],
    );
    evolve(&mut model, &remove).unwrap();
    let operation = OperationId("convert".into());
    let result = execute(
        &mut model,
        &StructuralCommand::FinishCreation {
            operation: operation.clone(),
            result: PreparationResult::Ready { data: None },
        },
    );
    assert_eq!(result.completed_command, None);
    assert!(!model.surfaces.contains_key(&1));
    assert!(matches!(
        model.operations[&operation].cleanup,
        Some(CleanupPlan::DiscardPrepared {
            surface: 1,
            activation_generation: 1
        })
    ));
    execute(
        &mut model,
        &StructuralCommand::FinishCleanup {
            operation: operation.clone(),
        },
    );
    assert!(matches!(
        model.operations[&operation].outcome,
        Some(OperationOutcome::Cancelled { .. })
    ));
}

#[test]
fn successful_creation_seed_survives_operation_compaction_and_restore_keeps_it() {
    let mut model = initial(false);
    let operation = OperationId("seeded".into());
    execute(&mut model, &convert("seeded", "markdown", None));
    execute(
        &mut model,
        &StructuralCommand::FinishCreation {
            operation: operation.clone(),
            result: PreparationResult::Ready { data: None },
        },
    );
    execute(&mut model, &StructuralCommand::FinishCleanup { operation });
    assert_eq!(model.surfaces[&1].creation_seed, Some(DataRef(77)));
    model.operations.clear();
    assert!(model.data_refs().any(|reference| reference == DataRef(77)));
    let surface = model.surfaces[&1].clone();
    let operation = OperationId("restored".into());
    execute(
        &mut model,
        &StructuralCommand::PrepareCreation {
            operation: operation.clone(),
            command_id: "restore-command".into(),
            input: DataRef(99),
            plan: CreationPlan {
                destination: CreationDestination::Restore {
                    surface: 1,
                    previous_activation: surface.activation.map(|a| a.generation),
                },
                surface: SurfaceSpec {
                    id: 1,
                    kind: surface.kind.clone(),
                    data: surface.data,
                },
                tab_name: String::new(),
                explicit_name: None,
            },
        },
    );
    execute(
        &mut model,
        &StructuralCommand::FinishCreation {
            operation: operation.clone(),
            result: PreparationResult::Ready { data: None },
        },
    );
    assert_eq!(
        model.surfaces[&1], surface,
        "authorization does not replace the committed leaf"
    );
    execute(&mut model, &StructuralCommand::FinishCleanup { operation });
    assert_eq!(model.surfaces[&1].kind, surface.kind);
    assert_eq!(model.surfaces[&1].data, surface.data);
    assert_eq!(model.surfaces[&1].creation_seed, Some(DataRef(77)));
    assert!(
        model.surfaces[&1].activation.unwrap().generation > surface.activation.unwrap().generation
    );
}

#[test]
fn retired_engine_discards_late_preparation_and_rejects_late_installation_success() {
    for already_authorized in [false, true] {
        let mut model = initial(false);
        model.engine_incarnation = 1;
        let old_surface = model.surfaces[&1].clone();
        let operation = OperationId("retired-candidate".into());
        execute(&mut model, &convert("retired-candidate", "markdown", None));
        if already_authorized {
            execute(
                &mut model,
                &StructuralCommand::FinishCreation {
                    operation: operation.clone(),
                    result: PreparationResult::Ready { data: None },
                },
            );
        }
        execute(
            &mut model,
            &StructuralCommand::RetireEngine {
                expected_incarnation: 1,
            },
        );
        if already_authorized {
            assert!(
                decide_structure(
                    &model,
                    &StructuralCommand::FinishCleanup {
                        operation: operation.clone()
                    }
                )
                .is_err()
            );
            assert!(
                model.operations[&operation].outcome.is_none(),
                "unknown external installation is not relabelled successful or failed"
            );
        } else {
            execute(
                &mut model,
                &StructuralCommand::FinishCreation {
                    operation: operation.clone(),
                    result: PreparationResult::Ready { data: None },
                },
            );
            assert!(matches!(
                model.operations[&operation].cleanup,
                Some(CleanupPlan::DiscardPrepared { .. })
            ));
            execute(
                &mut model,
                &StructuralCommand::FinishCleanup {
                    operation: operation.clone(),
                },
            );
            assert!(matches!(
                model.operations[&operation].outcome,
                Some(OperationOutcome::Cancelled { .. })
            ));
        }
        assert_eq!(model.surfaces[&1], old_surface);
        assert!(decide_structure(&model, &convert("new-candidate", "markdown", None)).is_err());
    }
}
