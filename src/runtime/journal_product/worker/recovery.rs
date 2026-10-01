//! Reconcile abandoned execution before the first product projection is exposed.
//!
//! A new writer lease proves exclusive journal access, not child termination or delivery to a
//! remote peer. This coordinator never replays shell input, plugin hooks or historical creates.
use super::*;
use tasty_core::{OperationId,OperationOutcome,StructuralCommand,StructureModels};
use tasty_event_store::{EffectState,EffectTransition,CommandRecord,CommandStatus,CommandKey};

pub(super) fn recover(executor:&Executor<StructureDecider>)->Result<(),String> {
    let work=executor.with_state(|models| {
        let mut work:Vec<_>=models.streams.iter().flat_map(|(stream,model)|model.operations.values().filter(|operation|operation.outcome.is_none()).map(move |operation|(operation.assembly.is_some(),stream.clone(),operation.id.clone()))).collect();
        // Leaf observations settle first. The coordinator can then retain uncertainty or cancel
        // an entirely unstarted group without publishing a partially materialized subtree.
        work.sort_by_key(|(assembly,_,_)|*assembly);work
    }).map_err(|error|error.to_string())?;
    for (_,stream,operation) in work {recover_one(executor,&stream,&operation)?;}
    Ok(())
}

fn recover_one(executor:&Executor<StructureDecider>,stream:&str,id:&OperationId)->Result<(),String> {
    let (request,holder)={
        let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;
        let epoch=inner.epoch;
        let model=inner.state.streams.get(stream).ok_or("recovery engine disappeared")?;
        let operation=model.operations.get(id).ok_or("recovery operation disappeared")?.clone();
        if operation.outcome.is_some() {return Ok(());}
        let member_uncertain=operation.assembly.as_ref().is_some_and(|plan|plan.snapshot.surfaces.keys().any(|surface| {
            model.operations.get(&tasty_core::CreationAssembly::member(id,*surface)).is_none_or(|member|!matches!(member.outcome,Some(OperationOutcome::Cancelled {..}|OperationOutcome::Failed {..})))
        }));
        let suffix=if operation.forward {"forward"}else if operation.retirement.is_some(){"retire"}else {"prepare"};
        let effect=if operation.assembly.is_some() {None} else {
            let effect=inner.store.effect(&format!("{}/{suffix}",id.0)).map_err(|error|error.to_string())?.ok_or("recovery operation has no effect obligation")?;
            effects::validate_binding(&effect,stream,&operation)?;
            if inner.store.effect_origin_epoch(&effect.effect_id).map_err(|error|error.to_string())?.0>=epoch.0 {return Err("startup recovery encountered an obligation from its current or a newer writer".into());}
            Some(effect)
        };
        let started=effect.as_ref().is_some_and(|effect|effect.attempt>0 || !matches!(effect.state,EffectState::Pending|EffectState::Deferred));
        let unknown=operation.retirement.is_some() || member_uncertain || started || operation.resource_prepared || operation.pending_outcome.is_some();
        let reason=if unknown {"previous runtime ended without a committed resource or delivery receipt"}else {"historical operation was never claimed; only selected live surfaces may activate"};
        let outcome=if unknown {OperationOutcome::Uncertain {reason:reason.into()}}else {OperationOutcome::Cancelled {reason:reason.into()}};
        let observation=serde_json::json!({
            "version":1,"stream":stream,"operation":id,"engine_incarnation":operation.engine_incarnation,
            "new_runtime_epoch":epoch.0,"reason":reason,
            "effect":effect.as_ref().map(|effect|serde_json::json!({"id":effect.effect_id,"state":format!("{:?}",effect.state),"attempt":effect.attempt,"generation":effect.resource_generation})),
            "physical_reap_confirmed":false,"remote_delivery_confirmed":false,
        });
        let bytes=serde_json::to_vec(&observation).map_err(|error|error.to_string())?;
        let holder=format!("recovery/{}/{stream}/{}",epoch.0,id.0);
        let evidence=inner.store.put_payload_pinned(epoch,&bytes,&holder).map_err(|error|error.to_string())?;
        let transition=effect.as_ref().and_then(|effect| {
            let to=match effect.state {
                EffectState::Running=>EffectState::Uncertain,
                EffectState::Pending|EffectState::Deferred if !unknown=>EffectState::Cancelled,
                _=>return None,
            };
            Some(EffectTransition {effect_id:effect.effect_id.clone(),from:effect.state,to,resource_generation:effect.resource_generation,attempt:(effect.state==EffectState::Running).then_some(effect.attempt),claim:None,result:Some(bytes.clone())})
        });
        let original_results=std::collections::BTreeMap::from([(operation.command_id.clone(),effects::read_original_results(&inner.store,&operation.command_id)?)]);
        let command=StructuralCommand::RecoverOperation {operation:id.clone(),outcome,evidence:tasty_core::DataRef(evidence.0)};
        let request=ExecuteRequest {
            key:Some(CommandKey {caller_scope:"resource-recovery".into(),idempotency_key:format!("{}/{stream}/{}",epoch.0,id.0)}),
            actor:"system".into(),origin:"resource-recovery".into(),causation_id:Some(operation.command_id),
            command:ResolvedCommand {original_digest:bytes,response:None,changes:vec![crate::runtime::journal_product::StreamCommand {stream:stream.into(),command}],effect_result:None,cancellation:transition,completion_view:None,original_results},
        };
        (request,holder)
    };
    executor.execute(&request).map_err(|error|error.to_string())?;
    let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;let epoch=inner.epoch;
    inner.store.release_payload_holder(epoch,&holder).map_err(|error|error.to_string())?;
    Ok(())
}

