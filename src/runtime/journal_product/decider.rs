//! Typed structural commands are decided without access to the store or live resources.

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
    pub(crate) changes: Vec<StreamCommand>,
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
        _context: &mut DecisionContext<'_>,
    ) -> Result<Decision<StreamEvent, NewEffect>, Rejection> {
        let mut candidate = state.clone();
        let mut events = Vec::new();
        let mut results: Vec<StructuralResult> = Vec::new();
        for change in &command.changes {
            if !tasty_domain::is_structure_stream(&change.stream) {
                return Err(Rejection("not an engine structure stream".into()));
            }
            let model = candidate.streams.entry(change.stream.clone()).or_default();
            let decision = decide_structure(model, &change.command)?;
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
            effects: Vec::new(),
            resolved: serde_json::to_vec(&command.changes).map_err(|e| Rejection(e.to_string()))?,
            response: serde_json::to_vec(&results).map_err(|e| Rejection(e.to_string()))?,
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
        _command: &ResolvedCommand,
        decision: &Decision<StreamEvent, NewEffect>,
    ) -> CommandRecordPlan {
        CommandRecordPlan {
            status: CommandStatus::Completed,
            response: Some(decision.response.clone()),
            command_updates: Vec::new(),
            effect_transitions: Vec::new(),
        }
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
