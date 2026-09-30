mod binding;
mod effects;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use tasty_event_store::CommandLookup;

use super::decider::{ResolvedCommand, StructureDecider};
use super::{Admission, Completion, QUEUE_CAPACITY, Request, ResultValue, Work, identity};
use crate::runtime::command_executor::{Executor, Request as ExecuteRequest};
use crate::runtime::journal;

struct Pending {
    admission: Admission,
    followers: Vec<u64>,
    reservations: Vec<tasty_event_store::IdRange>,
    inputs: Vec<tasty_domain::DataRef>,
}

type Acknowledgements = mpsc::Receiver<(u64, Result<(), String>)>;

pub(super) fn run(
    home: PathBuf,
    requests: mpsc::Receiver<Request>,
    completions: mpsc::SyncSender<Completion>,
    acknowledgements: Acknowledgements,
    closed: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let send = |completion| {
        if completions.send(completion).is_err() {
            return false;
        }
        wake();
        true
    };
    let opened = identity::open(&home)
        .and_then(|store| Executor::open(StructureDecider, store).map_err(|e| e.to_string()));
    let executor = match opened {
        Ok(executor) => executor,
        Err(error) => {
            send(Completion::StartupFailed(error));
            return;
        }
    };
    let mut published = {
        let inner = executor.inner.lock().expect("new executor lock");
        let cut = inner.state.batch;
        if !send(Completion::Ready {
            journal_id: inner.store.journal_id().to_owned(),
            runtime_epoch: inner.epoch.0,
            cut,
            bootstrap: inner.state.clone(),
        }) {
            return;
        }
        cut
    };
    if let Err(error) = acknowledge(&acknowledgements, published.unwrap_or(0)) {
        send(Completion::StartupFailed(error));
        return;
    }
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut halted: Option<String> = None;
    while let Ok(request) = requests.recv() {
        if closed.load(Ordering::Acquire) {
            break;
        }
        let mut predecessor = if matches!(
            request.work,
            Work::OpenEngine { .. }
                | Work::Resolve(_)
                | Work::Prepared { .. }
                | Work::CleanupFinished { .. }
                | Work::InstallationRejected { .. }
                | Work::ClaimPreparation { .. }
        ) && halted.is_none()
        {
            match executor.with_state(Clone::clone) {
                Ok(models) => Some(models),
                Err(error) => {
                    halted = Some(error.to_string());
                    None
                }
            }
        } else {
            None
        };
        let followers = if matches!(request.work, Work::Resolve(_) | Work::CancelAdmission) {
            pending
                .get(&request.ticket)
                .map(|p| p.followers.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut result = match &halted {
            Some(reason) => Err(reason.clone()),
            None => handle(&executor, &home, &mut pending, request.ticket, request.work),
        };
        if halted.is_none()
            && let Err(error) = publish(
                &executor,
                &mut published,
                &mut predecessor,
                &acknowledgements,
                match &result {
                    Ok(ResultValue::Bound(bound)) => Some(bound.binding.clone()),
                    _ => None,
                },
                &send,
            )
        {
            halted = Some(error.clone());
            result = Err(error);
        }
        for ticket in std::iter::once(request.ticket).chain(followers) {
            if !send(Completion::Finished {
                ticket,
                result: result.clone(),
            }) {
                return;
            }
        }
    }
}

fn handle(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    pending: &mut HashMap<u64, Pending>,
    ticket: u64,
    work: Work,
) -> Result<ResultValue, String> {
    match work {
        Work::OpenEngine {
            selection,
            normal_category_name,
            surface_floor,
        } => binding::open(
            executor,
            home,
            ticket,
            selection,
            normal_category_name,
            surface_floor,
        ),
        Work::Admit(admission) => {
            if pending.contains_key(&ticket)
                || pending.values().any(|p| p.followers.contains(&ticket))
            {
                return Err("journal admission ticket already exists".into());
            }
            if pending
                .values()
                .map(|p| 1 + p.followers.len())
                .sum::<usize>()
                >= QUEUE_CAPACITY
            {
                return Err("journal admission capacity exhausted".into());
            }
            if let Some(key) = &admission.key {
                for (leader, existing) in pending.iter_mut() {
                    if existing.admission.key.as_ref() == Some(key) {
                        if existing.admission.original_digest != admission.original_digest {
                            return Err("idempotency key belongs to a different request".into());
                        }
                        existing.followers.push(ticket);
                        return Ok(ResultValue::JoinedAdmission {
                            leader_ticket: *leader,
                        });
                    }
                }
            }
            // Recover before exposing any stored success; lookup still precedes target resolution.
            executor.with_state(|_| ()).map_err(|e| e.to_string())?;
            if let Some(key) = &admission.key {
                let inner = executor.inner.lock().map_err(|e| e.to_string())?;
                match inner
                    .store
                    .lookup_command(key, &admission.original_digest)
                    .map_err(|e| e.to_string())?
                {
                    CommandLookup::Hit(record) => return Ok(ResultValue::Stored(record)),
                    CommandLookup::DigestMismatch(_) => {
                        return Err("idempotency key belongs to a different request".into());
                    }
                    CommandLookup::Miss => {}
                }
            }
            if pending.len() >= QUEUE_CAPACITY {
                return Err("journal admission capacity exhausted".into());
            }
            pending.insert(
                ticket,
                Pending {
                    admission,
                    followers: Vec::new(),
                    reservations: Vec::new(),
                    inputs: Vec::new(),
                },
            );
            Ok(ResultValue::NeedsResolution)
        }
        Work::Resolve(changes) => {
            let admitted = pending
                .remove(&ticket)
                .ok_or("journal request was not admitted")?;
            for change in &changes {
                if matches!(
                    change.command,
                    tasty_domain::StructuralCommand::OpenEngine { .. }
                        | tasty_domain::StructuralCommand::FinishCreation { .. }
                        | tasty_domain::StructuralCommand::FinishCleanup { .. }
                        | tasty_domain::StructuralCommand::CancelUnstartedCreation { .. }
                        | tasty_domain::StructuralCommand::RejectInstallation { .. }
                ) {
                    return Err("effect results require their validated lease endpoint".into());
                }
                if let tasty_domain::StructuralCommand::PrepareCreation { input, .. } =
                    &change.command
                    && !admitted.inputs.contains(input)
                {
                    return Err("preparation input belongs to another admission".into());
                }
                for required in change.command.reserved_ids() {
                    if !admitted.reservations.iter().any(|range| {
                        range.kind == required.kind.label()
                            && range.contains(u64::from(required.id))
                    }) {
                        return Err(
                            "structural command uses an ID not reserved for its admission".into(),
                        );
                    }
                }
            }
            let admission = admitted.admission;
            let executed = executor
                .execute(&ExecuteRequest {
                    key: admission.key,
                    actor: admission.actor,
                    origin: admission.origin,
                    causation_id: admission.causation_id,
                    command: ResolvedCommand {
                        original_digest: admission.original_digest,
                        changes,
                        effect_result: None,
                        cancellation: None,
                        original_results: Default::default(),
                    },
                })
                .map_err(|e| e.to_string())?;
            Ok(ResultValue::Executed(executed))
        }
        Work::Reserve(kinds) => {
            if !pending.contains_key(&ticket) {
                return Err("ID reservation requires an admitted request".into());
            }
            if kinds.len() > 5 || kinds.iter().any(|(_, count)| *count == 0 || *count > 4096) {
                return Err("invalid structure ID reservation size".into());
            }
            let mut inner = executor.inner.lock().map_err(|e| e.to_string())?;
            let epoch = inner.epoch;
            let mut ranges = Vec::new();
            for (kind, count) in kinds {
                let max = if kind == tasty_domain::IdKind::Surface {
                    0x7fff_ffff
                } else {
                    u32::MAX
                };
                let range = inner
                    .store
                    .reserve_ids(epoch, kind.label(), u64::from(count), u64::from(max))
                    .map_err(|e| e.to_string())?;
                pending
                    .get_mut(&ticket)
                    .expect("admitted request exists")
                    .reservations
                    .push(range.clone());
                ranges.push(range);
            }
            Ok(ResultValue::Reserved(ranges))
        }
        #[cfg(feature = "gui")]
        Work::SaveView(view) => {
            let inner = executor.inner.lock().map_err(|error| error.to_string())?;
            if view.binding.journal_id != inner.store.journal_id()
                || view.binding.runtime_epoch != inner.epoch.0
                || inner
                    .state
                    .streams
                    .get(&view.binding.stream)
                    .is_none_or(|model| {
                        model.engine_incarnation != view.binding.incarnation
                            || model.applied.revision < view.binding.revision
                            || model.applied.batch < view.binding.published_cut
                    })
            {
                return Err("View snapshot belongs to a retired engine binding".into());
            }
            super::view_record::save(home, &view)?;
            Ok(ResultValue::ViewSaved)
        }
        Work::ReadPayload(reference) => {
            let inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let bytes = inner
                .store
                .read_payload(tasty_event_store::PayloadRef(reference.0))
                .map_err(|error| error.to_string())?;
            Ok(ResultValue::Payload { reference, bytes })
        }
        Work::ReadEngine(stream) => executor
            .with_state(|models| ResultValue::Engine(models.stream(&stream)))
            .map_err(|e| e.to_string()),
        Work::PutPreparation(input) => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("preparation input requires admission")?;
            let bytes = serde_json::to_vec(&input).map_err(|error| error.to_string())?;
            let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let epoch = inner.epoch;
            let reference = inner
                .store
                .put_payload(epoch, &bytes)
                .map_err(|error| error.to_string())?;
            let reference = tasty_domain::DataRef(reference.0);
            admitted.inputs.push(reference);
            Ok(ResultValue::InputStored(reference))
        }
        Work::ClaimPreparation { stream, operation } => {
            effects::claim(executor, &stream, &operation)
        }
        Work::Prepared { lease, result } => effects::prepared(executor, lease, result),
        Work::InstallationRejected { lease, reason } => effects::rejected(executor, lease, reason),
        Work::CleanupFinished { lease } => effects::cleaned(executor, lease),
        Work::CancelAdmission => {
            if pending.remove(&ticket).is_none() {
                for p in pending.values_mut() {
                    p.followers.retain(|f| *f != ticket);
                }
            }
            Ok(ResultValue::Cancelled)
        }
    }
}

fn publish(
    executor: &Executor<StructureDecider>,
    published: &mut Option<u64>,
    predecessor: &mut Option<tasty_domain::StructureModels>,
    acks: &Acknowledgements,
    engine_binding: Option<super::EngineBinding>,
    send: &impl Fn(Completion) -> bool,
) -> Result<(), String> {
    executor.with_state(|_| ()).map_err(|e| e.to_string())?;
    loop {
        let next = {
            let inner = executor.inner.lock().map_err(|e| e.to_string())?;
            inner
                .store
                .read_batches_after(*published, 1)
                .map_err(|e| e.to_string())?
                .pop()
        };
        let Some(batch) = next else {
            return Ok(());
        };
        let decoded = journal::stream_batch(&batch).map_err(|e| e.to_string())?;
        let mut previous = predecessor
            .take()
            .ok_or("committed batch has no live predecessor")?;
        let before = decoded
            .streams
            .keys()
            .map(|stream| (stream.clone(), previous.stream(stream)))
            .collect();
        tasty_domain::evolve_streams(&mut previous, &decoded).map_err(|error| error.to_string())?;
        *predecessor = Some(previous);
        if !send(Completion::Publish {
            engine_binding: engine_binding.clone(),
            batch: decoded,
            before,
        }) {
            return Err("application projection disconnected".into());
        }
        acknowledge(acks, batch.cut.batch_id)?;
        *published = Some(batch.cut.batch_id);
    }
}

fn acknowledge(acks: &Acknowledgements, expected: u64) -> Result<(), String> {
    let (acknowledged, applied) = acks
        .recv()
        .map_err(|_| "application projection disconnected")?;
    if acknowledged != expected {
        return Err("application acknowledged a different journal cut".into());
    }
    applied.map_err(|e| format!("committed projection halted: {e}"))
}
