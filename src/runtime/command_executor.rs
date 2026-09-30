//! decider 공용 최소 command executor.
//!
//! 흐름: 재시도 키 조회(대상 해소 전) → 새 요청만 decide → 명령·이벤트·effect를 한 transaction으로
//! 확정 → 확정된 batch를 저장소에서 다시 읽어 메모리 상태에 적용 → 응답. 확정이 실패하면 상태를
//! 바꾸지 않고 응답하지 않는다. 같은 프로세스에서 진행 중인 같은 키는 첫 실행의 결과를 기다려 함께
//! 받는다. writer 잠금을 잃거나 fencing되면 이후 쓰기를 멈춘다.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use tasty_domain::{Decider, Decision, DecisionContext};
use tasty_event_store::{
    BatchId, CommandKey, CommandLookup, CommandRecord, CommandStatus, CommandUpdate, CommitOutcome,
    CommitRequest, EffectTransition, EventStore, ExpectedRevision, NewCommand, NewEffect, NewEvent,
    OpaquePayload, PayloadRef, StoreError, StoredBatch, StreamAppend, StreamId, WriterEpoch,
};

/// revision 충돌 뒤 상태를 다시 읽고 decide를 다시 시도하는 최대 횟수(첫 시도 포함).
pub(crate) const MAX_DECIDE_ATTEMPTS: u32 = 3;

/// 순수 [`Decider`]를 journal에 연결하는 부분. 저장 봉투·stream·재구성은 여기서 정한다.
pub(crate) trait JournalDecider:
    Decider<Effect = NewEffect, State: Send, Rejection: Send> + Send + Sync
{
    fn event_stream(&self, event: &Self::Event) -> StreamId;
    fn stream_revision(&self, state: &Self::State, stream: &StreamId) -> Option<u64>;
    fn record(
        &self,
        state: &Self::State,
        command: &Self::Command,
        decision: &Decision<Self::Event, NewEffect>,
    ) -> CommandRecordPlan;
    fn encode(&self, event: &Self::Event) -> Result<OpaquePayload, String>;
    /// 이벤트가 참조하는 불변 payload. 같은 transaction에서 pin된다.
    fn payload_refs(&self, _event: &Self::Event) -> Vec<PayloadRef> {
        Vec::new()
    }
    /// 확정된 batch를 상태에 적용한다. 실패하면 상태는 그대로여야 한다.
    fn apply(&self, state: &mut Self::State, batch: &StoredBatch) -> Result<(), String>;
    fn load(&self, store: &EventStore) -> Result<Self::State, String>;
}

/// 실행 요청.
#[derive(Debug, Clone)]
pub(crate) struct Request<C> {
    pub(crate) key: Option<CommandKey>,
    pub(crate) actor: String,
    pub(crate) origin: String,
    /// An effect/result command keeps the identity which caused it. Initial events name their command.
    pub(crate) causation_id: Option<String>,
    pub(crate) command: C,
}

/// Progress and result/attempt updates are committed with the domain facts which justify them.
pub(crate) struct CommandRecordPlan {
    pub(crate) status: CommandStatus,
    pub(crate) response: Option<Vec<u8>>,
    pub(crate) command_updates: Vec<CommandUpdate>,
    pub(crate) effect_transitions: Vec<EffectTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// 이번 호출이 decide하고 확정했다. 이벤트가 있었으면 batch가 있다.
    Committed { batch: Option<BatchId> },
    /// 같은 키·같은 요청의 저장된 결과다. decide하지 않았다.
    Stored,
    /// 같은 프로세스에서 진행 중이던 첫 실행에 합류했다.
    Joined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Executed {
    pub(crate) command_id: String,
    pub(crate) status: CommandStatus,
    pub(crate) response: Option<Vec<u8>>,
    pub(crate) source: Source,
}

#[derive(Debug, Clone)]
pub(crate) enum ExecError<R: fmt::Debug> {
    Rejected(R),
    /// 같은 재시도 키가 다른 요청에 쓰였거나 쓰이는 중이다.
    KeyConflict(CommandKey),
    Store(Arc<StoreError>),
    /// writer 잠금이나 세대를 잃어 쓰기를 멈췄다.
    Halted,
    /// 상태 적용·재구성·인코딩 실패.
    Apply(String),
    /// revision 충돌이 정해진 횟수 동안 계속됐다.
    RetriesExhausted(u32),
    NoStoredResponse {
        command_id: String,
        status: CommandStatus,
    },
    /// 합류한 첫 실행이 결과 없이 끝났다.
    Abandoned,
    Poisoned,
}

impl<R: fmt::Debug> fmt::Display for ExecError<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(r) => write!(f, "rejected: {r:?}"),
            Self::KeyConflict(key) => write!(
                f,
                "command key ({}, {}) is bound to a different request",
                key.caller_scope, key.idempotency_key
            ),
            Self::Store(error) => write!(f, "{error}"),
            Self::Halted => {
                f.write_str("writing stopped because the writer lock or epoch was lost")
            }
            Self::Apply(reason) => write!(f, "state could not be applied: {reason}"),
            Self::RetriesExhausted(n) => {
                write!(f, "revision conflict persisted after {n} attempts")
            }
            Self::NoStoredResponse { command_id, status } => {
                write!(
                    f,
                    "stored command {command_id} is {status:?} without a response"
                )
            }
            Self::Abandoned => {
                f.write_str("the first execution of this key ended without a result")
            }
            Self::Poisoned => f.write_str("executor lock is poisoned"),
        }
    }
}

