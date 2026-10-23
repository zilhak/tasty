//! FIFO ready collection and a per-logical-request publication/ACK ledger.
use super::super::QueuedRequest;
use super::*;
use crate::runtime::command_executor::{ExecError, Executed, GroupCommand, GroupExecution, Source};

pub(super) struct Requests {
    receiver: mpsc::Receiver<QueuedRequest>,
    buffered: Option<QueuedRequest>,
}
impl Requests {
    pub(super) fn new(receiver: mpsc::Receiver<QueuedRequest>) -> Self {
        Self {
            receiver,
            buffered: None,
        }
    }
    pub(super) fn next(&mut self) -> Option<QueuedRequest> {
        self.buffered.take().or_else(|| self.receiver.recv().ok())
    }
    pub(super) fn collect(
        &mut self,
        first: QueuedRequest,
        allow_resolve: bool,
    ) -> Vec<QueuedRequest> {
        let mut ready = vec![first];
        while ready.len() < QUEUE_CAPACITY {
            let Ok(next) = self.receiver.try_recv() else {
                break;
            };
            if !eligible(&next.request.work, allow_resolve) {
                self.buffered = Some(next);
                break;
            }
            ready.push(next);
        }
        ready
    }
}

pub(super) fn eligible(work: &Work, allow_resolve: bool) -> bool {
    match work {
        Work::Resolve { .. } => allow_resolve,
        Work::Prepared { .. }
        | Work::CleanupFinished { .. }
        | Work::RetirementFinished { .. }
        | Work::InstallationRejected { .. }
        | Work::PreparationUncertain { .. } => true,
        #[cfg(feature = "gui")]
        Work::ForwardFinished { .. } => true,
        _ => false,
    }
}

struct Entry {
    queued: QueuedRequest, // includes the byte-budget token for the entire staging/publication lifetime
    admitted: Option<Pending>,
    measurement: metrics::Measurement,
}
impl Entry {
    fn build(
        &self,
        state: &tasty_core::StructureModels,
        store: &tasty_event_store::CommitGroup<'_>,
    ) -> Result<GroupCommand<ResolvedCommand>, ExecError<tasty_core::Rejection>> {
        use tasty_core::StructuralCommand as C;
        let finish = |lease: super::super::EffectLease, command, phase, view| {
            effects::prepare_finish(store, state, lease, command, phase, view)
                .map_err(ExecError::Record)
        };
        match &self.queued.request.work {
            Work::Resolve { changes, response } => resolve::request(
                self.admitted
                    .as_ref()
                    .ok_or_else(|| ExecError::Record("journal request was not admitted".into()))?,
                changes.clone(),
                response.clone(),
                state,
            )
            .map(GroupCommand::Ready)
            .map_err(ExecError::Record),
            Work::Prepared { lease, result } => finish(
                lease.clone(),
                C::FinishCreation {
                    operation: lease.operation.clone(),
                    result: result.clone(),
                },
                "prepared",
                None,
            ),
            Work::InstallationRejected { lease, reason } => finish(
                lease.clone(),
                C::RejectInstallation {
                    operation: lease.operation.clone(),
                    reason: reason.clone(),
                },
                "installation-rejected",
                None,
            ),
            Work::PreparationUncertain { lease, reason } => finish(
                lease.clone(),
                C::MarkPreparationUncertain {
                    operation: lease.operation.clone(),
                    reason: reason.clone(),
                },
                "uncertain",
                None,
            ),
            Work::CleanupFinished { lease, view } => finish(
                lease.clone(),
                C::FinishCleanup {
                    operation: lease.operation.clone(),
                },
                "cleaned",
                Some(view.clone()),
            ),
            Work::RetirementFinished { lease, outcome } => finish(
                lease.clone(),
                C::FinishRetirement {
                    operation: lease.operation.clone(),
                    outcome: outcome.clone(),
                },
                "retired",
                None,
            ),
            #[cfg(feature = "gui")]
            Work::ForwardFinished { lease, outcome } => finish(
                lease.clone(),
                C::FinishForward {
                    operation: lease.operation.clone(),
                    outcome: outcome.clone(),
                },
                "forwarded",
                None,
            ),
            _ => unreachable!("collector accepts only pure structural commits"),
        }
    }
}

