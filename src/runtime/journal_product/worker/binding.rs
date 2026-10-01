use super::*;
use crate::runtime::journal_product::{BoundEngine, EngineBinding, EngineSelection, StreamCommand};
use tasty_core::StructuralCommand;
use tasty_event_store::{CommandKey, CommandLookup};

pub(super) fn open(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    ticket: u64,
    selection: EngineSelection,
    normal_category_name: String,
    surface_floor: u32,
) -> Result<ResultValue, String> {
    executor
        .with_state(|_| ())
        .map_err(|error| error.to_string())?;
    let resume_view = matches!(&selection, EngineSelection::Slot { resume: true, .. } | EngineSelection::ImportedSlot { .. });
    let digest = serde_json::to_vec(&(&selection, &normal_category_name, surface_floor))
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
                return bound(
                    &inner.store,
                    home,
                    epoch.0,
                    stream,
                    inner.state.stream(stream),
                    resume_view,
                );
            }
            CommandLookup::DigestMismatch(_) => {
                return Err("bootstrap ticket was reused for another engine selection".into());
            }
            CommandLookup::Miss => {}
        }
        let next = inner
            .store
            .next_unreserved_id("surface")
            .map_err(|error| error.to_string())?;
        if next <= u64::from(surface_floor) {
            let count = u64::from(surface_floor) + 1 - next;
            inner
                .store
                .reserve_ids(
                    epoch,
                    "surface",
                    count,
                    u64::from(crate::runtime::terminal_store::PTY_ID_BASE - 1),
                )
                .map_err(|error| error.to_string())?;
        }
        if let EngineSelection::ImportedSlot { source } = &selection {
            crate::runtime::journal_payload::import::transfer(&mut inner.store, epoch, source)?;
            drop(inner);
            executor.reload_committed().map_err(|error|error.to_string())?;
            inner = executor.inner.lock().map_err(|error|error.to_string())?;
        }
        if let EngineSelection::Slot { slot, resume: true } = selection {
            drop(inner);
            import_legacy(executor, home, slot)?;
            inner = executor.inner.lock().map_err(|error| error.to_string())?;
        }
        let (stream, reset) = match selection {
            EngineSelection::Slot { slot, resume } => (format!("structure:slot-{slot}"), !resume),
            EngineSelection::ImportedSlot { source } => (format!("structure:slot-{}", source.destination_slot), false),
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
                response: None,
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
                completion_view: None,
                original_results: Default::default(),
            },
        })
        .map_err(|error| error.to_string())?;
    let inner = executor.inner.lock().map_err(|error| error.to_string())?;
    bound(
        &inner.store,
        home,
        inner.epoch.0,
        &stream,
        inner.state.stream(&stream),
        resume_view,
    )
}

fn bound(
    store: &tasty_event_store::EventStore,
    home: &std::path::Path,
    epoch: u64,
    stream: &str,
    model: tasty_core::JournalModel,
    resume_view: bool,
) -> Result<ResultValue, String> {
    let binding = EngineBinding {
        journal_id: store.journal_id().into(),
        stream: stream.into(),
        incarnation: model.engine_incarnation,
        runtime_epoch: epoch,
        published_cut: model.applied.batch,
        revision: model.applied.revision,
    };
    let restored_view = if resume_view {
        match super::super::view_record::load(store, home, &binding)? {
            Some(view) => Some(view),
            None if model.engine_incarnation == 1 => imported_view(store, stream)?,
            None => None,
        }
    } else {
        None
    };
    Ok(ResultValue::Bound(BoundEngine {
        binding,
        imported_view: restored_view,
        model,
    }))
}

fn import_legacy(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    slot: u32,
) -> Result<(), String> {
    let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
    let stream = format!("structure:slot-{slot}");
    if inner
        .state
        .streams
        .get(&stream)
        .is_some_and(|model| model.applied.revision.is_some())
    {
        return Ok(());
    }
    let path = home.join("layouts").join(format!("{slot:02}.json"));
    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("legacy slot read {}: {error}", path.display())),
    };
    struct Scrollback<'a>(&'a std::path::Path);
    impl crate::core::layout_persistence::import::ScrollbackSource for Scrollback<'_> {
        fn read_bytes(&self, id: &str) -> std::io::Result<Option<Vec<u8>>> {
            crate::scrollback_store::read_bytes_from_home(self.0, id)
        }
    }
    let epoch = inner.epoch;
    crate::core::layout_persistence::import::import_slot(
        &mut inner.store,
        epoch,
        slot,
        &json,
        &Scrollback(home),
    )
    .map_err(|error| error.to_string())?;
    drop(inner);
    executor
        .reload_committed()
        .map_err(|error| error.to_string())
}

fn imported_view(
    store: &tasty_event_store::EventStore,
    stream: &str,
) -> Result<Option<crate::core::layout_persistence::import::ImportedView>, String> {
    let Some(slot) = stream
        .strip_prefix("structure:slot-")
        .and_then(|slot| slot.parse::<u32>().ok())
    else {
        return Ok(None);
    };
    let key = CommandKey {
        caller_scope: crate::core::layout_persistence::import::IMPORT_SCOPE.into(),
        idempotency_key: format!("slot-{slot}"),
    };
    let record = match store
        .lookup_command(&key, &[])
        .map_err(|error| error.to_string())?
    {
        CommandLookup::Hit(record) | CommandLookup::DigestMismatch(record) => record,
        CommandLookup::Miss => return Ok(None),
    };
    let Some(bytes) = record.response.filter(|bytes| !bytes.is_empty()) else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(super) fn retire(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    ticket: u64,
    binding: EngineBinding,
) -> Result<ResultValue, String> {
    executor
        .with_state(|_| ())
        .map_err(|error| error.to_string())?;
    {
        let inner = executor.inner.lock().map_err(|error| error.to_string())?;
        if inner.store.journal_id() != binding.journal_id || inner.epoch.0 != binding.runtime_epoch
        {
            return Err("retirement belongs to another journal runtime".into());
        }
    }
    let slot = binding
        .stream
        .strip_prefix("structure:slot-")
        .and_then(|slot| slot.parse::<u32>().ok());
    let digest = serde_json::to_vec(&binding).map_err(|error| error.to_string())?;
    let executed = executor
        .execute(&ExecuteRequest {
            key: Some(CommandKey {
                caller_scope: "engine-retirement".into(),
                idempotency_key: format!("{}/{ticket}", binding.runtime_epoch),
            }),
            actor: "system".into(),
            origin: "engine-retirement".into(),
            causation_id: None,
            command: ResolvedCommand {
                original_digest: digest,
                response: None,
                changes: vec![StreamCommand {
                    stream: binding.stream,
                    command: StructuralCommand::RetireEngine {
                        expected_incarnation: binding.incarnation,
                    },
                }],
                effect_result: None,
                cancellation: None,
                completion_view: None,
                original_results: Default::default(),
            },
        })
        .map_err(|error| error.to_string())?;
    // The legacy export is no longer a resume authority. Remove it only after durable retirement.
    if matches!(
        executed.source,
        crate::runtime::command_executor::Source::Committed { .. }
    ) && let Some(slot) = slot
    {
        let path = home.join("layouts").join(format!("{slot:02}.json"));
        if let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("retired slot export removal {}: {error}", path.display());
        }
    }
    Ok(ResultValue::Executed(executed))
}
