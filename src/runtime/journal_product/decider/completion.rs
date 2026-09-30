//! Finish an original command only after all of its operations, across streams, are terminal.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tasty_domain::{Operation, OperationId, OperationOutcome};

pub(super) fn aggregate(
    state: &StructureModels,
    command: &ResolvedCommand,
    decision: &Decision<StreamEvent, NewEffect>,
) -> Vec<tasty_event_store::CommandUpdate> {
    let mut operations: BTreeMap<OperationId, Operation> = state
        .streams
        .values()
        .flat_map(|model| {
            model
                .operations
                .iter()
                .map(|(id, operation)| (id.clone(), operation.clone()))
        })
        .collect();
    let mut affected = BTreeSet::new();
    for event in &decision.events {
        if let DomainEvent::OperationFinished { id, outcome } = &event.event {
            let operation = operations
                .get_mut(id)
                .expect("decide checked the operation");
            operation.outcome = Some(outcome.clone());
            affected.insert(operation.command_id.clone());
        }
    }
    affected
        .into_iter()
        .map(|command_id| {
            let own: Vec<_> = operations
                .values()
                .filter(|operation| operation.command_id == command_id)
                .collect();
            let status = if own.iter().any(|operation| {
                operation.outcome.is_none()
                    || matches!(operation.outcome, Some(OperationOutcome::Uncertain { .. }))
            }) {
                CommandStatus::InProgress
            } else if own
                .iter()
                .any(|operation| matches!(operation.outcome, Some(OperationOutcome::Failed { .. })))
            {
                CommandStatus::Failed
            } else if own.iter().any(|operation| {
                matches!(
                    operation.outcome,
                    Some(OperationOutcome::Cancelled { .. } | OperationOutcome::Superseded { .. })
                )
            }) {
                CommandStatus::Cancelled
            } else {
                CommandStatus::Completed
            };
            let mut results = command
                .original_results
                .get(&command_id)
                .expect("result admission loaded the original template")
                .clone();
            for result in &mut results {
                if let StructuralResult::Pending { operation: id } = result {
                    let operation = operations
                        .get(id)
                        .expect("original result refers to its operation");
                    *result = match &operation.outcome {
                        Some(OperationOutcome::Succeeded) => operation
                            .creation
                            .as_ref()
                            .expect("creation result has its plan")
                            .created_result(),
                        Some(
                            OperationOutcome::Failed { reason }
                            | OperationOutcome::Cancelled { reason }
                            | OperationOutcome::Superseded { reason },
                        ) => StructuralResult::Failed {
                            reason: reason.clone(),
                        },
                        None | Some(OperationOutcome::Uncertain { .. }) => continue,
                    };
                }
            }
            tasty_event_store::CommandUpdate {
                command_id,
                status,
                response: Some(
                    serde_json::to_vec(&results).expect("typed structural results serialize"),
                ),
            }
        })
        .collect()
}
