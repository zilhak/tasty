use tasty_domain::{OperationId, PreparationResult, StructuralCommand};
use tasty_event_store::{
    ActivationClaim, CommandKey, CommandLookup, EffectState, EffectTransition, PayloadRef,
};

use super::*;
use crate::runtime::journal_product::{
    ClaimedPreparation, EffectLease, PreparationInput, StreamCommand,
};

type Result<T> = std::result::Result<T, String>;

pub(super) fn claim(
    executor: &Executor<StructureDecider>,
    stream: &str,
    id: &OperationId,
) -> Result<ResultValue> {
    executor
        .with_state(|_| ())
        .map_err(|error| error.to_string())?;
    let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
    let operation = inner
        .state
        .streams
        .get(stream)
        .and_then(|model| model.operations.get(id))
        .ok_or("preparation operation missing")?
        .clone();
    if operation.outcome.is_some() || operation.pending_outcome.is_some() {
        return Err("preparation is not waiting for activation".into());
    }
    let plan = operation
        .creation
        .clone()
        .ok_or("operation has no creation plan")?;
    let bytes = inner
        .store
        .read_payload(PayloadRef(operation.input.0))
        .map_err(|error| error.to_string())?;
    let input: PreparationInput =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if input.kind != plan.surface.kind {
        return Err("preparation input kind differs from its committed plan".into());
    }
    let capture = if matches!(
        plan.destination,
        tasty_domain::CreationDestination::Restore { .. } | tasty_domain::CreationDestination::Assembly {..}
    ) {
        plan.surface
            .data
            .map(|reference| {
                inner
                    .store
                    .read_payload(PayloadRef(reference.0))
                    .map_err(|error| error.to_string())
            })
            .transpose()?
    } else {
        None
    };
    let effect_id = format!("{}/prepare", id.0);
    let effect = inner
        .store
        .effect(&effect_id)
        .map_err(|error| error.to_string())?
        .ok_or("preparation effect missing")?;
    validate_binding(&effect, stream, &operation)?;
    if !matches!(effect.state, EffectState::Pending | EffectState::Deferred) {
        return Err("preparation effect cannot be automatically claimed from this state".into());
    }
    let model = inner
        .state
        .streams
        .get(stream)
        .ok_or("engine stream missing")?;
    if input.adopt.as_ref().is_some_and(|recipe|recipe.runtime_epoch!=inner.epoch.0)
        || input.child.as_ref().is_some_and(|recipe|recipe.runtime_epoch!=inner.epoch.0)
        || model.engine_retired
        || operation.engine_incarnation != model.engine_incarnation
        || !plan.target_is_live(model)
    {
        let transition = EffectTransition {
            effect_id,
            from: effect.state,
            to: EffectState::Cancelled,
            resource_generation: effect.resource_generation,
            attempt: None,
            claim: None,
            result: Some(b"target disappeared before preparation started".to_vec()),
        };
        let original_command = operation.command_id.clone();
        drop(inner);
        return cancel_unstarted(executor, stream, id, original_command, transition);
    }
    let epoch = inner.epoch;
    inner
        .store
        .transition_effect(
            epoch,
            &EffectTransition {
                effect_id: effect_id.clone(),
                from: effect.state,
                to: EffectState::Running,
                resource_generation: effect.resource_generation,
                attempt: None,
                claim: Some(ActivationClaim {
                    engine_id: stream.into(),
                    surface_id: Some(plan.surface.id.to_string()),
                    runtime_epoch: epoch.0,
                    activation_generation: operation.activation_generation,
                }.into()),
                result: None,
            },
        )
        .map_err(|error| error.to_string())?;
    let record = inner
        .store
        .effect(&effect_id)
        .map_err(|error| error.to_string())?
        .ok_or("claimed effect missing")?;
    Ok(ResultValue::Claimed(ClaimedPreparation {
        lease: EffectLease {
            effect_id,
            operation: id.clone(),
            stream: stream.into(),
            runtime_epoch: epoch.0,
            resource_generation: record.resource_generation,
            attempt: record.attempt,
        },
        input,
        plan,
        capture,
        engine_incarnation: operation.engine_incarnation,
    }))
}

