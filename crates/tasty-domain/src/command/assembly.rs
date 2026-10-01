use super::*;
use crate::{Operation,OperationId,OperationOutcome,CreationAssembly,CreationPlan,CreationDestination,StructuralEffect,SurfaceSpec};
use std::collections::BTreeSet;

pub(super) fn decide(model:&JournalModel,command:&StructuralCommand)->Result<StructuralDecision,Rejection> {
    let StructuralCommand::PrepareAssembly {operation,command_id,input,plan}=command else {unreachable!()};
    if !plan.target_is_live(model) || plan.snapshot.surfaces.is_empty()
        || plan.inputs.keys().ne(plan.snapshot.surfaces.keys()) {
        return Err(Rejection("restoration assembly is empty or its fixed inputs/target differ".into()));
    }
    // Validate the complete candidate structure before any factory can run.
    let all=plan.snapshot.surfaces.keys().copied().collect();
    let facts=plan.facts(model,&all)?;
    let mut candidate=model.clone();
    crate::evolve(&mut candidate,&crate::DomainBatch {
        batch_id:model.applied.batch.unwrap_or(0).checked_add(1).ok_or_else(||Rejection("batch exhausted".into()))?,
        events:facts.into_iter().enumerate().map(|(index,event)|crate::RecordedEvent {revision:model.applied.revision.unwrap_or(0)+index as u64+1,event}).collect(),
    }).map_err(|error|Rejection(error.to_string()))?;
    let coordinator=Operation {
        id:operation.clone(),command_id:command_id.clone(),engine_incarnation:model.engine_incarnation,
        creation:None,assembly:Some(plan.clone()),retirement:None,targets:Vec::new(),reserved:plan.reserved_ids(),input:*input,
        activation_generation:0,outcome:None,pending_outcome:None,cleanup:None,prepared_data:None,prepared_deferred:false,resource_prepared:false,reconciliation_evidence:None,
    };
    let mut events=vec![DomainEvent::OperationPrepared {operation:coordinator}];
    let mut effects=Vec::new();
    for (surface,value) in &plan.snapshot.surfaces {
        let id=CreationAssembly::member(operation,*surface);
        let generation=model.activation_high_water.get(surface).copied().unwrap_or(0).checked_add(1).ok_or_else(||Rejection("surface activation exhausted".into()))?;
        let tab=plan.snapshot.tabs.get(&value.tab).ok_or_else(||Rejection("assembly surface parent missing".into()))?;
        let member=Operation {
            id:id.clone(),command_id:command_id.clone(),engine_incarnation:model.engine_incarnation,
            creation:Some(CreationPlan {destination:CreationDestination::Assembly {operation:operation.clone()},surface:SurfaceSpec {id:*surface,kind:value.kind.clone(),data:value.data},tab_name:tab.name.clone(),explicit_name:tab.explicit_name.clone()}),
            assembly:None,retirement:None,targets:Vec::new(),reserved:Vec::new(),input:plan.inputs[surface],activation_generation:generation,
            outcome:None,pending_outcome:None,cleanup:None,prepared_data:None,prepared_deferred:false,resource_prepared:false,reconciliation_evidence:None,
        };
        effects.push(StructuralEffect::PrepareSurface {operation:id,input:member.input,surface:*surface,kind:value.kind.clone(),activation_generation:generation});
        events.push(DomainEvent::OperationPrepared {operation:member});
    }
    Ok(StructuralDecision {events,effects,result:StructuralResult::Pending {operation:operation.clone()},completed_command:None})
}