type ExecResult<R> = Result<Executed, ExecError<R>>;

pub(crate) struct Inner<S> {
    pub(crate) store: EventStore,
    pub(crate) epoch: WriterEpoch,
    pub(crate) state: S,
    next_command: u64,
    /// 쓰기 권한을 잃었다. 이후 요청은 저장소에 닿지 않는다.
    halted: bool,
    /// 확정 뒤 적용에 실패했다. 다음 요청 전에 저장소에서 다시 읽는다.
    stale: bool,
}

/// 진행 중인 같은 키의 결과를 기다리는 자리.
struct InFlight<R: fmt::Debug> {
    digest: Vec<u8>,
    result: Mutex<Option<ExecResult<R>>>,
    ready: Condvar,
}

pub(crate) struct Executor<D: JournalDecider> {
    decider: D,
    pub(crate) inner: Mutex<Inner<D::State>>,
    in_flight: Mutex<HashMap<CommandKey, Arc<InFlight<D::Rejection>>>>,
}

impl<D: JournalDecider> Executor<D> {
    /// writer 잠금을 얻고 저장소에서 상태를 읽는다.
    pub(crate) fn open(decider: D, mut store: EventStore) -> Result<Self, ExecError<D::Rejection>> {
        let epoch = store.acquire_writer().map_err(store_error)?;
        let state = decider.load(&store).map_err(ExecError::Apply)?;
        Ok(Self {
            decider,
            inner: Mutex::new(Inner {
                store,
                epoch,
                state,
                next_command: 0,
                halted: false,
                stale: false,
            }),
            in_flight: Mutex::new(HashMap::new()),
        })
    }

    /// 현재 메모리 상태를 읽는다.
    pub(crate) fn with_state<T>(
        &self,
        read: impl FnOnce(&D::State) -> T,
    ) -> Result<T, ExecError<D::Rejection>> {
        let mut inner = self.lock_inner()?;
        if inner.stale {
            self.reload(&mut inner)?;
        }
        Ok(read(&inner.state))
    }

    /// Import commits already validated structural facts through the same store. Mark the cached
    /// model stale before reloading, so a reload failure cannot expose the old projection as current.
    pub(crate) fn reload_committed(&self) -> Result<(), ExecError<D::Rejection>> {
        let mut inner = self.lock_inner()?;
        inner.stale = true;
        self.reload(&mut inner)
    }

    pub(crate) fn execute(&self, request: &Request<D::Command>) -> ExecResult<D::Rejection> {
        let digest = self.decider.request_digest(&request.command);
        let Some(key) = &request.key else {
            return self.run(None, &digest, request);
        };
        let slot = match self.join_or_lead(key, &digest)? {
            Role::Follower(slot) => return wait(&slot),
            Role::Leader(slot) => slot,
        };
        let mut lead = Lead {
            executor: self,
            key,
            slot,
            done: false,
        };
        let result = self.run(Some(key), &digest, request);
        lead.finish(result.clone());
        result
    }

    fn join_or_lead(
        &self,
        key: &CommandKey,
        digest: &[u8],
    ) -> Result<Role<D::Rejection>, ExecError<D::Rejection>> {
        let mut map = self.in_flight.lock().map_err(|_| ExecError::Poisoned)?;
        if let Some(existing) = map.get(key) {
            if existing.digest != digest {
                return Err(ExecError::KeyConflict(key.clone()));
            }
            return Ok(Role::Follower(existing.clone()));
        }
        let slot = Arc::new(InFlight {
            digest: digest.to_vec(),
            result: Mutex::new(None),
            ready: Condvar::new(),
        });
        map.insert(key.clone(), slot.clone());
        Ok(Role::Leader(slot))
    }

