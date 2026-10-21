use super::*;
use crate::runtime::journal_product::{
    BindingIssue, BoundEngine, EngineBinding, EngineSelection, StreamCommand,
};
use tasty_core::{StructuralCommand, StructureModels};
use tasty_event_store::{CommandKey, CommandLookup};

/// surface ID 하한을 읽은 뒤 연다. 읽지 못하면 아무것도 기록하지 않고 이 opening만 거절한다.
pub(super) fn open_after_floor(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    ticket: u64,
    selection: EngineSelection,
    normal_category_name: String,
    surface_floor: &crate::runtime::journal_product::SurfaceFloor,
    outstanding_credits: u64,
) -> Result<ResultValue, String> {
    match surface_floor.read() {
        Ok(floor) => open(
            executor,
            home,
            ticket,
            selection,
            normal_category_name,
            floor,
            outstanding_credits,
        ),
        Err(reason) => Ok(ResultValue::OpeningRefused(reason)),
    }
}

fn open(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    ticket: u64,
    selection: EngineSelection,
    normal_category_name: String,
    surface_floor: u32,
    outstanding_credits: u64,
) -> Result<ResultValue, String> {
    executor
        .with_state(|_| ())
        .map_err(|error| error.to_string())?;
    let target = match &selection {
        EngineSelection::Slot { slot, .. } => Some(*slot),
        EngineSelection::ImportedSlot { source } => Some(source.destination_slot),
        EngineSelection::FreshHeadless => None,
    };
    if let Some(slot) = target
        && let Some(cause) = executor
            .decider()
            .report()
            .damaged
            .get(&format!("structure:slot-{slot}"))
    {
        // 이어 쓰면 읽지 못한 원본 기록 위에 새 기록이 쌓인다. 앱은 이 슬롯을 고르지 않는다.
        return Err(format!("layout slot {slot} has damaged records: {cause}"));
    }
    let digest = serde_json::to_vec(&(&selection, &normal_category_name, surface_floor))
        .map_err(|error| error.to_string())?;
    let mut import_issues = Vec::new();
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
                    &selection,
                    Vec::new(),
                );
            }
            CommandLookup::DigestMismatch(_) => {
                return Err("bootstrap ticket was reused for another engine selection".into());
            }
            CommandLookup::Miss => {}
        }
        // Existing stream resume/recovery remains possible under pressure. Fresh structure
        // (including legacy import) is new admission; same-key results were returned above.
        let fresh = match &selection {
            EngineSelection::Slot { slot, resume } => {
                !*resume
                    || !inner
                        .state
                        .streams
                        .contains_key(&format!("structure:slot-{slot}"))
            }
            EngineSelection::ImportedSlot { source } => !inner
                .state
                .streams
                .contains_key(&format!("structure:slot-{}", source.destination_slot)),
            EngineSelection::FreshHeadless => true,
        };
        if fresh {
            inner
                .store
                .ensure_new_admission(epoch, outstanding_credits)
                .map_err(|error| error.to_string())?;
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
            executor
                .reload_committed()
                .map_err(|error| error.to_string())?;
            inner = executor.inner.lock().map_err(|error| error.to_string())?;
        }
        if let EngineSelection::Slot { slot, resume: true } = &selection {
            drop(inner);
            import_issues = import_legacy(executor, home, *slot)?;
            inner = executor.inner.lock().map_err(|error| error.to_string())?;
        }
        let (stream, reset) = match &selection {
            EngineSelection::Slot { slot, resume } => (format!("structure:slot-{slot}"), !*resume),
            EngineSelection::ImportedSlot { source } => {
                (format!("structure:slot-{}", source.destination_slot), false)
            }
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
        &selection,
        import_issues,
    )
}

#[allow(clippy::too_many_arguments)] // reason: 두 호출부가 같은 결합 결과를 만드는 내부 도우미
fn bound(
    store: &tasty_event_store::EventStore,
    home: &std::path::Path,
    epoch: u64,
    stream: &str,
    model: tasty_core::JournalModel,
    selection: &EngineSelection,
    mut issues: Vec<BindingIssue>,
) -> Result<ResultValue, String> {
    let binding = EngineBinding {
        journal_id: store.journal_id().into(),
        stream: stream.into(),
        incarnation: model.engine_incarnation,
        runtime_epoch: epoch,
        published_cut: model.applied.batch,
        revision: model.applied.revision,
    };
    // The completed legacy import is durable provenance even after a View checkpoint.
    // A reset incarnation or an explicit cross-journal transfer is not legacy restoration.
    let legacy_view = if matches!(selection, EngineSelection::Slot { resume: true, .. })
        && model.engine_incarnation == 1
    {
        imported_view(store, stream)?
    } else {
        None
    };
    #[cfg(feature = "gui")]
    let restore_legacy_metadata = legacy_view.is_some();
    let restored_view = if matches!(
        selection,
        EngineSelection::Slot { resume: true, .. } | EngineSelection::ImportedSlot { .. }
    ) {
        let (view, issue) = super::super::view_record::load(store, home, &binding)?;
        // 읽지 못한 View checkpoint가 있으면 더 오래된 import 선택으로 돌아가지 않는다.
        let unreadable = issue.is_some();
        issues.extend(issue);
        if unreadable {
            None
        } else {
            view.or(legacy_view)
        }
    } else {
        None
    };
    Ok(ResultValue::Bound(BoundEngine {
        binding,
        imported_view: restored_view,
        issues,
        #[cfg(feature = "gui")]
        restore_legacy_metadata,
        model,
    }))
}

