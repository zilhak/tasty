//! A mirror command records an external submission obligation, never a local mirror tree mutation.
use super::*;
use crate::{Operation,OperationOutcome,StructuralEffect};
pub(super) fn decide(model:&JournalModel,command:&StructuralCommand)->Result<StructuralDecision,Rejection> {
    match command {
        StructuralCommand::PrepareForward {operation,command_id,input}=> {
            let operation=Operation {id:operation.clone(),command_id:command_id.clone(),engine_incarnation:model.engine_incarnation,
                creation:None,assembly:None,retirement:None,forward:true,targets:Vec::new(),reserved:Vec::new(),input:*input,activation_generation:0,
                outcome:None,pending_outcome:None,cleanup:None,prepared_data:None,prepared_deferred:false,resource_prepared:false,reconciliation_evidence:None};
            Ok(StructuralDecision {events:vec![DomainEvent::OperationPrepared {operation:operation.clone()}],effects:vec![StructuralEffect::ForwardStructure {operation:operation.id.clone(),input:*input}],result:StructuralResult::Pending {operation:operation.id},completed_command:None})
        },
        StructuralCommand::FinishForward {operation,outcome}=> {
            let original=model.operations.get(operation).filter(|operation|operation.forward && operation.outcome.is_none()).ok_or_else(||Rejection("forward operation is not pending".into()))?;
            let result=match outcome {OperationOutcome::Succeeded=>StructuralResult::Updated,OperationOutcome::Uncertain {..}=>StructuralResult::Pending {operation:operation.clone()},OperationOutcome::Failed {reason}|OperationOutcome::Cancelled {reason}=>StructuralResult::Failed {reason:reason.clone()},_=>return Err(Rejection("forward cannot be silently superseded".into()))};
            Ok(StructuralDecision {events:vec![DomainEvent::OperationFinished {id:operation.clone(),outcome:outcome.clone()}],effects:Vec::new(),result,completed_command:(!matches!(outcome,OperationOutcome::Uncertain {..})).then(||original.command_id.clone())})
        },
        _=>unreachable!(),
    }
}
