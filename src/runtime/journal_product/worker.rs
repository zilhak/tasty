mod admission;
mod assembly;
mod binding;
mod capture;
mod effects;
mod recovery;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use tasty_event_store::CommandLookup;

use super::decider::{ResolvedCommand, StructureDecider};
use super::{Admission, Completion, JournalError, QUEUE_CAPACITY, ResultValue, Work, identity};
use crate::runtime::command_executor::{Executor, Request as ExecuteRequest};
use crate::runtime::journal;

struct Pending {
    disk_credit: u64,
    admission: Admission,
    followers: Vec<u64>,
    reservations: Vec<tasty_event_store::IdRange>,
    inputs: Vec<tasty_core::DataRef>,
}

const MAX_ADMISSION_IDS: u64 = 16_384;
impl Pending {
    fn check_ids(&self, additional: u64) -> Result<(), String> {
        let held = self
            .reservations
            .iter()
            .map(tasty_event_store::IdRange::len)
            .sum::<u64>();
        if held
            .checked_add(additional)
            .is_none_or(|total| total > MAX_ADMISSION_IDS)
        {
            return Err("admission ID reservation capacity exhausted".into());
        }
        Ok(())
    }
}

type Acknowledgements = mpsc::Receiver<(u64, Result<(), String>)>;