pub(super) fn prepared(
    executor: &Executor<StructureDecider>,
    lease: EffectLease,
    result: PreparationResult,
) -> Result<ResultValue> {
    let command = StructuralCommand::FinishCreation {
        operation: lease.operation.clone(),
        result,
    };
    finish(executor, lease, command, "prepared", None)
}

pub(super) fn rejected(
    executor: &Executor<StructureDecider>,
    lease: EffectLease,
    reason: String,
) -> Result<ResultValue> {
    let command = StructuralCommand::RejectInstallation {
        operation: lease.operation.clone(),
        reason,
    };
    finish(executor, lease, command, "installation-rejected", None)
}

pub(super) fn uncertain(executor:&Executor<StructureDecider>,lease:EffectLease,reason:String)->Result<ResultValue> {
    let command=StructuralCommand::MarkPreparationUncertain {operation:lease.operation.clone(),reason};
    finish(executor,lease,command,"uncertain",None)
}

pub(super) fn cleaned(
    executor: &Executor<StructureDecider>,
    lease: EffectLease,
    view:super::super::CompletionView,
) -> Result<ResultValue> {
    let command = StructuralCommand::FinishCleanup {
        operation: lease.operation.clone(),
    };
    finish(executor, lease, command, "cleaned", Some(view))
}

fn finish(
    executor: &Executor<StructureDecider>,
    lease: EffectLease,
    command: StructuralCommand,
    phase: &str,
    completion_view: Option<super::super::CompletionView>,
) -> Result<ResultValue> {
    let key = CommandKey {
        caller_scope: "journal-effect-result".into(),
        idempotency_key: format!("{}/{}/{phase}", lease.effect_id, lease.attempt),
    };
    let digest = serde_json::to_vec(&(&lease, &command, &completion_view))
        .map_err(|error| error.to_string())?;
    executor
        .with_state(|_| ())
        .map_err(|error| error.to_string())?;
    let (causation, original_results) = {
        let inner = executor.inner.lock().map_err(|error| error.to_string())?;
        if inner.epoch.0 != lease.runtime_epoch {
            return Err("effect result belongs to an earlier runtime".into());
        }
        // Stored result lookup precedes resolving the operation again.
        match inner
            .store
            .lookup_command(&key, &digest)
            .map_err(|error| error.to_string())?
        {
            CommandLookup::Hit(record) => return Ok(ResultValue::Stored(record)),
            CommandLookup::DigestMismatch(_) => {
                return Err("effect result key conflicts with its original payload".into());
            }
            CommandLookup::Miss => {}
        }
        let effect = inner
            .store
            .effect(&lease.effect_id)
            .map_err(|error| error.to_string())?
            .ok_or("effect missing")?;
        if effect.state != EffectState::Running
            || effect.attempt != lease.attempt
            || effect.resource_generation != lease.resource_generation
        {
            return Err("effect result is stale".into());
        }
        let operation = inner
            .state
            .streams
            .get(&lease.stream)
            .and_then(|model| model.operations.get(&lease.operation))
            .ok_or("effect operation missing")?;
        validate_binding(&effect, &lease.stream, operation)?;
        let command_id = operation.command_id.clone();
        let template = read_original_results(&inner.store, &command_id)?;
        (
            command_id.clone(),
            std::collections::BTreeMap::from([(command_id, template)]),
        )
    };
    let request = ExecuteRequest {
        key: Some(key),
        actor: "system".into(),
        origin: "effect-result".into(),
        causation_id: Some(causation),
        command: ResolvedCommand {
            original_digest: digest,
            response: None,
            changes: vec![StreamCommand {
                stream: lease.stream.clone(),
                command,
            }],
            effect_result: Some(lease),
            cancellation: None,
            completion_view,
            original_results,
        },
    };
    executor
        .execute(&request)
        .map(ResultValue::Executed)
        .map_err(|error| error.to_string())
}