/// Installation of a member is durable but does not make its leaf independently visible.
/// The final result consists of surviving leaves under the original restore omission policy.
pub(super) fn settle(model:&JournalModel,member:&Operation,outcome:OperationOutcome)->Result<StructuralDecision,Rejection> {
    let Some(CreationPlan {destination:CreationDestination::Assembly {operation:group},..})=&member.creation else {return Err(Rejection("operation is not an assembly member".into()));};
    let coordinator=model.operations.get(group).ok_or_else(||Rejection("assembly coordinator missing".into()))?;
    let plan=coordinator.assembly.as_ref().ok_or_else(||Rejection("assembly plan missing".into()))?;
    let mut events=vec![DomainEvent::OperationFinished {id:member.id.clone(),outcome:outcome.clone()}];
    let mut ready=BTreeSet::new();let mut waiting=false;let mut failed=false;
    for surface in plan.snapshot.surfaces.keys() {
        let id=CreationAssembly::member(group,*surface);
        let result=if id==member.id {Some(&outcome)} else {model.operations.get(&id).and_then(|operation|operation.outcome.as_ref())};
        match result {
            Some(OperationOutcome::Succeeded)=>{ready.insert(*surface);},
            Some(OperationOutcome::Failed {..}|OperationOutcome::Cancelled {..})=>failed=true,
            _=>waiting=true,
        }
    }
    if waiting {return Ok(StructuralDecision {events,effects:Vec::new(),result:StructuralResult::Pending {operation:group.clone()},completed_command:None});}
    if failed && !plan.omit_failed && !ready.is_empty() {
        // Installed peers may have externally observable registration. Recovery must reconcile
        // those exact owners; an ordinary failed response would falsely close the obligation.
        events.push(DomainEvent::OperationFinished {id:group.clone(),outcome:OperationOutcome::Uncertain {reason:"assembly peer failed after resources were prepared".into()}});
        return Ok(StructuralDecision {events,effects:Vec::new(),result:StructuralResult::Pending {operation:group.clone()},completed_command:None});
    }
    if !plan.target_is_live(model) || coordinator.engine_incarnation!=model.engine_incarnation {
        events.push(DomainEvent::OperationFinished {id:group.clone(),outcome:OperationOutcome::Uncertain {reason:"assembly target changed after installation".into()}});
        return Ok(StructuralDecision {events,effects:Vec::new(),result:StructuralResult::Pending {operation:group.clone()},completed_command:None});
    }
    if ready.is_empty() {
        let reason="no restorable surface remained".to_owned();
        events.push(DomainEvent::OperationFinished {id:group.clone(),outcome:OperationOutcome::Failed {reason:reason.clone()}});
        return Ok(StructuralDecision {events,effects:Vec::new(),result:StructuralResult::Failed {reason},completed_command:Some(coordinator.command_id.clone())});
    }
    let mut completed=plan.clone();
    for surface in &ready {
        let operation=&model.operations[&CreationAssembly::member(group,*surface)];
        if let Some(data)=operation.prepared_data {completed.snapshot.surfaces.get_mut(surface).ok_or_else(||Rejection("prepared surface missing".into()))?.data=Some(data);}
    }
    events.extend(completed.facts(model,&ready)?);
    for surface in &ready {
        let operation=&model.operations[&CreationAssembly::member(group,*surface)];
        events.push(DomainEvent::SurfaceActivationChanged {id:*surface,previous_generation:None,activation:crate::Activation {generation:operation.activation_generation,phase:if operation.prepared_deferred {crate::ActivationPhase::Deferred}else {crate::ActivationPhase::Ready}}});
        events.push(DomainEvent::SurfaceCreationSeeded {id:*surface,activation_generation:operation.activation_generation,input:plan.snapshot.surfaces[surface].creation_seed.unwrap_or(operation.input)});
    }
    if let Some(id)=&plan.undo {events.push(DomainEvent::UndoRecordConsumed {id:id.clone()});}
    events.push(DomainEvent::OperationFinished {id:group.clone(),outcome:OperationOutcome::Succeeded});
    Ok(StructuralDecision {events,effects:Vec::new(),result:plan.result(&ready),completed_command:Some(coordinator.command_id.clone())})
}


/// Private resources must all have outcomes before a group may publish any external registration.
/// A failed preset prepares discard obligations for its peers instead of exposing a partial tree.
pub(super) fn prepared_member(model:&JournalModel,member:&Operation,result:&crate::PreparationResult)->Result<StructuralDecision,Rejection> {
    let Some(CreationPlan {destination:CreationDestination::Assembly {operation:group},..})=&member.creation else {return Err(Rejection("preparation is not an assembly member".into()));};
    let coordinator=model.operations.get(group).ok_or_else(||Rejection("assembly coordinator missing".into()))?;
    let plan=coordinator.assembly.as_ref().ok_or_else(||Rejection("assembly plan missing".into()))?;
    let mut events=vec![match result {
        crate::PreparationResult::Ready {data}=>DomainEvent::OperationResourcePrepared {id:member.id.clone(),data:*data,deferred:false},
        crate::PreparationResult::Deferred {data}=>DomainEvent::OperationResourcePrepared {id:member.id.clone(),data:*data,deferred:true},
        crate::PreparationResult::Failed {reason}=>DomainEvent::OperationFinished {id:member.id.clone(),outcome:OperationOutcome::Failed {reason:reason.clone()}},
    }];
    let mut private=Vec::new();let mut waiting=false;let mut failed=false;
    for surface in plan.snapshot.surfaces.keys() {
        let id=CreationAssembly::member(group,*surface);
        let peer=model.operations.get(&id).ok_or_else(||Rejection("assembly member missing".into()))?;
        if id==member.id {
            match result {
                crate::PreparationResult::Ready {data}=>private.push((peer,*data,false)),
                crate::PreparationResult::Deferred {data}=>private.push((peer,*data,true)),
                crate::PreparationResult::Failed {..}=>failed=true,
            }
        } else if peer.resource_prepared {private.push((peer,peer.prepared_data,peer.prepared_deferred));}
        else if matches!(peer.outcome,Some(OperationOutcome::Failed {..}|OperationOutcome::Cancelled {..})) {failed=true;}
        else {waiting=true;}
    }
    if !waiting {
        if private.is_empty() {
            let reason="no restorable surface remained".to_owned();
            events.push(DomainEvent::OperationFinished {id:group.clone(),outcome:OperationOutcome::Failed {reason:reason.clone()}});
            return Ok(StructuralDecision {events,effects:Vec::new(),result:StructuralResult::Failed {reason},completed_command:Some(coordinator.command_id.clone())});
        } else {
            let discarded=(!plan.omit_failed && failed) || !plan.target_is_live(model) || coordinator.engine_incarnation!=model.engine_incarnation;
            for (peer,data,deferred) in private {
                let surface=peer.creation.as_ref().ok_or_else(||Rejection("assembly peer has no leaf".into()))?.surface.id;
                let (outcome,cleanup)=if discarded {(OperationOutcome::Cancelled {reason:"assembly was cancelled before external installation".into()},crate::CleanupPlan::DiscardPrepared {surface,activation_generation:peer.activation_generation})}
                    else {(OperationOutcome::Succeeded,crate::CleanupPlan::InstallPrepared {surface,previous_activation:None})};
                events.push(DomainEvent::OperationAwaitingCleanup {id:peer.id.clone(),outcome,cleanup,prepared_data:data,deferred});
            }
        }
    }
    Ok(StructuralDecision {events,effects:Vec::new(),result:StructuralResult::Pending {operation:group.clone()},completed_command:None})
}
