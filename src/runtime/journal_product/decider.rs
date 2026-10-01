//! Typed structural commands are decided without access to the store or live resources.

mod completion;

use serde::{Deserialize, Serialize};
use tasty_domain::{
    Decider, Decision, DecisionContext, DomainEvent, Rejection, StructuralCommand,
    StructuralResult, StructureModels, decide_structure,
};
use tasty_event_store::{
    CommandStatus, EventStore, NewEffect, OpaquePayload, PayloadRef, StoredBatch, StreamId,
};

use crate::runtime::command_executor::{CommandRecordPlan, JournalDecider};
use crate::runtime::journal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StreamCommand {
    pub(crate) stream: String,
    pub(crate) command: StructuralCommand,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCommand {
    /// Caller method and original arguments, before implicit target resolution.
    pub(crate) original_digest: Vec<u8>,
    pub(crate) response: Option<super::ResponsePlan>,
    pub(crate) changes: Vec<StreamCommand>,
    pub(crate) effect_result: Option<super::EffectLease>,
    pub(crate) cancellation: Option<tasty_event_store::EffectTransition>,
    pub(crate) completion_mirrors: Option<usize>,
    /// Result templates read before deciding a completion, never fetched from storage inside decide.
    pub(crate) original_results:
        std::collections::BTreeMap<String, super::response::OriginalResults>,
}

#[derive(Debug, Clone)]
pub(crate) struct StreamEvent {
    pub(crate) stream: StreamId,
    pub(crate) event: DomainEvent,
}

pub(crate) struct StructureDecider;

impl Decider for StructureDecider {
    type State = StructureModels;
    type Command = ResolvedCommand;
    type Event = StreamEvent;
    type Effect = NewEffect;
    type Rejection = Rejection;

    fn request_digest(&self, command: &ResolvedCommand) -> Vec<u8> {
        command.original_digest.clone()
    }

    fn decide(
        &self,
        state: &StructureModels,
        command: &ResolvedCommand,
        context: &mut DecisionContext<'_>,
    ) -> Result<Decision<StreamEvent, NewEffect>, Rejection> {
        match self.decide_changes(state, command, context) {
            Err(error) if command.response.is_some() => Ok(Decision {
                events: Vec::new(),
                effects: Vec::new(),
                resolved: encoded_resolution(
                    &command.changes,
                    &command.response,
                    command.completion_mirrors,
                )?,
                response: super::ResponsePlan::rejected(&error),
            }),
            result => result,
        }
    }
}

impl StructureDecider {
    fn decide_changes(
        &self,
        state: &StructureModels,
        command: &ResolvedCommand,
        context: &mut DecisionContext<'_>,
    ) -> Result<Decision<StreamEvent, NewEffect>, Rejection> {
        let mut candidate = state.clone();
        let mut events = Vec::new();
        let mut results: Vec<StructuralResult> = Vec::new();
        let mut effects = Vec::new();
        let mut resolved_changes = Vec::new();
        for (index, change) in command.changes.iter().enumerate() {
            if !tasty_domain::is_structure_stream(&change.stream) {
                return Err(Rejection("not an engine structure stream".into()));
            }
            let model = candidate.streams.entry(change.stream.clone()).or_default();
            let mut resolved = change.command.clone();
            if let StructuralCommand::PrepareCreation {
                operation,
                command_id,
                ..
            } = &mut resolved
            {
                *command_id = context.command_id.to_owned();
                *operation =
                    tasty_domain::OperationId(format!("{}/prepare/{index}", context.command_id));
            }
            let decision = decide_structure(model, &resolved)?;
            resolved_changes.push(StreamCommand {
                stream: change.stream.clone(),
                command: resolved,
            });
            effects.extend(
                decision
                    .effects
                    .iter()
                    .map(|effect| new_effect(&change.stream, effect))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let revision = model.applied.revision.unwrap_or(0);
            let batch_id = model
                .applied
                .batch
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Rejection("journal batch range exhausted".into()))?;
            let batch = tasty_domain::DomainBatch {
                batch_id,
                events: decision
                    .events
                    .iter()
                    .enumerate()
                    .map(|(index, event)| {
                        Ok(tasty_domain::RecordedEvent {
                            revision: revision.checked_add(index as u64 + 1).ok_or_else(|| {
                                Rejection("structure revision range exhausted".into())
                            })?,
                            event: event.clone(),
                        })
                    })
                    .collect::<Result<_, Rejection>>()?,
            };
            tasty_domain::evolve(model, &batch).map_err(|e| Rejection(e.to_string()))?;
            events.extend(decision.events.into_iter().map(|event| StreamEvent {
                stream: StreamId::new(&change.stream),
                event,
            }));
            results.push(decision.result);
        }
        Ok(Decision {
            events,
            effects,
            resolved: encoded_resolution(
                &resolved_changes,
                &command.response,
                command.completion_mirrors,
            )?,
            response: match &command.response {
                Some(response)
                    if results
                        .iter()
                        .any(|result| matches!(result, StructuralResult::Pending { .. })) =>
                {
                    let mut progress = super::response::ResponseProgress::new(results);
                    progress
                        .freeze(response, &candidate, 0)
                        .map_err(Rejection)?;
                    serde_json::to_vec(&progress).map_err(|error| Rejection(error.to_string()))?
                }
                Some(response) => response.render(&candidate, 0)?,
                None => serde_json::to_vec(&results).map_err(|e| Rejection(e.to_string()))?,
            },
        })
    }
}

impl JournalDecider for StructureDecider {
    fn event_stream(&self, event: &StreamEvent) -> StreamId {
        event.stream.clone()
    }

    fn stream_revision(&self, state: &StructureModels, stream: &StreamId) -> Option<u64> {
        state
            .streams
            .get(stream.as_str())
            .and_then(|model| model.applied.revision)
    }

    fn record(
        &self,
        state: &StructureModels,
        command: &ResolvedCommand,
        decision: &Decision<StreamEvent, NewEffect>,
    ) -> Result<CommandRecordPlan, String> {
        let command_updates = completion::aggregate(state, command, decision)?;
        let internal = command.changes.iter().all(|change| {
            matches!(
                change.command,
                StructuralCommand::FinishCreation { .. }
                    | StructuralCommand::FinishCleanup { .. }
                    | StructuralCommand::CancelUnstartedCreation { .. }
                    | StructuralCommand::RejectInstallation { .. }
                    | StructuralCommand::MarkPreparationUncertain { .. }
            )
        });
        let pending = !internal && !decision.effects.is_empty();
        let failed = !pending
            && command.response.is_some()
            && serde_json::from_slice::<tasty_ipc::protocol::JsonRpcResponse>(&decision.response)
                .expect("public structural response serializes")
                .error
                .is_some();
        Ok(CommandRecordPlan {
            status: if pending {
                CommandStatus::InProgress
            } else if failed {
                CommandStatus::Failed
            } else {
                CommandStatus::Completed
            },
            response: Some(decision.response.clone()),
            command_updates,
            effect_transitions: command
                .effect_result
                .as_ref()
                .into_iter()
                .filter_map(|lease| {
                    let outcome = decision
                        .events
                        .iter()
                        .find_map(|event| match &event.event {
                            DomainEvent::OperationFinished { id, outcome }
                                if *id == lease.operation =>
                            {
                                Some(outcome)
                            }
                            _ => None,
                        })?;
                    let to = match outcome {
                        tasty_domain::OperationOutcome::Failed { .. } => {
                            tasty_event_store::EffectState::Failed
                        }
                        tasty_domain::OperationOutcome::Uncertain { .. } => {
                            tasty_event_store::EffectState::Uncertain
                        }
                        _ => tasty_event_store::EffectState::Succeeded,
                    };
                    Some(tasty_event_store::EffectTransition {
                        effect_id: lease.effect_id.clone(),
                        from: tasty_event_store::EffectState::Running,
                        to,
                        resource_generation: lease.resource_generation,
                        attempt: Some(lease.attempt),
                        claim: None,
                        result: Some(decision.response.clone()),
                    })
                })
                .chain(command.cancellation.iter().cloned())
                .collect(),
        })
    }

    fn encode(&self, event: &StreamEvent) -> Result<OpaquePayload, String> {
        journal::to_payload(&event.event).map_err(|e| e.to_string())
    }

    fn payload_refs(&self, event: &StreamEvent) -> Vec<PayloadRef> {
        event
            .event
            .data_refs()
            .into_iter()
            .map(|reference| PayloadRef(reference.0))
            .collect()
    }

    fn apply(&self, state: &mut StructureModels, batch: &StoredBatch) -> Result<(), String> {
        journal::apply_all(state, batch).map_err(|e| e.to_string())
    }

    fn load(&self, store: &EventStore) -> Result<StructureModels, String> {
        journal::load_all(store).map_err(|e| e.to_string())
    }
}

fn new_effect(
    stream: &str,
    effect: &tasty_domain::StructuralEffect,
) -> Result<NewEffect, Rejection> {
    use tasty_domain::StructuralEffect;
    let (operation, generation, step) = match effect {
        StructuralEffect::PrepareSurface {
            operation,
            activation_generation,
            ..
        } => (operation, *activation_generation, "prepare"),
    };
    Ok(NewEffect {
        effect_id: format!("{}/{}", operation.0, step),
        operation_id: operation.0.clone(),
        // This is the durable activation obligation, not the process-local Pty generation.
        resource_generation: generation,
        payload: OpaquePayload {
            type_tag: "structure.surface_effect".into(),
            schema_version: 1,
            bytes: serde_json::to_vec(&super::preparation::RecordedEffect {
                stream: stream.into(),
                instruction: effect.clone(),
            })
            .map_err(|error| Rejection(error.to_string()))?,
        },
        initial: tasty_event_store::EffectState::Pending,
    })
}

fn encoded_resolution(
    changes: &[StreamCommand],
    response: &Option<super::ResponsePlan>,
    completion_mirrors: Option<usize>,
) -> Result<Vec<u8>, Rejection> {
    if response.is_none() && completion_mirrors.is_none() {
        return serde_json::to_vec(changes).map_err(|error| Rejection(error.to_string()));
    }
    serde_json::to_vec(&super::response::RecordedResolution {
        version: 1,
        changes: changes.to_vec(),
        response: response.clone(),
        completion_mirrors,
    })
    .map_err(|error| Rejection(error.to_string()))
}