    fn run(
        &self,
        key: Option<&CommandKey>,
        digest: &[u8],
        request: &Request<D::Command>,
    ) -> ExecResult<D::Rejection> {
        let mut inner = self.lock_inner()?;
        if inner.halted {
            return Err(ExecError::Halted);
        }
        if let Some(key) = key {
            match inner
                .store
                .lookup_command(key, digest)
                .map_err(store_error)?
            {
                CommandLookup::Miss => {}
                CommandLookup::Hit(record) => {
                    // Identity lookup still precedes target resolution, but a stored success
                    // cannot bypass recovery of a previously committed, unapplied batch.
                    if inner.stale {
                        self.reload(&mut inner)?;
                    }
                    return stored(record);
                }
                CommandLookup::DigestMismatch(_) => {
                    return Err(ExecError::KeyConflict(key.clone()));
                }
            }
        }
        if inner.stale {
            self.reload(&mut inner)?;
        }
        for _ in 0..MAX_DECIDE_ATTEMPTS {
            match self.attempt(&mut inner, key, digest, request)? {
                Attempt::Done(executed) => return Ok(executed),
                Attempt::Conflict => self.reload(&mut inner)?,
            }
        }
        Err(ExecError::RetriesExhausted(MAX_DECIDE_ATTEMPTS))
    }

    /// decide 한 번과 그 결정의 확정.
    fn attempt(
        &self,
        inner: &mut Inner<D::State>,
        key: Option<&CommandKey>,
        digest: &[u8],
        request: &Request<D::Command>,
    ) -> Result<Attempt, ExecError<D::Rejection>> {
        inner.next_command += 1;
        let command_id = format!("cmd-{}-{}", inner.epoch.0, inner.next_command);
        let now_ms = now_ms();
        let mut ctx = DecisionContext {
            command_id: &command_id,
            now_ms,
        };
        let decision = self
            .decider
            .decide(&inner.state, &request.command, &mut ctx)
            .map_err(ExecError::Rejected)?;
        let draft = Draft {
            epoch: inner.epoch,
            command_id: &command_id,
            key,
            digest,
            request,
            now_ms,
        };
        let commit = self.commit_request(&inner.state, &draft, &decision)?;
        let recorded = commit
            .command
            .as_ref()
            .expect("a submitted command is recorded");
        let status = recorded.status;
        let response = recorded.response.clone();
        match inner.store.commit(&commit) {
            Ok(CommitOutcome::Committed { batch }) => {
                let batch = batch.map(|cut| cut.batch_id);
                if let Some(batch_id) = batch {
                    self.apply(inner, batch_id)?;
                }
                Ok(Attempt::Done(Executed {
                    command_id,
                    status,
                    response,
                    source: Source::Committed { batch },
                }))
            }
            Ok(CommitOutcome::Duplicate(record)) => stored(record).map(Attempt::Done),
            Err(StoreError::RevisionConflict { .. }) => Ok(Attempt::Conflict),
            Err(StoreError::KeyConflict { .. }) => match key {
                Some(key) => Err(ExecError::KeyConflict(key.clone())),
                None => Err(ExecError::Apply("key conflict without a key".to_owned())),
            },
            Err(error) => {
                if loses_writer(&error) {
                    inner.halted = true;
                }
                Err(store_error(error))
            }
        }
    }

    fn commit_request(
        &self,
        state: &D::State,
        draft: &Draft<'_, D::Command>,
        decision: &Decision<D::Event, NewEffect>,
    ) -> Result<CommitRequest, ExecError<D::Rejection>> {
        let Draft {
            epoch,
            command_id,
            key,
            digest,
            request,
            now_ms,
        } = *draft;
        let mut streams: BTreeMap<StreamId, Vec<NewEvent>> = BTreeMap::new();
        for (index, event) in decision.events.iter().enumerate() {
            streams
                .entry(self.decider.event_stream(event))
                .or_default()
                .push(NewEvent {
                    event_id: format!("{command_id}/{index}"),
                    payload: self.decider.encode(event).map_err(ExecError::Apply)?,
                    recorded_at_ms: now_ms,
                    causation_id: Some(
                        request
                            .causation_id
                            .clone()
                            .unwrap_or_else(|| command_id.to_owned()),
                    ),
                    actor: request.actor.clone(),
                    origin: request.origin.clone(),
                    payload_refs: self.decider.payload_refs(event),
                });
        }
        let plan = self.decider.record(state, &request.command, decision);
        let mut commit = CommitRequest::new(epoch);
        commit.command = Some(NewCommand {
            command_id: command_id.to_owned(),
            key: key.cloned(),
            request_digest: digest.to_vec(),
            resolved: decision.resolved.clone(),
            status: plan.status,
            response: plan.response,
        });
        for (stream_id, events) in streams {
            let expected = self
                .decider
                .stream_revision(state, &stream_id)
                .map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact);
            commit.appends.push(StreamAppend {
                stream_id,
                expected,
                events,
            });
        }
        commit.command_updates = plan.command_updates;
        commit.effect_transitions = plan.effect_transitions;
        commit.effects = decision.effects.clone();
        Ok(commit)
    }

    /// 확정된 batch를 저장소에서 다시 읽어 적용한다. 실패하면 다음 요청 전에 다시 읽는다.
    fn apply(
        &self,
        inner: &mut Inner<D::State>,
        batch_id: BatchId,
    ) -> Result<(), ExecError<D::Rejection>> {
        let result = match inner.store.read_batch(batch_id) {
            Ok(batch) => self.decider.apply(&mut inner.state, &batch),
            Err(error) => Err(error.to_string()),
        };
        result.map_err(|reason| {
            inner.stale = true;
            ExecError::Apply(reason)
        })
    }

    fn reload(&self, inner: &mut Inner<D::State>) -> Result<(), ExecError<D::Rejection>> {
        inner.state = self.decider.load(&inner.store).map_err(ExecError::Apply)?;
        inner.stale = false;
        Ok(())
    }

    fn lock_inner(&self) -> Result<MutexGuard<'_, Inner<D::State>>, ExecError<D::Rejection>> {
        self.inner.lock().map_err(|_| ExecError::Poisoned)
    }
}