fn cancel_unstarted(
    executor: &Executor<StructureDecider>,
    stream: &str,
    operation: &OperationId,
    original_command: String,
    transition: EffectTransition,
) -> Result<ResultValue> {
    let command = StructuralCommand::CancelUnstartedCreation {
        operation: operation.clone(),
        reason: "target disappeared before preparation started".into(),
    };
    let digest = serde_json::to_vec(&command).map_err(|error| error.to_string())?;
    let original_results = {
        let inner = executor.inner.lock().map_err(|error| error.to_string())?;
        std::collections::BTreeMap::from([(
            original_command.clone(),
            read_original_results(&inner.store, &original_command)?,
        )])
    };
    let request = ExecuteRequest {
        key: Some(CommandKey {
            caller_scope: "journal-unstarted-cancellation".into(),
            idempotency_key: operation.0.clone(),
        }),
        actor: "system".into(),
        origin: "effect-admission".into(),
        causation_id: Some(original_command),
        command: ResolvedCommand {
            original_digest: digest,
            response: None,
            changes: vec![StreamCommand {
                stream: stream.into(),
                command,
            }],
            effect_result: None,
            cancellation: Some(transition),
            completion_view: None,
            original_results,
        },
    };
    executor
        .execute(&request)
        .map(ResultValue::Executed)
        .map_err(|error| error.to_string())
}

pub(super) fn validate_binding(effect:&tasty_event_store::EffectRecord,stream:&str,operation:&tasty_domain::Operation)->Result<()> {
    use tasty_domain::StructuralEffect;
    if effect.operation_id!=operation.id.0 || effect.command_id.as_deref()!=Some(operation.command_id.as_str())
        || effect.payload.type_tag!="structure.surface_effect" || effect.payload.schema_version!=1 {
        return Err("effect does not belong to this operation and command".into());
    }
    let recorded:crate::runtime::journal_product::preparation::RecordedEffect=serde_json::from_slice(&effect.payload.bytes).map_err(|error|error.to_string())?;
    if recorded.stream!=stream {return Err("effect belongs to another engine stream".into());}
    let valid=match recorded.instruction {
        StructuralEffect::PrepareSurface {operation:id,input,surface,kind,activation_generation}=> {
            effect.claim_kind==tasty_event_store::ClaimKind::Activation && effect.effect_id==format!("{}/prepare",operation.id.0)
                && operation.creation.as_ref().is_some_and(|plan|id==operation.id && input==operation.input && surface==plan.surface.id && kind==plan.surface.kind && activation_generation==operation.activation_generation && effect.resource_generation==activation_generation)
        },
        StructuralEffect::ForwardStructure {operation:id,input}=>effect.claim_kind==tasty_event_store::ClaimKind::Obligation && effect.effect_id==format!("{}/forward",operation.id.0) && id==operation.id && operation.forward && input==operation.input,
        StructuralEffect::RetireSurfaces {operation:id,plan}=> {
            effect.claim_kind==tasty_event_store::ClaimKind::Obligation && effect.effect_id==format!("{}/retire",operation.id.0)
                && id==operation.id && operation.retirement.as_ref()==Some(&plan)
        },
    };
    if valid {Ok(())} else {Err("effect payload differs from its exact operation binding".into())}
}