/// 슬롯 파일을 텍스트로 읽는다. 없으면 `None`, 읽거나 텍스트로 해석하지 못하면 그 사실이다.
fn read_legacy_slot(path: &std::path::Path) -> Result<Option<String>, BindingIssue> {
    let shown = path.display().to_string();
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|error| {
            tracing::warn!(
                path = shown,
                "legacy slot is not text; starting fresh: {error}"
            );
            BindingIssue::LegacyUnparsable {
                path: shown,
                cause: error.to_string(),
            }
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            tracing::error!(
                path = shown,
                "legacy slot could not be read; starting fresh: {error}"
            );
            Err(BindingIssue::LegacyUnreadable {
                path: shown,
                permission_denied: error.kind() == std::io::ErrorKind::PermissionDenied,
                cause: error.to_string(),
            })
        }
    }
}

/// 이전 형식 슬롯 파일을 처음 한 번 가져온다. 파일을 읽거나 해석하지 못하면 그 슬롯은 초기 상태로
/// 시작하고 사실만 돌려준다. 원본 파일과 scrollback 파일은 바꾸지 않는다. 초기 상태로 시작하면 그
/// stream에 기록이 생기므로 같은 파일을 다시 가져오지 않는다.
fn import_legacy(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    slot: u32,
) -> Result<Vec<BindingIssue>, String> {
    let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
    let stream = format!("structure:slot-{slot}");
    if inner
        .state
        .streams
        .get(&stream)
        .is_some_and(|model| model.applied.revision.is_some())
    {
        return Ok(Vec::new());
    }
    let path = home.join("layouts").join(format!("{slot:02}.json"));
    let shown = path.display().to_string();
    let json = match read_legacy_slot(&path) {
        Ok(Some(json)) => json,
        Ok(None) => return Ok(Vec::new()),
        Err(issue) => return Ok(vec![issue]),
    };
    struct Scrollback<'a>(&'a std::path::Path);
    impl crate::core::layout_persistence::import::ScrollbackSource for Scrollback<'_> {
        fn read_bytes(&self, id: &str) -> std::io::Result<Option<Vec<u8>>> {
            crate::scrollback_store::read_bytes_from_home(self.0, id)
        }
    }
    let epoch = inner.epoch;
    use crate::core::layout_persistence::import::ImportError;
    let outcome = match crate::core::layout_persistence::import::import_slot(
        &mut inner.store,
        epoch,
        slot,
        &json,
        &Scrollback(home),
    ) {
        Ok(outcome) => outcome,
        Err(error @ (ImportError::Unparsable | ImportError::UnsupportedVersion)) => {
            tracing::warn!(
                path = shown,
                "legacy slot is not a layout; starting fresh: {error}"
            );
            return Ok(vec![BindingIssue::LegacyUnparsable {
                path: shown,
                cause: error.to_string(),
            }]);
        }
        Err(error) => return Err(error.to_string()),
    };
    drop(inner);
    executor
        .reload_committed()
        .map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if !outcome.missing_scrollback.is_empty() {
        tracing::warn!(
            path = shown,
            count = outcome.missing_scrollback.len(),
            "imported terminals without their saved scrollback"
        );
        issues.push(BindingIssue::MissingScrollback {
            count: outcome.missing_scrollback.len(),
            path: shown,
        });
    }
    Ok(issues)
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

#[cfg(any(feature = "gui", test))]
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

/// Keep only models this work can publish. Unrelated and retired streams stay on the worker.
pub(super) fn publication_predecessor(models: &StructureModels, work: &Work) -> StructureModels {
    let streams: Vec<String> = match work {
        Work::Resolve { changes, .. } => {
            changes.iter().map(|change| change.stream.clone()).collect()
        }
        Work::Capture { binding, .. } => vec![binding.stream.clone()],
        #[cfg(any(feature = "gui", test))]
        Work::RetireEngine(binding) => vec![binding.stream.clone()],
        Work::ClaimPreparation { stream, .. } => vec![stream.clone()],
        Work::ReconcilePreparation { lease, .. }
        | Work::ReconcileRetirement { lease, .. }
        | Work::RetirementFinished { lease, .. }
        | Work::Prepared { lease, .. }
        | Work::CleanupFinished { lease, .. }
        | Work::InstallationRejected { lease, .. }
        | Work::PreparationUncertain { lease, .. } => vec![lease.stream.clone()],
        #[cfg(feature = "gui")]
        Work::ForwardFinished { lease, .. } => vec![lease.stream.clone()],
        Work::OpenEngine { selection, .. } => match selection {
            EngineSelection::Slot { slot, .. } => vec![format!("structure:slot-{slot}")],
            EngineSelection::ImportedSlot { source } => {
                vec![format!("structure:slot-{}", source.destination_slot)]
            }
            // The persistent allocator issues a previously unused stream.
            EngineSelection::FreshHeadless => Vec::new(),
        },
        _ => Vec::new(),
    };
    StructureModels {
        batch: models.batch,
        streams: streams
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|stream| {
                let model = models.stream(&stream);
                (stream, model)
            })
            .collect(),
    }
}
