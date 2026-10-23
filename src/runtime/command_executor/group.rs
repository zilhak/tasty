//! Sequential private staging. The worker owns queue/byte bounds and publication.
use super::*;
use tasty_event_store::CommitGroup;

pub(crate) enum GroupCommand<C> {
    Ready(Request<C>),
    Stored(CommandRecord),
}

pub(crate) struct GroupExecution<R: fmt::Debug> {
    pub(crate) results: Vec<ExecResult<R>>,
    pub(crate) halted: Option<String>,
}

impl<D: JournalDecider<State: Clone>> Executor<D> {
    /// `build` must be pure apart from reads through the transaction view. No claims, ID
    /// reservations, memory waits, publication, or execution may occur while this view is held.
    /// A short result prefix means the next generated request starts another physical group.
    pub(crate) fn execute_group(
        &self,
        count: usize,
        mut build: impl FnMut(
            usize,
            &D::State,
            &CommitGroup<'_>,
        ) -> Result<GroupCommand<D::Command>, ExecError<D::Rejection>>,
    ) -> GroupExecution<D::Rejection> {
        // A leader which already registered must finish before we take this gate. New leaders
        // register only after the whole transaction and private state publication are complete.
        let gate = match self.group_gate() {
            Ok(gate) => gate,
            Err(error) => return failed(count, error),
        };
        let mut inner = match self.lock_inner() {
            Ok(inner) => inner,
            Err(error) => return failed(count, error),
        };
        if inner.halted {
            return failed(count, ExecError::Halted);
        }
        if inner.stale
            && let Err(error) = self.reload(&mut inner)
        {
            return failed(count, error);
        }
        for attempt in 0..MAX_DECIDE_ATTEMPTS {
            match self.stage_group(&mut inner, count, &mut build, &gate) {
                GroupAttempt::Done(result) => {
                    if result.halted.is_some() {
                        inner.halted = true;
                    }
                    return result;
                }
                GroupAttempt::Conflict if attempt + 1 < MAX_DECIDE_ATTEMPTS => {
                    if let Err(error) = self.reload(&mut inner) {
                        return failed(count, error);
                    }
                }
                GroupAttempt::Conflict => {
                    return failed(count, ExecError::RetriesExhausted(MAX_DECIDE_ATTEMPTS));
                }
            }
        }
        unreachable!("attempts are bounded and nonzero")
    }