#[allow(clippy::too_many_arguments)] // reason: 같은 worker의 admission 대기열을 그룹 공개가 끝난 뒤에만 다시 처리한다
pub(super) fn process(
    executor: &Executor<StructureDecider>,
    home: &std::path::Path,
    queued: Vec<QueuedRequest>,
    pending: &mut HashMap<u64, Pending>,
    waiting: &mut VecDeque<(u64, Admission)>,
    published: &mut Option<u64>,
    acks: &Acknowledgements,
    halted: &mut Option<String>,
    send: &impl Fn(Completion) -> bool,
    #[cfg(test)] fail_next_publication: &AtomicBool,
) -> bool {
    let observed = queued[0].observation.metrics.clone();
    observe_admissions(&observed, pending, waiting);
    let pending_before = pending.len();
    if !run(
        executor,
        queued,
        pending,
        published,
        acks,
        halted,
        send,
        #[cfg(test)]
        fail_next_publication,
    ) {
        return false;
    }
    let credit_released = pending.len() < pending_before;
    let connected = process_waiting(
        executor,
        home,
        pending,
        waiting,
        halted.as_deref(),
        credit_released,
        send,
    );
    observe_admissions(&observed, pending, waiting);
    connected
}

#[allow(clippy::too_many_arguments)] // reason: 같은 worker가 소유한 큐·ACK·admission·publication 자원을 함께 빌린다
fn run(
    executor: &Executor<StructureDecider>,
    queued: Vec<QueuedRequest>,
    pending: &mut HashMap<u64, Pending>,
    published: &mut Option<u64>,
    acks: &Acknowledgements,
    halted: &mut Option<String>,
    send: &impl Fn(Completion) -> bool,
    #[cfg(test)] fail_next_publication: &AtomicBool,
) -> bool {
    let predecessor = match executor.with_state(Clone::clone) {
        Ok(state) => Some(state),
        Err(error) => {
            *halted = Some(error.to_string());
            None
        }
    };
    let observed = queued[0].observation.metrics.clone();
    let entries: Vec<_> = queued
        .into_iter()
        .map(|queued| {
            queued.observation.start();
            let measurement =
                metrics::Measurement::begin(executor, queued.submitted, &queued.request.work);
            let admitted = if matches!(queued.request.work, Work::Resolve { .. }) {
                pending.remove(&queued.request.ticket)
            } else {
                None
            };
            Entry {
                queued,
                admitted,
                measurement,
            }
        })
        .collect();
    let mut publication = Publication {
        executor,
        observed,
        pending,
        published,
        acks,
        halted,
        predecessor,
        send,
        announced: false,
        #[cfg(test)]
        fail_next_publication,
    };
    let mut offset = 0;
    while offset < entries.len() {
        let execution = publication.execute(&entries[offset..]);
        if let Some(reason) = execution.halted {
            *publication.halted = Some(reason);
        }
        let count = execution.results.len();
        assert!(
            count > 0,
            "a nonempty group always consumes at least one bounded request"
        );
        for (entry, result) in entries[offset..offset + count]
            .iter()
            .zip(execution.results)
        {
            if !publication.complete(entry, result) {
                return false;
            }
        }
        offset += count;
    }
    true
}

type Execution = Result<Executed, ExecError<tasty_core::Rejection>>;

struct Publication<'a, F> {
    observed: Arc<super::super::JournalMetrics>,
    executor: &'a Executor<StructureDecider>,
    pending: &'a mut HashMap<u64, Pending>,
    published: &'a mut Option<u64>,
    acks: &'a Acknowledgements,
    halted: &'a mut Option<String>,
    predecessor: Option<tasty_core::StructureModels>,
    send: &'a F,
    announced: bool,
    #[cfg(test)]
    fail_next_publication: &'a AtomicBool,
}