/// The durable command remains InProgress. The current caller receives an explicit unknown
/// outcome instead of waiting for an operation that has no live execution continuation.
pub(super) fn command_result(models:&StructureModels,record:CommandRecord,replay:bool)->ResultValue {
    if record.status==CommandStatus::InProgress {
        let reason=models.streams.values().flat_map(|model|model.operations.values()).filter(|operation|operation.command_id==record.command_id).find_map(|operation|match &operation.outcome {Some(OperationOutcome::Uncertain {reason})=>Some(reason.clone()),_=>None});
        if let Some(reason)=reason {return ResultValue::RecoveryRequired {command_id:record.command_id,reason,replay};}
    }
    if replay {ResultValue::Stored(record)} else {ResultValue::Command(record)}
}

/// A receipt from the retained in-process owner can complete an uncertain retirement. An old
/// runtime's PID, a missing surface, or a new activation is never accepted as this proof.
pub(super) fn reconcile_retirement(executor:&Executor<StructureDecider>,lease:crate::runtime::journal_product::EffectLease,evidence:Vec<u8>)->Result<ResultValue,String> {
    let (request,holder)={
        let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;let epoch=inner.epoch;
        if epoch.0!=lease.runtime_epoch {return Err("reconciliation receipt belongs to a retired runtime".into());}
        let observation:serde_json::Value=serde_json::from_slice(&evidence).map_err(|error|error.to_string())?;
        let observed_lease:crate::runtime::journal_product::EffectLease=serde_json::from_value(observation["lease"].clone()).map_err(|error|error.to_string())?;
        if observed_lease!=lease {return Err("receipt and result lease name different owners".into());}
        if observation["source"]!="owned-retirement-receipts" || observation["runtime_epoch"].as_u64()!=Some(epoch.0) || observation["metadata_complete"]!=true {return Err("retirement observation is not an owned completion receipt".into());}
        let key=CommandKey {caller_scope:"retirement-reconciliation".into(),idempotency_key:format!("{}/{}/{}",lease.effect_id,lease.attempt,epoch.0)};
        use sha2::Digest;let digest=sha2::Sha256::digest(&evidence).to_vec();
        match inner.store.lookup_command(&key,&digest).map_err(|error|error.to_string())? {
            CommandLookup::Hit(record)=>return Ok(ResultValue::Stored(record)),CommandLookup::DigestMismatch(_)=>return Err("reconciliation key has another receipt".into()),CommandLookup::Miss=>{},
        }
        let operation=inner.state.streams.get(&lease.stream).and_then(|model|model.operations.get(&lease.operation)).ok_or("reconciliation operation missing")?.clone();
        if operation.retirement.is_none() || !matches!(operation.outcome,Some(OperationOutcome::Uncertain {..})) || observation["engine_incarnation"].as_u64()!=Some(operation.engine_incarnation) {return Err("reconciliation belongs to another retirement".into());}
        let observed_targets:Vec<tasty_core::RetiredSurface>=serde_json::from_value(observation["targets"].clone()).map_err(|error|error.to_string())?;
        if operation.retirement.as_ref().is_none_or(|plan|plan.surfaces!=observed_targets) {return Err("receipt names different retired resources".into());}
        let effect=inner.store.effect(&lease.effect_id).map_err(|error|error.to_string())?.ok_or("reconciliation effect missing")?;
        effects::validate_binding(&effect,&lease.stream,&operation)?;
        if effect.state!=EffectState::Uncertain || effect.attempt!=lease.attempt || effect.resource_generation!=lease.resource_generation {return Err("reconciliation attempt is stale".into());}
        let holder=format!("recovery-receipt/{}/{}",epoch.0,lease.effect_id);
        let data=inner.store.put_payload_pinned(epoch,&evidence,&holder).map_err(|error|error.to_string())?;
        let original_results=std::collections::BTreeMap::from([(operation.command_id.clone(),effects::read_original_results(&inner.store,&operation.command_id)?)]);
        let request=ExecuteRequest {key:Some(key),actor:"system".into(),origin:"owned-retirement-reconciliation".into(),causation_id:Some(operation.command_id),command:ResolvedCommand {
            original_digest:digest,response:None,changes:vec![crate::runtime::journal_product::StreamCommand {stream:lease.stream.clone(),command:StructuralCommand::ReconcileRetirement {operation:lease.operation.clone(),evidence:tasty_core::DataRef(data.0)}}],effect_result:None,
            cancellation:Some(EffectTransition {effect_id:lease.effect_id,from:EffectState::Uncertain,to:EffectState::Succeeded,resource_generation:lease.resource_generation,attempt:Some(lease.attempt),claim:None,result:Some(evidence)}),completion_view:None,original_results,
        }};(request,holder)
    };
    let result=executor.execute(&request).map(ResultValue::Executed).map_err(|error|error.to_string())?;
    let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;let epoch=inner.epoch;
    inner.store.release_payload_holder(epoch,&holder).map_err(|error|error.to_string())?;Ok(result)
}