pub(super) fn claim_retirement(executor:&Executor<StructureDecider>,stream:&str,id:&OperationId)->Result<ResultValue> {
    executor.with_state(|_|()).map_err(|error|error.to_string())?;
    let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;
    let operation=inner.state.streams.get(stream).and_then(|model|model.operations.get(id)).ok_or("retirement operation missing")?.clone();
    if operation.outcome.is_some() {return Err("retirement operation already has an outcome".into());}
    let plan=operation.retirement.clone().ok_or("operation is not a retirement")?;
    if inner.state.streams.get(stream).is_some_and(|model|plan.surfaces.iter().any(|surface|model.surfaces.contains_key(&surface.id))) {
        return Err("retirement tombstone has not removed every target".into());
    }
    let effect_id=format!("{}/retire",id.0);
    let effect=inner.store.effect(&effect_id).map_err(|error|error.to_string())?.ok_or("retirement effect missing")?;
    validate_binding(&effect,stream,&operation)?;
    if effect.state!=EffectState::Pending {return Err("retirement attempt requires explicit reconciliation before retry".into());}
    let epoch=inner.epoch;
    if inner.store.effect_origin_epoch(&effect_id).map_err(|error|error.to_string())?!=epoch {
        return Err("retirement belongs to an earlier runtime; physical owner reconciliation is required".into());
    }
    inner.store.transition_effect(epoch,&EffectTransition {
        effect_id:effect_id.clone(),from:EffectState::Pending,to:EffectState::Running,resource_generation:effect.resource_generation,attempt:None,
        claim:Some(tasty_event_store::ObligationClaim {engine_id:stream.into(),engine_incarnation:operation.engine_incarnation,operation_id:id.0.clone(),runtime_epoch:epoch.0}.into()),result:None,
    }).map_err(|error|error.to_string())?;
    let claimed=inner.store.effect(&effect_id).map_err(|error|error.to_string())?.ok_or("claimed retirement missing")?;
    Ok(ResultValue::RetirementClaimed(super::super::ClaimedRetirement {
        lease:EffectLease {effect_id,operation:id.clone(),stream:stream.into(),runtime_epoch:epoch.0,resource_generation:claimed.resource_generation,attempt:claimed.attempt},
        plan,engine_incarnation:operation.engine_incarnation,
    }))
}
pub(super) fn retired(executor:&Executor<StructureDecider>,lease:EffectLease,outcome:tasty_domain::OperationOutcome)->Result<ResultValue> {
    let command=StructuralCommand::FinishRetirement {operation:lease.operation.clone(),outcome};
    finish(executor,lease,command,"retired",None)
}

pub(super) fn read_original_results(
    store: &tasty_event_store::EventStore,
    command_id: &str,
) -> Result<super::super::response::OriginalResults> {
    let record = store
        .command(command_id)
        .map_err(|error| error.to_string())?
        .ok_or("original command record missing")?;
    super::super::response::OriginalResults::from_record(&record)
}


pub(super) fn claim_forward(executor:&Executor<StructureDecider>,stream:&str,id:&OperationId)->Result<ResultValue> {
    executor.with_state(|_|()).map_err(|error|error.to_string())?;
    let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;
    let model=inner.state.streams.get(stream).ok_or("forward engine missing")?;
    let operation=model.operations.get(id).filter(|operation|operation.forward && operation.outcome.is_none()).ok_or("forward operation missing")?.clone();
    if model.engine_retired || model.engine_incarnation!=operation.engine_incarnation {return Err("forward engine binding retired".into());}
    let effect_id=format!("{}/forward",id.0);let epoch=inner.epoch;
    let effect=inner.store.effect(&effect_id).map_err(|error|error.to_string())?.ok_or("forward effect missing")?;
    validate_binding(&effect,stream,&operation)?;
    if effect.state!=EffectState::Pending || inner.store.effect_origin_epoch(&effect_id).map_err(|error|error.to_string())?!=epoch {return Err("old remote submission requires reconciliation; it cannot be resent automatically".into());}
    let payload=inner.store.read_payload(PayloadRef(operation.input.0)).map_err(|error|error.to_string())?;
    inner.store.transition_effect(epoch,&EffectTransition {effect_id:effect_id.clone(),from:EffectState::Pending,to:EffectState::Running,resource_generation:effect.resource_generation,attempt:None,claim:Some(tasty_event_store::ObligationClaim {engine_id:stream.into(),engine_incarnation:operation.engine_incarnation,operation_id:id.0.clone(),runtime_epoch:epoch.0}.into()),result:None}).map_err(|error|error.to_string())?;
    let claimed=inner.store.effect(&effect_id).map_err(|error|error.to_string())?.ok_or("claimed forward missing")?;
    Ok(ResultValue::ForwardClaimed {lease:EffectLease {effect_id,operation:id.clone(),stream:stream.into(),runtime_epoch:epoch.0,resource_generation:claimed.resource_generation,attempt:claimed.attempt},payload})
}
pub(super) fn forwarded(executor:&Executor<StructureDecider>,lease:EffectLease,outcome:tasty_domain::OperationOutcome)->Result<ResultValue> {
    finish(executor,lease.clone(),StructuralCommand::FinishForward {operation:lease.operation,outcome},"forwarded",None)
}