enum Role<R: fmt::Debug> {
    Leader(Arc<InFlight<R>>),
    Follower(Arc<InFlight<R>>),
}

/// 확정 요청을 만들 때 필요한 이번 시도의 값.
struct Draft<'a, C> {
    epoch: WriterEpoch,
    command_id: &'a str,
    key: Option<&'a CommandKey>,
    digest: &'a [u8],
    request: &'a Request<C>,
    now_ms: u64,
}

enum Attempt {
    Done(Executed),
    Conflict,
}

/// 첫 실행의 결과를 합류한 호출에 전달하고 진행 중 목록에서 지운다.
/// 결과 없이 끝나도(panic 포함) 기다리는 호출이 멈추지 않도록 drop에서 정리한다.
struct Lead<'a, D: JournalDecider> {
    executor: &'a Executor<D>,
    key: &'a CommandKey,
    slot: Arc<InFlight<D::Rejection>>,
    done: bool,
}

impl<D: JournalDecider> Lead<'_, D> {
    fn finish(&mut self, result: ExecResult<D::Rejection>) {
        self.done = true;
        self.publish(result);
    }

    // 두 잠금은 사용자 코드를 부르지 않고 짧게만 잡는다. 그래도 poison이면 복구하지 않고 기록한 뒤
    // 해당 정리를 건너뛴다. 기다리는 호출은 poison 오류를 받는다.
    fn publish(&self, result: ExecResult<D::Rejection>) {
        match self.executor.in_flight.lock() {
            Ok(mut map) => {
                map.remove(self.key);
            }
            Err(_) => tracing::error!("command executor in-flight table is poisoned"),
        }
        match self.slot.result.lock() {
            Ok(mut slot) => *slot = Some(result),
            Err(_) => tracing::error!("command executor result slot is poisoned"),
        }
        self.slot.ready.notify_all();
    }
}

impl<D: JournalDecider> Drop for Lead<'_, D> {
    fn drop(&mut self) {
        if !self.done {
            self.publish(Err(ExecError::Abandoned));
        }
    }
}

fn wait<R: Clone + fmt::Debug>(slot: &InFlight<R>) -> ExecResult<R> {
    let mut guard = slot.result.lock().map_err(|_| ExecError::Poisoned)?;
    loop {
        if let Some(result) = guard.as_ref() {
            return result.clone().map(|executed| Executed {
                source: Source::Joined,
                ..executed
            });
        }
        guard = slot.ready.wait(guard).map_err(|_| ExecError::Poisoned)?;
    }
}

fn stored<R: fmt::Debug>(record: CommandRecord) -> ExecResult<R> {
    if record.response.is_none() && record.status.is_terminal() {
        return Err(ExecError::NoStoredResponse {
            command_id: record.command_id,
            status: record.status,
        });
    }
    Ok(Executed {
        command_id: record.command_id,
        status: record.status,
        response: record.response,
        source: Source::Stored,
    })
}

fn store_error<R: fmt::Debug>(error: StoreError) -> ExecError<R> {
    ExecError::Store(Arc::new(error))
}

/// 이 오류 뒤에는 이 executor가 더 쓰지 않는다.
fn loses_writer(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::Fenced { .. }
            | StoreError::NotWriter
            | StoreError::WriterLocked
            | StoreError::WriterLockUnavailable(_)
            | StoreError::JournalArchived
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