pub(super) fn run(
    home: PathBuf,
    readers: Arc<crate::runtime::journal_payload::PayloadReaders>,
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
    let Some((executor, mut published)) = open_published_executor(&home, &acknowledgements, &send)
    else {
        return;
    };
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    // Admissions waiting for a disk credit, in arrival order. They count toward QUEUE_CAPACITY.
    let mut waiting: VecDeque<(u64, Admission)> = VecDeque::new();
    let mut halted: Option<String> = None;
    while let Ok(queued) = requests.recv() {
        let request = queued.request;
        drop(queued._bytes);
        if closed.load(Ordering::Acquire) {
            break;
        }
        let pending_before = pending.len();
        if halted.is_none() && !waiting.is_empty() && matches!(request.work, Work::Admit(_)) {
            // A later admission must not take a credit ahead of one already waiting for it.
            let Work::Admit(admission) = request.work else {
                unreachable!("checked admission")
            };
            if !wait_for_credit(&pending, &mut waiting, request.ticket, admission, &send) {
                return;
            }
            continue;
        }
        let was_halted = halted.is_some();
        let mut predecessor = capture_predecessor(&executor, &request.work, &mut halted);
        let mut followers = if matches!(request.work, Work::Resolve { .. } | Work::CancelAdmission)
        {
            pending
                .get(&request.ticket)
                .map(|p| p.followers.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let checkpoint_requested = match &request.work {
            Work::Capture { .. } => true,
            #[cfg(any(feature = "gui", test))]
            Work::RetireEngine(_) => true,
            _ => false,
        };
        let mut release_admission = matches!(
            request.work,
            Work::Resolve { .. } | Work::CancelAdmission | Work::Capture { .. }
        );
        let is_admit = matches!(request.work, Work::Admit(_));
        let mut deferred = None;
        let mut result = match &halted {
            Some(reason) => Err(JournalError::Halted(reason.clone())),
            None => handle(
                &executor,
                &home,
                &mut pending,
                request.ticket,
                request.work,
                &mut deferred,
            ),
        };
        if let Some(admission) = deferred {
            if !wait_for_credit(&pending, &mut waiting, request.ticket, admission, &send) {
                return;
            }
            continue;
        }
        release_finished_admission(
            &executor,
            &mut pending,
            request.ticket,
            result.is_err() && !is_admit,
            &mut followers,
            &mut release_admission,
        );
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
            result = Err(JournalError::Halted(error));
        }
        if !was_halted && halted.is_some() {
            release_halted_admissions(&executor, &mut pending);
        }
        if checkpoint_requested && halted.is_none() && result.is_ok() {
            checkpoint_published(&executor, published, &readers);
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
        let credit_released = pending.len() < pending_before;
        if !process_waiting(
            &executor,
            &home,
            &mut pending,
            &mut waiting,
            halted.as_deref(),
            credit_released,
            &send,
        ) {
            return;
        }
    }
    if halted.is_none() {
        checkpoint_published(&executor, published, &readers);
    }
}

fn capture_predecessor(
    executor: &Executor<StructureDecider>,
    work: &Work,
    halted: &mut Option<String>,
) -> Option<tasty_core::StructureModels> {
    let publishes = match work {
        Work::ReconcilePreparation { .. }
        | Work::ReconcileRetirement { .. }
        | Work::Capture { .. }
        | Work::RetirementFinished { .. }
        | Work::OpenEngine { .. }
        | Work::Resolve { .. }
        | Work::Prepared { .. }
        | Work::CleanupFinished { .. }
        | Work::InstallationRejected { .. }
        | Work::PreparationUncertain { .. }
        | Work::ClaimPreparation { .. } => true,
        #[cfg(any(feature = "gui", test))]
        Work::RetireEngine(_) => true,
        #[cfg(feature = "gui")]
        Work::ForwardFinished { .. } => true,
        _ => false,
    };
    if publishes && halted.is_none() {
        match executor.with_state(|models| binding::publication_predecessor(models, work)) {
            Ok(models) => Some(models),
            Err(error) => {
                *halted = Some(error.to_string());
                None
            }
        }
    } else {
        None
    }
}

/// Complete halted waiters or retry them in arrival order after a credit is released.
fn process_waiting(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    pending: &mut HashMap<u64, Pending>,
    waiting: &mut VecDeque<(u64, Admission)>,
    halted: Option<&str>,
    credit_released: bool,
    send: &impl Fn(Completion) -> bool,
) -> bool {
    if let Some(reason) = halted {
        for (ticket, _) in waiting.drain(..) {
            if !send(Completion::Finished {
                ticket,
                result: Err(JournalError::Halted(reason.to_owned())),
            }) {
                return false;
            }
        }
    } else if credit_released {
        while let Some((ticket, admission)) = waiting.pop_front() {
            let mut deferred = None;
            let result = handle(
                executor,
                home,
                pending,
                ticket,
                Work::Admit(admission),
                &mut deferred,
            );
            if let Some(admission) = deferred {
                waiting.push_front((ticket, admission));
                break;
            }
            if !send(Completion::Finished { ticket, result }) {
                return false;
            }
        }
    }
    true
}

/// Queue an admission until a credit returns. The wait shares the admission count limit, so a
/// full queue still answers immediately. Returns false when the App side has gone away.
fn wait_for_credit(
    pending: &HashMap<u64, Pending>,
    waiting: &mut VecDeque<(u64, Admission)>,
    ticket: u64,
    admission: Admission,
    send: &impl Fn(Completion) -> bool,
) -> bool {
    let admitted: usize = pending.values().map(|p| 1 + p.followers.len()).sum();
    if admitted + waiting.len() >= QUEUE_CAPACITY {
        return send(Completion::Finished {
            ticket,
            result: Err(JournalError::QueueFull(
                "journal admission capacity exhausted",
            )),
        });
    }
    waiting.push_back((ticket, admission));
    true
}

/// Maintenance cannot change an already committed command response. The leaf atomically replaces
/// snapshot/live pins and preserves the previous checkpoint on failure; retry at the next capture,
/// retirement or graceful worker stop, never by waking an unbounded maintenance loop.
fn checkpoint_published(
    executor: &Executor<StructureDecider>,
    published: Option<u64>,
    readers: &crate::runtime::journal_payload::PayloadReaders,
) {
    match executor.with_state(|models| models.batch) {
        Ok(cut) if cut == published => {
            checkpoint_current(executor, readers);
        }
        Ok(_) => {
            tracing::warn!("structure checkpoint skipped: committed cut has not been published")
        }
        Err(error) => {
            tracing::warn!(%error,"structure checkpoint skipped: canonical state unavailable")
        }
    }
}

/// `deferred` receives an admission that must wait for a credit; its returned value is unused.
fn handle(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    pending: &mut HashMap<u64, Pending>,
    ticket: u64,
    work: Work,
    deferred: &mut Option<Admission>,
) -> Result<ResultValue, JournalError> {
    match work {
        Work::Admit(admission) => admission::admit(executor, pending, ticket, admission, deferred),
        work => handle_work(executor, home, pending, ticket, work).map_err(JournalError::Internal),
    }
}

fn handle_work(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    pending: &mut HashMap<u64, Pending>,
    ticket: u64,
    work: Work,
) -> Result<ResultValue, String> {
    match work {
        Work::CapturePreset { draft } => capture::preset(executor, draft)
            .map(|(preset, base_name)| ResultValue::CapturedPreset { preset, base_name }),
        Work::ReconcilePreparation {
            lease,
            evidence,
            discarded,
            view,
        } => recovery::reconcile_preparation(executor, lease, evidence, discarded, view),
        Work::ReconcileRetirement { lease, evidence } => {
            recovery::reconcile_retirement(executor, lease, evidence)
        }
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
            Ok(recovery::command_result(&inner.state, record, false))
        }
        #[cfg(any(feature = "gui", test))]
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
            pending.values().map(|entry| entry.disk_credit).sum(),
        ),
        Work::Admit(_) => unreachable!("admission is handled at the typed boundary"),
        Work::Resolve {
            mut changes,
            response,
        } => {
            let admitted = pending
                .remove(&ticket)
                .ok_or("journal request was not admitted")?;
            for change in &changes {
                if matches!(
                    change.command,
                    tasty_core::StructuralCommand::ReconcilePreparation { .. }
                        | tasty_core::StructuralCommand::ReconcileRetirement { .. }
                        | tasty_core::StructuralCommand::RecoverOperation { .. }
                        | tasty_core::StructuralCommand::RecordCapture { .. }
                        | tasty_core::StructuralCommand::OpenEngine { .. }
                        | tasty_core::StructuralCommand::RetireEngine { .. }
                        | tasty_core::StructuralCommand::FinishCreation { .. }
                        | tasty_core::StructuralCommand::FinishCleanup { .. }
                        | tasty_core::StructuralCommand::FinishRetirement { .. }
                        | tasty_core::StructuralCommand::FinishForward { .. }
                        | tasty_core::StructuralCommand::CancelUnstartedCreation { .. }
                        | tasty_core::StructuralCommand::RejectInstallation { .. }
                        | tasty_core::StructuralCommand::MarkPreparationUncertain { .. }
                ) {
                    return Err("effect results require their validated lease endpoint".into());
                }
                if let tasty_core::StructuralCommand::PrepareCreation { input, .. }
                | tasty_core::StructuralCommand::Close { input, .. }
                | tasty_core::StructuralCommand::PrepareAssembly { input, .. }
                | tasty_core::StructuralCommand::PrepareForward { input, .. }
                | tasty_core::StructuralCommand::Replace { input, .. } = &change.command
                    && !admitted.inputs.contains(input)
                {
                    return Err("preparation input belongs to another admission".into());
                }
                if let tasty_core::StructuralCommand::Close {
                    undo: Some(capture),
                    ..
                } = &change.command
                    && capture
                        .data_refs()
                        .any(|reference| !admitted.inputs.contains(&reference))
                {
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
            executor
                .with_state(|models| {
                    for change in &mut changes {
                        if let tasty_core::StructuralCommand::PrepareCreation { plan, .. } =
                            &mut change.command
                            && let tasty_core::CreationDestination::Convert {
                                surface,
                                previous_activation,
                                ..
                            } = &mut plan.destination
                            && *previous_activation == Some(0)
                        {
                            *previous_activation = models
                                .streams
                                .get(&change.stream)
                                .and_then(|model| model.surfaces.get(surface))
                                .and_then(|surface| {
                                    surface.activation.map(|activation| activation.generation)
                                });
                        }
                    }
                })
                .map_err(|error| error.to_string())?;
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
        Work::PrepareSubtree { binding, draft } => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("preset input has no admitted owner")?;
            assembly::preset(executor, admitted, ticket, binding, draft)
        }
        Work::PrepareUndo {
            binding,
            target_pane,
            scope,
            shell,
        } => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("undo input has no admitted owner")?;
            assembly::undo(
                executor,
                admitted,
                ticket,
                binding,
                target_pane,
                scope,
                shell,
            )
        }
        #[cfg(feature = "gui")]
        Work::ClaimForward { stream, operation } => {
            effects::claim_forward(executor, &stream, &operation)
        }
        #[cfg(feature = "gui")]
        Work::ForwardFinished { lease, outcome } => effects::forwarded(executor, lease, outcome),
        Work::Capture { binding, surfaces } => {
            capture::persist(executor, ticket, binding, surfaces)
        }
        Work::CaptureClosed {
            view,
            binding,
            target,
            display_name,
            surfaces,
        } => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("close capture has no admitted request")?;
            let (input, undo) = capture::closed(
                executor,
                ticket,
                binding,
                target,
                display_name,
                surfaces,
                view,
            )?;
            admitted.inputs.push(input);
            if let Some(capture) = &undo {
                admitted.inputs.extend(capture.data_refs());
            }
            Ok(ResultValue::ClosedCaptured { input, undo })
        }
        Work::ReserveExecutionIds { binding, kinds } => {
            executor
                .with_state(|_| ())
                .map_err(|error| error.to_string())?;
            let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let epoch = inner.epoch;
            if binding.journal_id != inner.store.journal_id()
                || binding.runtime_epoch != epoch.0
                || inner
                    .state
                    .streams
                    .get(&binding.stream)
                    .is_none_or(|model| {
                        model.engine_retired || model.engine_incarnation != binding.incarnation
                    })
            {
                return Err("execution reservation belongs to a retired engine".into());
            }
            if kinds.len() > 4 {
                return Err("invalid execution reservation kind count".into());
            }
            let mut ranges = Vec::new();
            for (kind, count) in kinds {
                if kind == tasty_core::IdKind::Category || count == 0 {
                    return Err("execution reservation has invalid kind/count".into());
                }
                let max = if kind == tasty_core::IdKind::Surface {
                    0x7fff_ffff
                } else {
                    u32::MAX
                };
                ranges.push(
                    inner
                        .store
                        .reserve_ids(epoch, kind.label(), u64::from(count), u64::from(max))
                        .map_err(|error| error.to_string())?,
                );
            }
            Ok(ResultValue::ExecutionIds { binding, ranges })
        }
        Work::Reserve(kinds) => {
            if !pending.contains_key(&ticket) {
                return Err("ID reservation requires an admitted request".into());
            }
            if kinds.len() > 5 || kinds.iter().any(|(_, count)| *count == 0 || *count > 4096) {
                return Err("invalid structure ID reservation size".into());
            }
            pending
                .get(&ticket)
                .ok_or("ID reservation requires admission")?
                .check_ids(kinds.iter().map(|(_, count)| u64::from(*count)).sum())?;
            let mut inner = executor.inner.lock().map_err(|e| e.to_string())?;
            let epoch = inner.epoch;
            let mut ranges = Vec::new();
            for (kind, count) in kinds {
                let max = if kind == tasty_core::IdKind::Surface {
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
        Work::SaveView(view) => capture::save_view(executor, home, &view),
        Work::ReadPayload(reference) => {
            let inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let bytes = inner
                .store
                .read_payload(tasty_event_store::PayloadRef(reference.0))
                .map_err(|error| error.to_string())?;
            Ok(ResultValue::Payload { reference, bytes })
        }
        #[cfg(test)]
        Work::ReadEngine(stream) => executor
            .with_state(|models| ResultValue::Engine(models.stream(&stream)))
            .map_err(|e| e.to_string()),
        Work::PutPayload(bytes) => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("payload has no admitted owner")?;
            let inner = &mut *executor.inner.lock().map_err(|error| error.to_string())?;
            let epoch = inner.epoch;
            let reference = inner
                .store
                .put_admission_payload_pinned(
                    epoch,
                    &bytes,
                    &format!("admission/{}/{ticket}", epoch.0),
                )
                .map_err(|error| error.to_string())?;
            let reference = tasty_core::DataRef(reference.0);
            admitted.inputs.push(reference);
            Ok(ResultValue::InputStored(reference))
        }
        Work::PutPreparation(input) => {
            let admitted = pending
                .get_mut(&ticket)
                .ok_or("preparation input requires admission")?;
            let bytes = serde_json::to_vec(&input).map_err(|error| error.to_string())?;
            let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
            let epoch = inner.epoch;
            let reference = inner
                .store
                .put_admission_payload_pinned(
                    epoch,
                    &bytes,
                    &format!("admission/{}/{ticket}", epoch.0),
                )
                .map_err(|error| error.to_string())?;
            let reference = tasty_core::DataRef(reference.0);
            admitted.inputs.push(reference);
            Ok(ResultValue::InputStored(reference))
        }
        Work::ClaimRetirement { stream, operation } => {
            effects::claim_retirement(executor, &stream, &operation)
        }
        Work::RetirementFinished { lease, outcome } => effects::retired(executor, lease, outcome),
        Work::ClaimPreparation { stream, operation } => {
            effects::claim(executor, &stream, &operation)
        }
        Work::Prepared { lease, result } => effects::prepared(executor, lease, result),
        Work::InstallationRejected { lease, reason } => effects::rejected(executor, lease, reason),
        Work::PreparationUncertain { lease, reason } => effects::uncertain(executor, lease, reason),
        Work::CleanupFinished { lease, view } => effects::cleaned(executor, lease, view),
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
    predecessor: &mut Option<tasty_core::StructureModels>,
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
        tasty_core::evolve_streams(&mut previous, &decoded).map_err(|error| error.to_string())?;
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

fn release_finished_admission(
    executor: &Executor<StructureDecider>,
    pending: &mut HashMap<u64, Pending>,
    ticket: u64,
    failed: bool,
    followers: &mut Vec<u64>,
    release_admission: &mut bool,
) {
    // A failed preparation is terminal for this unresolved admission. Release its credit
    // and notify joined callers even when App never sends a later CancelAdmission.
    if failed && let Some(abandoned) = pending.remove(&ticket) {
        for follower in abandoned.followers {
            if !followers.contains(&follower) {
                followers.push(follower);
            }
        }
        *release_admission = true;
    }
    if *release_admission {
        let mut inner = executor.inner.lock().expect("worker executor lock");
        let epoch = inner.epoch;
        if let Err(error) = inner.store.release_payload_holder(
            epoch,
            &format!("admission/{}/{ticket}", epoch.0, ticket = ticket),
        ) {
            // A leaked pin is conservative; it is reclaimed by a future fenced writer.
            tracing::warn!("admission payload pin release failed: {error}");
        }
    }
}

fn release_halted_admissions(
    executor: &Executor<StructureDecider>,
    pending: &mut HashMap<u64, Pending>,
) {
    // Publication failure ends all unresolved admissions. Their credits must not survive
    // the terminal transport failure; committed operations already have durable pins.
    let abandoned: Vec<_> = pending.drain().map(|(ticket, _)| ticket).collect();
    let mut inner = executor.inner.lock().expect("worker executor lock");
    let epoch = inner.epoch;
    for ticket in abandoned {
        if let Err(error) = inner
            .store
            .release_payload_holder(epoch, &format!("admission/{}/{ticket}", epoch.0))
        {
            tracing::warn!(%error,"halted admission pin remains for fenced-writer cleanup");
        }
    }
}

fn open_published_executor(
    home: &std::path::Path,
    acknowledgements: &Acknowledgements,
    send: &impl Fn(Completion) -> bool,
) -> Option<(Executor<StructureDecider>, Option<u64>)> {
    let opened = identity::open(home)
        .and_then(|store| Executor::open(StructureDecider, store).map_err(|e| e.to_string()));
    let executor = match opened {
        Ok(executor) => executor,
        Err(error) => {
            send(Completion::StartupFailed(error));
            return None;
        }
    };
    {
        let mut inner = executor.inner.lock().expect("new executor lock");
        let epoch = inner.epoch;
        if let Err(error) = inner.store.release_abandoned_admission_holders(epoch) {
            send(Completion::StartupFailed(error.to_string()));
            return None;
        }
    }
    if let Err(error) = recovery::recover(&executor) {
        send(Completion::StartupFailed(error));
        return None;
    }
    let published = {
        let inner = executor.inner.lock().expect("new executor lock");
        let cut = inner.state.batch;
        if !send(Completion::Ready {
            #[cfg(test)]
            journal_id: inner.store.journal_id().to_owned(),
            runtime_epoch: inner.epoch.0,
            cut,
            bootstrap: inner.state.clone(),
        }) {
            return None;
        }
        cut
    };
    if let Err(error) = acknowledge(acknowledgements, published.unwrap_or(0)) {
        send(Completion::StartupFailed(error));
        return None;
    }
    Some((executor, published))
}

fn checkpoint_current(
    executor: &Executor<StructureDecider>,
    readers: &crate::runtime::journal_payload::PayloadReaders,
) {
    if let Err(error) = capture::checkpoint(executor, readers) {
        tracing::warn!(%error,"structure checkpoint failed; retaining previous checkpoint");
    }
}