impl<F: Fn(Completion) -> bool> Publication<'_, F> {
    fn execute(&self, entries: &[Entry]) -> GroupExecution<tasty_core::Rejection> {
        if let Some(reason) = self.halted.as_ref() {
            return GroupExecution {
                results: entries
                    .iter()
                    .map(|_| Err(ExecError::Apply(reason.clone())))
                    .collect(),
                halted: Some(reason.clone()),
            };
        }
        let oldest = entries
            .iter()
            .map(|entry| entry.queued.observation.submitted())
            .min()
            .expect("nonempty group");
        let started = self.observed.batch_start(oldest);
        let retries_before = self.executor.group_revision_retries();
        let measurement = metrics::GroupMeasurement::begin(self.executor);
        let execution = self
            .executor
            .execute_group(entries.len(), |index, state, store| {
                entries[index].build(state, store)
            });
        let resolves = entries[..execution.results.len()]
            .iter()
            .filter(|entry| matches!(entry.queued.request.work, Work::Resolve { .. }))
            .count();
        self.observed.batch_finished(
            started,
            execution.results.len(),
            execution.halted.is_some(),
            self.executor.group_revision_retries() - retries_before,
            entries[..execution.results.len()]
                .iter()
                .map(|entry| entry.queued.observation.bytes_estimate())
                .sum(),
        );
        for entry in &entries[..execution.results.len()] {
            entry.queued.observation.execution_finished();
        }
        measurement.finished(
            self.executor,
            execution.results.len(),
            resolves,
            execution.halted.is_some(),
        );
        execution
    }

    fn complete(&mut self, entry: &Entry, result: Execution) -> bool {
        let ticket = entry.queued.request.ticket;
        let batch = result.as_ref().ok().and_then(|result| match result.source {
            Source::Committed { batch } => batch,
            _ => None,
        });
        let mut result = entry.result_value(self.executor, result);
        entry.release_admission(self.executor, self.pending);
        if let Some(admitted) = &entry.admitted {
            self.observed.admission_released(
                1 + admitted.followers.len(),
                admitted.admission.original_digest.len(),
            );
        }
        entry
            .measurement
            .durable(self.executor, ticket, result.is_ok());
        self.publish(batch);
        if let Some(reason) = self.halted.as_ref() {
            result = Err(JournalError::Halted(reason.clone()));
        }
        if !self.announce_halt() {
            return false;
        }
        entry
            .queued
            .observation
            .finish(super::super::observability::Outcome::handled(
                result.is_ok(),
            ));
        entry
            .measurement
            .finished(self.executor, ticket, result.is_ok());
        entry.send_result(self.send, result)
    }

    fn publish(&mut self, batch: Option<u64>) {
        if self.halted.is_some() {
            return;
        }
        let Some(batch) = batch else {
            return;
        };
        if let Err(error) = publish(
            self.executor,
            &self.observed,
            self.published,
            &mut self.predecessor,
            self.acks,
            None,
            Some(batch),
            self.send,
            #[cfg(test)]
            self.fail_next_publication,
        ) {
            self.observed.publication_error();
            *self.halted = Some(error);
        }
    }

    fn announce_halt(&mut self) -> bool {
        if self.announced {
            return true;
        }
        let Some(reason) = self.halted.as_ref() else {
            return true;
        };
        release_halted_admissions(self.executor, self.pending);
        if !(self.send)(Completion::Halted(reason.clone())) {
            return false;
        }
        self.announced = true;
        true
    }
}

impl Entry {
    fn result_value(
        &self,
        executor: &Executor<StructureDecider>,
        result: Execution,
    ) -> Result<ResultValue, JournalError> {
        let executed = result.map_err(|error| JournalError::Internal(error.to_string()))?;
        if matches!(self.queued.request.work, Work::Resolve { .. })
            || executed.source != Source::Stored
        {
            return Ok(ResultValue::Executed(executed));
        }
        // Effect completion hits keep the existing Stored record transport contract.
        let record = executor
            .inner
            .lock()
            .map_err(|error| JournalError::Internal(error.to_string()))?
            .store
            .command(&executed.command_id)
            .map_err(|error| JournalError::Internal(error.to_string()))?
            .ok_or_else(|| JournalError::Internal("stored effect result missing".into()))?;
        Ok(ResultValue::Stored(record))
    }

    fn release_admission(
        &self,
        executor: &Executor<StructureDecider>,
        pending: &mut HashMap<u64, Pending>,
    ) {
        if !matches!(self.queued.request.work, Work::Resolve { .. }) {
            return;
        }
        // Durable event/effect pins already exist. On rollback this only releases the admission.
        let mut ignored = Vec::new();
        let mut release = true;
        release_finished_admission(
            executor,
            pending,
            self.queued.request.ticket,
            false,
            &mut ignored,
            &mut release,
        );
    }

    fn send_result(
        &self,
        send: &impl Fn(Completion) -> bool,
        result: Result<ResultValue, JournalError>,
    ) -> bool {
        let followers = self
            .admitted
            .as_ref()
            .map(|pending| pending.followers.as_slice())
            .unwrap_or_default();
        for ticket in std::iter::once(self.queued.request.ticket).chain(followers.iter().copied()) {
            if !send(Completion::Finished {
                ticket,
                result: result.clone(),
            }) {
                return false;
            }
        }
        true
    }
}