    fn group_gate(&self) -> Result<GroupGate<'_, D::Rejection>, ExecError<D::Rejection>> {
        let mut joined = HashMap::new();
        loop {
            let lock = self.in_flight.lock().map_err(|_| ExecError::Poisoned)?;
            if lock.is_empty() {
                return Ok(GroupGate {
                    _lock: lock,
                    joined,
                });
            }
            let waiting: Vec<_> = lock
                .iter()
                .map(|(key, slot)| {
                    joined.entry(key.clone()).or_insert_with(|| slot.clone());
                    slot.clone()
                })
                .collect();
            drop(lock);
            #[cfg(test)]
            self.group_waiting_for_in_flight
                .store(true, std::sync::atomic::Ordering::SeqCst);
            for slot in waiting {
                wait_ready(&slot)?;
            }
            #[cfg(test)]
            self.group_waiting_for_in_flight
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn stage_group(
        &self,
        inner: &mut Inner<D::State>,
        count: usize,
        build: &mut impl FnMut(
            usize,
            &D::State,
            &CommitGroup<'_>,
        ) -> Result<GroupCommand<D::Command>, ExecError<D::Rejection>>,
        gate: &GroupGate<'_, D::Rejection>,
    ) -> GroupAttempt<D::Rejection> {
        let mut staged = inner.state.clone();
        let epoch = inner.epoch;
        let mut group = match inner.store.begin_commit_group(epoch) {
            Ok(group) => group,
            Err(error) => {
                if loses_writer(&error) {
                    inner.halted = true;
                }
                return GroupAttempt::Done(failed(count, store_error(error)));
            }
        };
        let mut results = Vec::with_capacity(count);
        for index in 0..count {
            let prepared = build(index, &staged, &group);
            match self.stage_one(
                &mut group,
                &mut staged,
                &mut inner.next_command,
                epoch,
                prepared,
                gate,
            ) {
                Step::Done(result) => results.push(result),
                Step::Full => break,
                Step::Conflict => return GroupAttempt::Conflict,
                Step::Fatal(error) => return GroupAttempt::Done(failed(count, error)),
            }
        }
        if let Err(error) = group.finish() {
            inner.stale = true;
            // Never retry an uncertain physical COMMIT. The caller halts publication/execution.
            let mut failure = failed(count, store_error(error));
            failure.halted = failure.halted.map(|reason| format!("physical commit failed; reopen the journal to determine its durable outcome: {reason}"));
            return GroupAttempt::Done(failure);
        }
        inner.state = staged;
        GroupAttempt::Done(GroupExecution {
            results,
            halted: None,
        })
    }

    fn stage_one(
        &self,
        group: &mut CommitGroup<'_>,
        staged: &mut D::State,
        next_command: &mut u64,
        epoch: WriterEpoch,
        prepared: Result<GroupCommand<D::Command>, ExecError<D::Rejection>>,
        gate: &GroupGate<'_, D::Rejection>,
    ) -> Step<D::Rejection> {
        let request = match prepared {
            Ok(GroupCommand::Ready(request)) => request,
            Ok(GroupCommand::Stored(record)) => {
                if let Some(key) = &record.key
                    && let Some(result) = joined_result(gate, key, &record.request_digest)
                {
                    return Step::Done(result);
                }
                return Step::Done(stored(record));
            }
            Err(error) => return Step::Done(Err(error)),
        };
        let digest = self.decider.request_digest(&request.command);
        if let Some(key) = &request.key {
            if let Some(result) = joined_result(gate, key, &digest) {
                return Step::Done(result);
            }
            match group.lookup_command(key, &digest) {
                Ok(CommandLookup::Hit(record)) => return Step::Done(stored(record)),
                Ok(CommandLookup::DigestMismatch(_)) => {
                    return Step::Done(Err(ExecError::KeyConflict(key.clone())));
                }
                Ok(CommandLookup::Miss) => {}
                Err(error) => return Step::Fatal(store_error(error)),
            }
        }
        let (command_id, commit) =
            match self.group_draft(staged, next_command, epoch, &digest, &request) {
                Ok(draft) => draft,
                Err(error) => return Step::Done(Err(error)),
            };
        match group.fits(&commit) {
            Ok(false) => return Step::Full,
            Ok(true) => {}
            Err(error) => return Step::Done(Err(store_error(error))),
        }
        self.append_staged(group, staged, command_id, &commit)
    }

    fn group_draft(
        &self,
        state: &D::State,
        next_command: &mut u64,
        epoch: WriterEpoch,
        digest: &[u8],
        request: &Request<D::Command>,
    ) -> Result<(String, CommitRequest), ExecError<D::Rejection>> {
        *next_command += 1;
        let command_id = format!("cmd-{}-{}", epoch.0, next_command);
        let now_ms = now_ms();
        let mut context = DecisionContext {
            command_id: &command_id,
            now_ms,
        };
        let decision = self
            .decider
            .decide(state, &request.command, &mut context)
            .map_err(ExecError::Rejected)?;
        let draft = Draft {
            epoch,
            command_id: &command_id,
            key: request.key.as_ref(),
            digest,
            request,
            now_ms,
        };
        let commit = self.commit_request(state, &draft, &decision)?;
        Ok((command_id, commit))
    }

    fn append_staged(
        &self,
        group: &mut CommitGroup<'_>,
        staged: &mut D::State,
        command_id: String,
        commit: &CommitRequest,
    ) -> Step<D::Rejection> {
        let recorded = commit.command.as_ref().expect("submitted command recorded");
        match group.append(commit) {
            Ok(CommitOutcome::Committed { batch }) => {
                let batch = batch.map(|cut| cut.batch_id);
                if let Some(id) = batch
                    && let Err(error) = self.apply_staged(group, staged, id)
                {
                    return Step::Fatal(error);
                }
                Step::Done(Ok(Executed {
                    command_id,
                    status: recorded.status,
                    response: recorded.response.clone(),
                    source: Source::Committed { batch },
                }))
            }
            Ok(CommitOutcome::Duplicate(record)) => Step::Done(stored(record)),
            Err(StoreError::RevisionConflict { .. }) => Step::Conflict,
            Err(StoreError::KeyConflict { .. }) => Step::Done(Err(ExecError::KeyConflict(
                recorded.key.clone().expect("key conflict has key"),
            ))),
            Err(error) if !group.is_poisoned() => Step::Done(Err(store_error(error))),
            Err(error) => Step::Fatal(store_error(error)),
        }
    }

    fn apply_staged(
        &self,
        group: &CommitGroup<'_>,
        state: &mut D::State,
        batch: BatchId,
    ) -> Result<(), ExecError<D::Rejection>> {
        let batch = group.read_batch(batch).map_err(store_error)?;
        self.decider.apply(state, &batch).map_err(ExecError::Apply)
    }
}

enum Step<R: fmt::Debug> {
    Done(ExecResult<R>),
    Full,
    Conflict,
    Fatal(ExecError<R>),
}

enum GroupAttempt<R: fmt::Debug> {
    Done(GroupExecution<R>),
    Conflict,
}

fn failed<R: fmt::Debug + Clone>(count: usize, error: ExecError<R>) -> GroupExecution<R> {
    let reason = error.to_string();
    GroupExecution {
        results: (0..count).map(|_| Err(error.clone())).collect(),
        halted: Some(reason),
    }
}

struct GroupGate<'a, R: fmt::Debug> {
    _lock: MutexGuard<'a, HashMap<CommandKey, Arc<InFlight<R>>>>,
    joined: HashMap<CommandKey, Arc<InFlight<R>>>,
}

fn wait_ready<R: fmt::Debug>(slot: &InFlight<R>) -> Result<(), ExecError<R>> {
    let mut result = slot.result.lock().map_err(|_| ExecError::Poisoned)?;
    while result.is_none() {
        result = slot.ready.wait(result).map_err(|_| ExecError::Poisoned)?;
    }
    Ok(())
}

fn joined_result<R: fmt::Debug + Clone>(
    gate: &GroupGate<'_, R>,
    key: &CommandKey,
    digest: &[u8],
) -> Option<ExecResult<R>> {
    let slot = gate.joined.get(key)?;
    if slot.digest != digest {
        return Some(Err(ExecError::KeyConflict(key.clone())));
    }
    // These slots finished before the physical transaction opened. Never wait on a leader here.
    let result = match slot.result.lock() {
        Ok(result) => result
            .as_ref()
            .cloned()
            .unwrap_or(Err(ExecError::Abandoned)),
        Err(_) => Err(ExecError::Poisoned),
    };
    Some(result.map(|executed| Executed {
        source: Source::Joined,
        ..executed
    }))
}
