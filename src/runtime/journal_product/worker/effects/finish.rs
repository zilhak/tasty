//! The same pure completion builder reads committed or privately staged records.
use super::*;
use crate::runtime::command_executor::GroupCommand;
use tasty_event_store::{
    CommandRecord, CommitGroup, EffectRecord, EventStore, StoreResult, WriterEpoch,
};

pub(crate) trait CompletionStore {
    fn epoch(&self) -> WriterEpoch;
    fn lookup(&self, key: &CommandKey, digest: &[u8]) -> StoreResult<CommandLookup>;
    fn effect(&self, id: &str) -> StoreResult<Option<EffectRecord>>;
    fn command(&self, id: &str) -> StoreResult<Option<CommandRecord>>;
}

pub(super) struct Committed<'a>(pub &'a EventStore, pub WriterEpoch);
impl CompletionStore for Committed<'_> {
    fn epoch(&self) -> WriterEpoch {
        self.1
    }
    fn lookup(&self, key: &CommandKey, digest: &[u8]) -> StoreResult<CommandLookup> {
        self.0.lookup_command(key, digest)
    }
    fn effect(&self, id: &str) -> StoreResult<Option<EffectRecord>> {
        self.0.effect(id)
    }
    fn command(&self, id: &str) -> StoreResult<Option<CommandRecord>> {
        self.0.command(id)
    }
}
impl CompletionStore for CommitGroup<'_> {
    fn epoch(&self) -> WriterEpoch {
        self.writer_epoch()
    }
    fn lookup(&self, key: &CommandKey, digest: &[u8]) -> StoreResult<CommandLookup> {
        self.lookup_command(key, digest)
    }
    fn effect(&self, id: &str) -> StoreResult<Option<EffectRecord>> {
        CommitGroup::effect(self, id)
    }
    fn command(&self, id: &str) -> StoreResult<Option<CommandRecord>> {
        CommitGroup::command(self, id)
    }
}

pub(crate) fn prepare(
    store: &impl CompletionStore,
    state: &tasty_core::StructureModels,
    lease: EffectLease,
    command: StructuralCommand,
    phase: &str,
    completion_view: Option<super::super::super::CompletionView>,
) -> Result<GroupCommand<ResolvedCommand>> {
    let key = CommandKey {
        caller_scope: "journal-effect-result".into(),
        idempotency_key: format!("{}/{}/{phase}", lease.effect_id, lease.attempt),
    };
    let digest =
        serde_json::to_vec(&(&lease, &command, &completion_view)).map_err(|e| e.to_string())?;
    if store.epoch().0 != lease.runtime_epoch {
        return Err("effect result belongs to an earlier runtime".into());
    }
    match store.lookup(&key, &digest).map_err(|e| e.to_string())? {
        CommandLookup::Hit(record) => return Ok(GroupCommand::Stored(record)),
        CommandLookup::DigestMismatch(_) => {
            return Err("effect result key conflicts with its original payload".into());
        }
        CommandLookup::Miss => {}
    }
    let effect = store
        .effect(&lease.effect_id)
        .map_err(|e| e.to_string())?
        .ok_or("effect missing")?;
    if effect.state != EffectState::Running
        || effect.attempt != lease.attempt
        || effect.resource_generation != lease.resource_generation
    {
        return Err("effect result is stale".into());
    }
    let operation = state
        .streams
        .get(&lease.stream)
        .and_then(|model| model.operations.get(&lease.operation))
        .ok_or("effect operation missing")?;
    validate_binding(&effect, &lease.stream, operation)?;
    let causation = operation.command_id.clone();
    let record = store
        .command(&causation)
        .map_err(|e| e.to_string())?
        .ok_or("original command record missing")?;
    let template = super::super::super::response::OriginalResults::from_record(&record)?;
    Ok(GroupCommand::Ready(ExecuteRequest {
        key: Some(key),
        actor: "system".into(),
        origin: "effect-result".into(),
        causation_id: Some(causation.clone()),
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
            original_results: std::collections::BTreeMap::from([(causation, template)]),
        },
    }))
}

#[cfg(test)]
mod read_tests;
