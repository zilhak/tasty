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
        tasty_domain::CreationDestination::Restore { .. }
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
    if operation.engine_incarnation != model.engine_incarnation || !plan.target_is_live(model) {
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
                }),
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
    finish(executor, lease, command, "prepared")
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
    finish(executor, lease, command, "installation-rejected")
}

pub(super) fn cleaned(
    executor: &Executor<StructureDecider>,
    lease: EffectLease,
) -> Result<ResultValue> {
    let command = StructuralCommand::FinishCleanup {
        operation: lease.operation.clone(),
    };
    finish(executor, lease, command, "cleaned")
}

fn finish(
    executor: &Executor<StructureDecider>,
    lease: EffectLease,
    command: StructuralCommand,
    phase: &str,
) -> Result<ResultValue> {
    let key = CommandKey {
        caller_scope: "journal-effect-result".into(),
        idempotency_key: format!("{}/{}/{phase}", lease.effect_id, lease.attempt),
    };
    let digest = serde_json::to_vec(&(&lease, &command)).map_err(|error| error.to_string())?;
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
            changes: vec![StreamCommand {
                stream: lease.stream.clone(),
                command,
            }],
            effect_result: Some(lease),
            cancellation: None,
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
            changes: vec![StreamCommand {
                stream: stream.into(),
                command,
            }],
            effect_result: None,
            cancellation: Some(transition),
            original_results,
        },
    };
    executor
        .execute(&request)
        .map(ResultValue::Executed)
        .map_err(|error| error.to_string())
}

fn validate_binding(
    effect: &tasty_event_store::EffectRecord,
    stream: &str,
    operation: &tasty_domain::Operation,
) -> Result<()> {
    if effect.operation_id != operation.id.0
        || effect.command_id.as_deref() != Some(operation.command_id.as_str())
        || effect.effect_id != format!("{}/prepare", operation.id.0)
        || effect.payload.type_tag != "structure.surface_effect"
        || effect.payload.schema_version != 1
    {
        return Err("effect does not belong to this operation and command".into());
    }
    let recorded: crate::runtime::journal_product::preparation::RecordedEffect =
        serde_json::from_slice(&effect.payload.bytes).map_err(|error| error.to_string())?;
    let tasty_domain::StructuralEffect::PrepareSurface {
        operation: id,
        input,
        surface,
        kind,
        activation_generation,
    } = recorded.instruction;
    let plan = operation
        .creation
        .as_ref()
        .ok_or("effect operation has no creation plan")?;
    if recorded.stream != stream
        || id != operation.id
        || input != operation.input
        || surface != plan.surface.id
        || kind != plan.surface.kind
        || activation_generation != operation.activation_generation
        || effect.resource_generation != activation_generation
    {
        return Err(
            "effect payload binding does not match the operation, stream, and surface".into(),
        );
    }
    Ok(())
}

fn read_original_results(
    store: &tasty_event_store::EventStore,
    command_id: &str,
) -> Result<Vec<tasty_domain::StructuralResult>> {
    let record = store
        .command(command_id)
        .map_err(|error| error.to_string())?
        .ok_or("original command record missing")?;
    let response = record
        .response
        .ok_or("original command progress results missing")?;
    serde_json::from_slice(&response).map_err(|error| error.to_string())
}
