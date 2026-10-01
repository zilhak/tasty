mod binding;
mod effects;
mod capture;
mod assembly;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use tasty_event_store::CommandLookup;

use super::decider::{ResolvedCommand, StructureDecider};
use super::{Admission, Completion, QUEUE_CAPACITY, ResultValue, Work, identity};
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
    requests: mpsc::Receiver<super::QueuedRequest>,
    completions: mpsc::SyncSender<Completion>,
    acknowledgements: Acknowledgements,
    closed: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
    #[cfg(test)] fail_next_publication: Arc<AtomicBool>,
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
    {
        let mut inner=executor.inner.lock().expect("new executor lock");
        let epoch=inner.epoch;
        if let Err(error)=inner.store.release_abandoned_admission_holders(epoch) {send(Completion::StartupFailed(error.to_string()));return;}
    }
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
    while let Ok(queued) = requests.recv() {
        let request = queued.request;
        drop(queued._bytes);
        if closed.load(Ordering::Acquire) {
            break;
        }
        let was_halted = halted.is_some();
        let mut predecessor = if matches!(
            request.work,
            Work::Capture {..}
                | Work::RetirementFinished {..}
                | Work::OpenEngine { .. }
                | Work::RetireEngine(_)
                | Work::Resolve { .. }
                | Work::Prepared { .. }
                | Work::CleanupFinished { .. }
                | Work::InstallationRejected { .. }
                | Work::PreparationUncertain { .. }
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
        let followers = if matches!(request.work, Work::Resolve { .. } | Work::CancelAdmission) {
            pending
                .get(&request.ticket)
                .map(|p| p.followers.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let release_admission=matches!(request.work,Work::Resolve {..}|Work::CancelAdmission);
        let mut result = match &halted {
            Some(reason) => Err(reason.clone()),
            None => handle(&executor, &home, &mut pending, request.ticket, request.work),
        };
        if release_admission {
            let mut inner=executor.inner.lock().expect("worker executor lock");
            let epoch=inner.epoch;
            if let Err(error)=inner.store.release_payload_holder(epoch,&format!("admission/{}/{ticket}",epoch.0,ticket=request.ticket)) {
                // A leaked pin is conservative; it is reclaimed by a future fenced writer.
                tracing::warn!("admission payload pin release failed: {error}");
            }
        }
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
                #[cfg(test)]
                &fail_next_publication,
            )
        {
            halted = Some(error.clone());
            result = Err(error);
        }
        if !was_halted
            && let Some(reason) = &halted
            && !send(Completion::Halted(reason.clone()))
        {
            return;
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
        Work::ReadCommand(command_id) => {
            executor
                .with_state(|_| ())
                .map_err(|error| error.to_string())?;
            let inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let record = inner
                .store
                .command(&command_id)
                .map_err(|error| error.to_string())?
                .ok_or("original command missing")?;
            Ok(ResultValue::Command(record))
        }
        Work::RetireEngine(binding) => binding::retire(executor, home, ticket, binding),
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
            let pending_count: usize = pending.values().map(|p| 1 + p.followers.len()).sum();
            if let Some(key) = &admission.key {
                for (leader, existing) in pending.iter_mut() {
                    if existing.admission.key.as_ref() == Some(key) {
                        if existing.admission.original_digest != admission.original_digest {
                            return Err("idempotency key belongs to a different request".into());
                        }
                        if pending_count >= QUEUE_CAPACITY {
                            return Err("journal admission capacity exhausted".into());
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
            let held_bytes: usize = pending
                .values()
                .map(|pending| pending.admission.original_digest.len())
                .sum();
            if held_bytes.saturating_add(admission.original_digest.len()) > super::MAX_QUEUED_BYTES
            {
                return Err("journal pending admission byte capacity exhausted".into());
            }
            if pending_count >= QUEUE_CAPACITY {
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
        Work::Resolve { mut changes, response } => {
            let admitted = pending
                .remove(&ticket)
                .ok_or("journal request was not admitted")?;
            for change in &changes {
                if matches!(
                    change.command,
                    tasty_domain::StructuralCommand::RecordCapture {..}
                        | tasty_domain::StructuralCommand::OpenEngine { .. }
                        | tasty_domain::StructuralCommand::RetireEngine { .. }
                        | tasty_domain::StructuralCommand::FinishCreation { .. }
                        | tasty_domain::StructuralCommand::FinishCleanup { .. }
                        | tasty_domain::StructuralCommand::FinishRetirement {..}
                        | tasty_domain::StructuralCommand::CancelUnstartedCreation { .. }
                        | tasty_domain::StructuralCommand::RejectInstallation { .. }
                            | tasty_domain::StructuralCommand::MarkPreparationUncertain { .. }
                ) {
                    return Err("effect results require their validated lease endpoint".into());
                }
                if let tasty_domain::StructuralCommand::PrepareCreation { input, .. }
                    |tasty_domain::StructuralCommand::Close {input,..}
                    |tasty_domain::StructuralCommand::PrepareAssembly {input,..} = &change.command
                    && !admitted.inputs.contains(input)
                {
                    return Err("preparation input belongs to another admission".into());
                }
                if let tasty_domain::StructuralCommand::Close {undo:Some(capture),..}=&change.command
                    && capture.data_refs().any(|reference|!admitted.inputs.contains(&reference)) {
                    return Err("undo capture belongs to another admission".into());
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
            // Resolve admission-only activation markers against the sole canonical source.
            // The decider receives a fixed plan and cannot read a live projection or the database.
            executor.with_state(|models| {
                for change in &mut changes {
                    if let tasty_domain::StructuralCommand::PrepareCreation {plan,..}=&mut change.command
                        && let tasty_domain::CreationDestination::Convert {surface,previous_activation,..}=&mut plan.destination
                        && *previous_activation==Some(0) {
                            *previous_activation=models.streams.get(&change.stream).and_then(|model|model.surfaces.get(surface)).and_then(|surface|surface.activation.map(|activation|activation.generation));
                        }
                }
            }).map_err(|error|error.to_string())?;
            let admission = admitted.admission;
            let executed = executor
                .execute(&ExecuteRequest {
                    key: admission.key,
                    actor: admission.actor,
                    origin: admission.origin,
                    causation_id: admission.causation_id,
                    command: ResolvedCommand {
                        original_digest: admission.original_digest,
                        response,
                        changes,
                        effect_result: None,
                        cancellation: None,
                        completion_view: None,
                        original_results: Default::default(),
                    },
                })
                .map_err(|e| e.to_string())?;
            Ok(ResultValue::Executed(executed))
        }
        Work::PrepareSubtree {binding,draft}=> {
            let admitted=pending.get_mut(&ticket).ok_or("preset input has no admitted owner")?;
            assembly::preset(executor,admitted,ticket,binding,draft)
        },
        Work::PrepareUndo {binding,target_pane,scope,shell}=> {
            let admitted=pending.get_mut(&ticket).ok_or("undo input has no admitted owner")?;
            assembly::undo(executor,admitted,ticket,binding,target_pane,scope,shell)
        },
        Work::Capture {binding,surfaces}=>capture::persist(executor,ticket,binding,surfaces),
        Work::CaptureClosed {view,binding,target,display_name,surfaces}=>{
            let admitted=pending.get_mut(&ticket).ok_or("close capture has no admitted request")?;
            let (input,undo)=capture::closed(executor,ticket,binding,target,display_name,surfaces,view)?;
            admitted.inputs.push(input);
            if let Some(capture)=&undo {admitted.inputs.extend(capture.data_refs());}
            Ok(ResultValue::ClosedCaptured {input,undo})
        },
        Work::ReserveExecutionIds {binding,kinds}=>{
            executor.with_state(|_|()).map_err(|error|error.to_string())?;
            let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;
            let epoch=inner.epoch;
            if binding.journal_id!=inner.store.journal_id() || binding.runtime_epoch!=epoch.0 || inner.state.streams.get(&binding.stream).is_none_or(|model|model.engine_retired || model.engine_incarnation!=binding.incarnation) {return Err("execution reservation belongs to a retired engine".into());}
            if kinds.len()>4 {return Err("invalid execution reservation kind count".into());}
            let mut ranges=Vec::new();
            for (kind,count) in kinds {
                if kind==tasty_domain::IdKind::Category || count==0 {return Err("execution reservation has invalid kind/count".into());}
                let max=if kind==tasty_domain::IdKind::Surface {0x7fff_ffff}else {u32::MAX};
                ranges.push(inner.store.reserve_ids(epoch,kind.label(),u64::from(count),u64::from(max)).map_err(|error|error.to_string())?);
            }
            Ok(ResultValue::ExecutionIds {binding,ranges})
        },
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
                        model.engine_retired
                            || model.engine_incarnation != view.binding.incarnation
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
        Work::PutPayload(bytes)=> {
            let admitted=pending.get_mut(&ticket).ok_or("payload has no admitted owner")?;
            let inner=&mut *executor.inner.lock().map_err(|error|error.to_string())?;
            let epoch=inner.epoch;
            let reference=inner.store.put_payload_pinned(epoch,&bytes,&format!("admission/{}/{ticket}",epoch.0)).map_err(|error|error.to_string())?;
            let reference=tasty_domain::DataRef(reference.0);admitted.inputs.push(reference);Ok(ResultValue::InputStored(reference))
        },
        Work::PutPreparation(input) => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("preparation input requires admission")?;
            let bytes = serde_json::to_vec(&input).map_err(|error| error.to_string())?;
            let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let epoch = inner.epoch;
            let reference = inner
                .store
                .put_payload_pinned(epoch, &bytes, &format!("admission/{}/{ticket}",epoch.0))
                .map_err(|error| error.to_string())?;
            let reference = tasty_domain::DataRef(reference.0);
            admitted.inputs.push(reference);
            Ok(ResultValue::InputStored(reference))
        }
        Work::ClaimRetirement {stream,operation}=>effects::claim_retirement(executor,&stream,&operation),
        Work::RetirementFinished {lease,outcome}=>effects::retired(executor,lease,outcome),
        Work::ClaimPreparation { stream, operation } => {
            effects::claim(executor, &stream, &operation)
        }
        Work::Prepared { lease, result } => effects::prepared(executor, lease, result),
        Work::InstallationRejected { lease, reason } => effects::rejected(executor, lease, reason),
        Work::PreparationUncertain {lease,reason}=>effects::uncertain(executor,lease,reason),
        Work::CleanupFinished {lease,view}=>effects::cleaned(executor,lease,view),
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
    #[cfg(test)] fail_next_publication: &AtomicBool,
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
        #[cfg(test)]
        if fail_next_publication.swap(false, Ordering::AcqRel) {
            return Err("injected failure after commit before publication".into());
        }
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
