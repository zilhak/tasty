//! Shared pure validation and fixed-input construction for single and grouped resolves.
use super::*;

pub(super) fn request(
    admitted: &Pending,
    mut changes: Vec<super::super::StreamCommand>,
    response: Option<super::super::ResponsePlan>,
    models: &tasty_core::StructureModels,
) -> Result<ExecuteRequest<ResolvedCommand>, String> {
    for change in &changes {
        if matches!(
            change.command,
            tasty_core::StructuralCommand::ReconcilePreparation { .. }
                | tasty_core::StructuralCommand::ReconcileRetirement { .. }
                | tasty_core::StructuralCommand::RecoverOperation { .. }
                | tasty_core::StructuralCommand::RecordCapture { .. }
                | tasty_core::StructuralCommand::OpenEngine { .. }
                | tasty_core::StructuralCommand::RetireEngine { .. }
                | tasty_core::StructuralCommand::FinishCreation { .. }
                | tasty_core::StructuralCommand::FinishCleanup { .. }
                | tasty_core::StructuralCommand::FinishRetirement { .. }
                | tasty_core::StructuralCommand::FinishForward { .. }
                | tasty_core::StructuralCommand::CancelUnstartedCreation { .. }
                | tasty_core::StructuralCommand::RejectInstallation { .. }
                | tasty_core::StructuralCommand::MarkPreparationUncertain { .. }
        ) {
            return Err("effect results require their validated lease endpoint".into());
        }
        if let tasty_core::StructuralCommand::PrepareCreation { input, .. }
        | tasty_core::StructuralCommand::Close { input, .. }
        | tasty_core::StructuralCommand::PrepareAssembly { input, .. }
        | tasty_core::StructuralCommand::PrepareForward { input, .. }
        | tasty_core::StructuralCommand::Replace { input, .. } = &change.command
            && !admitted.inputs.contains(input)
        {
            return Err("preparation input belongs to another admission".into());
        }
        if let tasty_core::StructuralCommand::Close {
            undo: Some(capture),
            ..
        } = &change.command
            && capture
                .data_refs()
                .any(|reference| !admitted.inputs.contains(&reference))
        {
            return Err("undo capture belongs to another admission".into());
        }
        for required in change.command.reserved_ids() {
            if !admitted.reservations.iter().any(|range| {
                range.kind == required.kind.label() && range.contains(u64::from(required.id))
            }) {
                return Err("structural command uses an ID not reserved for its admission".into());
            }
        }
    }

    for change in &mut changes {
        if let tasty_core::StructuralCommand::PrepareCreation { plan, .. } = &mut change.command
            && let tasty_core::CreationDestination::Convert {
                surface,
                previous_activation,
                ..
            } = &mut plan.destination
            && *previous_activation == Some(0)
        {
            *previous_activation = models
                .streams
                .get(&change.stream)
                .and_then(|model| model.surfaces.get(surface))
                .and_then(|surface| surface.activation.map(|activation| activation.generation));
        }
    }
    let admission = &admitted.admission;
    Ok(ExecuteRequest {
        key: admission.key.clone(),
        actor: admission.actor.clone(),
        origin: admission.origin.clone(),
        causation_id: admission.causation_id.clone(),
        command: ResolvedCommand {
            original_digest: admission.original_digest.clone(),
            response,
            changes,
            effect_result: None,
            cancellation: None,
            completion_view: None,
            original_results: Default::default(),
        },
    })
}
