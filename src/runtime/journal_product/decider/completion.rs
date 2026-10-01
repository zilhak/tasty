//! Finish an original command only after all of its operations, across streams, are terminal.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tasty_domain::{Operation, OperationId, OperationOutcome};

pub(super) fn aggregate(
    state: &StructureModels,
    command: &ResolvedCommand,
    decision: &Decision<StreamEvent, NewEffect>,
) -> Result<Vec<tasty_event_store::CommandUpdate>, String> {
    if !decision
        .events
        .iter()
        .any(|event| matches!(event.event, DomainEvent::OperationFinished { .. }))
    {
        return Ok(Vec::new());
    }
    let after = projected_after(state, decision)?;
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
            let original = command
                .original_results
                .get(&command_id)
                .ok_or("original command response template missing")?;
            let mut progress = original.progress.clone();
            for result in &mut progress.results {
                if let StructuralResult::Pending { operation: id } = result {
                    let operation = operations
                        .get(id)
                        .expect("original result refers to its operation");
                    *result = match &operation.outcome {
                        Some(OperationOutcome::Succeeded)=> {
                            if operation.forward {StructuralResult::Updated} else if let Some(plan)=&operation.retirement {if plan.replacement.is_some() {StructuralResult::Moved {moved:true}}else {StructuralResult::Closed {closed:true}}}
                            else if let Some(plan)=&operation.assembly {
                                let surviving=plan.snapshot.surfaces.keys().filter(|surface|operations.get(&tasty_domain::CreationAssembly::member(&operation.id,**surface)).is_some_and(|member|matches!(member.outcome,Some(OperationOutcome::Succeeded)))).copied().collect();
                                plan.result(&surviving)
                            } else {operation.creation.as_ref().ok_or("completed operation has no result plan")?.created_result()}
                        },
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
            let response = if let Some(plan) = &original.response {
                let complete =
                    progress.freeze(plan, &after, command.completion_view.as_ref().unwrap_or(&super::super::CompletionView::default()))?;
                if status == CommandStatus::InProgress {
                    Some(serde_json::to_vec(&progress).map_err(|error| error.to_string())?)
                } else {
                    Some(complete.ok_or("terminal command still has pending public result parts")?)
                }
            } else {
                Some(serde_json::to_vec(&progress.results).map_err(|error| error.to_string())?)
            };
            Ok(tasty_event_store::CommandUpdate {
                command_id,
                status,
                response,
            })
        })
        .collect()
}

fn projected_after(
    state: &StructureModels,
    decision: &Decision<StreamEvent, NewEffect>,
) -> Result<StructureModels, String> {
    let mut after = state.clone();
    let mut streams: BTreeMap<&str, Vec<DomainEvent>> = BTreeMap::new();
    for event in &decision.events {
        streams
            .entry(event.stream.as_str())
            .or_default()
            .push(event.event.clone());
    }
    for (stream, events) in streams {
        let model = after.streams.entry(stream.into()).or_default();
        let revision = model.applied.revision.unwrap_or(0);
        let batch_id = model
            .applied
            .batch
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("projection batch exhausted")?;
        let events = events
            .into_iter()
            .enumerate()
            .map(|(index, event)| {
                Ok(tasty_domain::RecordedEvent {
                    revision: revision
                        .checked_add(index as u64 + 1)
                        .ok_or("projection revision exhausted")?,
                    event,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        tasty_domain::evolve(model, &tasty_domain::DomainBatch { batch_id, events })
            .map_err(|error| error.to_string())?;
    }
    Ok(after)
}
