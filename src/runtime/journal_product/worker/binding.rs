use super::*;
use crate::runtime::journal_product::{BoundEngine, EngineBinding, EngineSelection, StreamCommand};
use tasty_domain::StructuralCommand;
use tasty_event_store::{CommandKey, CommandLookup};

pub(super) fn open(
    executor: &Executor<StructureDecider>,
    ticket: u64,
    selection: EngineSelection,
    normal_category_name: String,
) -> Result<ResultValue, String> {
    executor
        .with_state(|_| ())
        .map_err(|error| error.to_string())?;
    let digest = serde_json::to_vec(&(&selection, &normal_category_name))
        .map_err(|error| error.to_string())?;
    let (key, stream, previous, reset) = {
        let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
        let epoch = inner.epoch;
        let key = CommandKey {
            caller_scope: "engine-bootstrap".into(),
            idempotency_key: format!("{}/{ticket}", epoch.0),
        };
        match inner
            .store
            .lookup_command(&key, &digest)
            .map_err(|error| error.to_string())?
        {
            CommandLookup::Hit(record) => {
                let changes: Vec<StreamCommand> =
                    serde_json::from_slice(&record.resolved).map_err(|error| error.to_string())?;
                let stream = &changes
                    .first()
                    .ok_or("bootstrap command has no engine")?
                    .stream;
                return Ok(bound(
                    &inner.store,
                    epoch.0,
                    stream,
                    inner.state.stream(stream),
                ));
            }
            CommandLookup::DigestMismatch(_) => {
                return Err("bootstrap ticket was reused for another engine selection".into());
            }
            CommandLookup::Miss => {}
        }
        let (stream, reset) = match selection {
            EngineSelection::Slot { slot, resume } => (format!("structure:slot-{slot}"), !resume),
            EngineSelection::FreshHeadless => {
                let reserved = inner
                    .store
                    .reserve_ids(epoch, "engine", 1, u64::MAX)
                    .map_err(|error| error.to_string())?;
                (format!("structure:headless-{}", reserved.start), true)
            }
        };
        let previous = inner
            .state
            .streams
            .get(&stream)
            .map_or(0, |model| model.engine_incarnation);
        (key, stream, previous, reset)
    };
    executor
        .execute(&ExecuteRequest {
            key: Some(key),
            actor: "system".into(),
            origin: "engine-bootstrap".into(),
            causation_id: None,
            command: ResolvedCommand {
                original_digest: digest,
                changes: vec![StreamCommand {
                    stream: stream.clone(),
                    command: StructuralCommand::OpenEngine {
                        expected_incarnation: previous,
                        reset_structure: reset,
                        normal_category_name,
                    },
                }],
                effect_result: None,
                cancellation: None,
                original_results: Default::default(),
            },
        })
        .map_err(|error| error.to_string())?;
    let inner = executor.inner.lock().map_err(|error| error.to_string())?;
    Ok(bound(
        &inner.store,
        inner.epoch.0,
        &stream,
        inner.state.stream(&stream),
    ))
}

fn bound(
    store: &tasty_event_store::EventStore,
    epoch: u64,
    stream: &str,
    model: tasty_domain::JournalModel,
) -> ResultValue {
    ResultValue::Bound(BoundEngine {
        binding: EngineBinding {
            journal_id: store.journal_id().into(),
            stream: stream.into(),
            incarnation: model.engine_incarnation,
            runtime_epoch: epoch,
        },
        model,
    })
}
