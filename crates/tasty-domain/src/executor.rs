//! decider 공용 최소 command executor.
//!
//! 흐름: 재시도 키 조회(대상 해소 전) → 새 요청만 decide → 명령·이벤트·effect를 한 transaction으로
//! 확정 → 확정된 batch를 메모리 상태에 적용 → 응답. 확정이 실패하면 상태를 바꾸지 않고 응답하지
//! 않는다. 같은 프로세스에서 진행 중인 같은 키는 첫 실행의 결과를 기다려 함께 받는다.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use tasty_event_store::{
    BatchId, CommandKey, CommandLookup, CommandRecord, CommandStatus, CommitOutcome, CommitRequest,
    EventStore, ExpectedRevision, NewCommand, NewEffect, NewEvent, OpaquePayload, PayloadRef,
    Revision, StoreError, StoredBatch, StreamAppend, StreamId, WriterEpoch,
};

use crate::ids::IdSupplier;

/// revision 충돌 뒤 상태를 다시 읽고 decide를 다시 시도하는 최대 횟수(첫 시도 포함).
pub const MAX_DECIDE_ATTEMPTS: u32 = 3;

/// decide에 주는 입력. 새 ID와 시각은 여기서만 받아 decide를 결정적으로 유지한다.
pub struct DecisionContext<'a> {
    pub ids: &'a mut dyn IdSupplier,
    pub command_id: &'a str,
    pub now_ms: u64,
}

/// decide 결과. 이벤트·effect·명령 기록이 한 transaction으로 확정된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision<E> {
    pub events: Vec<E>,
    pub effects: Vec<NewEffect>,
    /// 최초 해소한 대상·입력. 재요청은 이 기록을 쓰고 대상을 다시 해소하지 않는다.
    pub resolved: Vec<u8>,
    pub response: Vec<u8>,
}

/// 한 stream의 상태·명령·이벤트를 정하는 규칙.
pub trait Decider: Send + Sync {
    type State: Send;
    type Command;
    type Event;
    /// 도메인 거절. 저장하지 않으며 같은 키의 재요청은 다시 decide한다.
    type Rejection: Clone + fmt::Debug + Send;

    fn stream(&self) -> StreamId;
    /// 상태에 마지막으로 적용한 이 stream의 revision.
    fn revision(&self, state: &Self::State) -> Option<Revision>;
    /// 같은 키의 재요청이 원래 요청과 같은지 판정하는 값.
    fn request_digest(&self, command: &Self::Command) -> Vec<u8>;
    fn decide(
        &self,
        state: &Self::State,
        command: &Self::Command,
        ctx: &mut DecisionContext<'_>,
    ) -> Result<Decision<Self::Event>, Self::Rejection>;
    fn encode(&self, event: &Self::Event) -> Result<OpaquePayload, String>;
    /// 이벤트가 참조하는 불변 payload. 같은 transaction에서 pin된다.
    fn payload_refs(&self, _event: &Self::Event) -> Vec<PayloadRef> {
        Vec::new()
    }
    fn evolve(&self, state: &mut Self::State, batch: &StoredBatch) -> Result<(), String>;
    fn load(&self, store: &EventStore) -> Result<Self::State, String>;
}

/// 실행 요청.
#[derive(Debug, Clone)]
pub struct Request<C> {
    pub key: Option<CommandKey>,
    pub actor: String,
    pub origin: String,
    pub command: C,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// 이번 호출이 decide하고 확정했다. 이벤트가 있었으면 batch가 있다.
    Committed { batch: Option<BatchId> },
    /// 같은 키·같은 요청의 저장된 결과다. decide하지 않았다.
    Stored,
    /// 같은 프로세스에서 진행 중이던 첫 실행에 합류했다.
    Joined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub response: Vec<u8>,
    pub source: Source,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecError<R: fmt::Debug> {
    #[error("rejected: {0:?}")]
    Rejected(R),

    #[error("command key ({}, {}) is bound to a different request", .0.caller_scope, .0.idempotency_key)]
    KeyConflict(CommandKey),

    #[error(transparent)]
    Store(Arc<StoreError>),

    #[error("writing stopped because the writer lock or epoch was lost")]
    Halted,

    #[error("state could not be applied: {0}")]
    Apply(String),

    #[error("revision conflict persisted after {0} attempts")]
    RetriesExhausted(u32),

    #[error("stored command {command_id} is {status:?} without a response")]
    NoStoredResponse {
        command_id: String,
        status: CommandStatus,
    },

    #[error("the first execution of this key ended without a result")]
    Abandoned,

    #[error("executor lock is poisoned")]
    Poisoned,
}

type ExecResult<R> = Result<Executed, ExecError<R>>;

pub(crate) struct Inner<S> {
    pub(crate) store: EventStore,
    pub(crate) epoch: WriterEpoch,
    pub(crate) state: S,
    ids: Box<dyn IdSupplier + Send>,
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

pub struct Executor<D: Decider> {
    decider: D,
    pub(crate) inner: Mutex<Inner<D::State>>,
    in_flight: Mutex<HashMap<CommandKey, Arc<InFlight<D::Rejection>>>>,
}

impl<D: Decider> Executor<D> {
    /// writer 잠금을 얻고 저장소에서 상태를 읽는다.
    pub fn open(
        decider: D,
        mut store: EventStore,
        ids: Box<dyn IdSupplier + Send>,
    ) -> Result<Self, ExecError<D::Rejection>> {
        let epoch = store.acquire_writer().map_err(store_error)?;
        let state = decider.load(&store).map_err(ExecError::Apply)?;
        Ok(Self {
            decider,
            inner: Mutex::new(Inner {
                store,
                epoch,
                state,
                ids,
                next_command: 0,
                halted: false,
                stale: false,
            }),
            in_flight: Mutex::new(HashMap::new()),
        })
    }

    /// 현재 메모리 상태를 읽는다.
    pub fn with_state<T>(
        &self,
        read: impl FnOnce(&D::State) -> T,
    ) -> Result<T, ExecError<D::Rejection>> {
        Ok(read(&self.lock_inner()?.state))
    }

    pub fn execute(&self, request: &Request<D::Command>) -> ExecResult<D::Rejection> {
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
                CommandLookup::Hit(record) => return stored(record),
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
            ids: inner.ids.as_mut(),
            command_id: &command_id,
            now_ms,
        };
        let decision = self
            .decider
            .decide(&inner.state, &request.command, &mut ctx)
            .map_err(ExecError::Rejected)?;
        let expected = self
            .decider
            .revision(&inner.state)
            .map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact);
        let draft = Draft {
            epoch: inner.epoch,
            command_id: &command_id,
            key,
            digest,
            request,
            now_ms,
            expected,
        };
        let commit = self.commit_request(&draft, &decision)?;
        match inner.store.commit(&commit) {
            Ok(CommitOutcome::Committed { batch }) => {
                let batch = batch.map(|cut| cut.batch_id);
                if let Some(batch_id) = batch {
                    self.apply(inner, batch_id)?;
                }
                Ok(Attempt::Done(Executed {
                    response: decision.response,
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
        draft: &Draft<'_, D::Command>,
        decision: &Decision<D::Event>,
    ) -> Result<CommitRequest, ExecError<D::Rejection>> {
        let Draft {
            epoch,
            command_id,
            key,
            digest,
            request,
            now_ms,
            expected,
        } = *draft;
        let mut events = Vec::with_capacity(decision.events.len());
        for (index, event) in decision.events.iter().enumerate() {
            events.push(NewEvent {
                event_id: format!("{command_id}/{index}"),
                payload: self.decider.encode(event).map_err(ExecError::Apply)?,
                recorded_at_ms: now_ms,
                causation_id: None,
                actor: request.actor.clone(),
                origin: request.origin.clone(),
                payload_refs: self.decider.payload_refs(event),
            });
        }
        let mut commit = CommitRequest::new(epoch);
        commit.command = Some(NewCommand {
            command_id: command_id.to_owned(),
            key: key.cloned(),
            request_digest: digest.to_vec(),
            resolved: decision.resolved.clone(),
            status: CommandStatus::Completed,
            response: Some(decision.response.clone()),
        });
        if !events.is_empty() {
            commit.appends.push(StreamAppend {
                stream_id: self.decider.stream(),
                expected,
                events,
            });
        }
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
            Ok(batch) => self.decider.evolve(&mut inner.state, &batch),
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
    expected: ExpectedRevision,
}

enum Attempt {
    Done(Executed),
    Conflict,
}

/// 첫 실행의 결과를 합류한 호출에 전달하고 진행 중 목록에서 지운다.
/// 결과 없이 끝나도(panic 포함) 기다리는 호출이 멈추지 않도록 drop에서 정리한다.
struct Lead<'a, D: Decider> {
    executor: &'a Executor<D>,
    key: &'a CommandKey,
    slot: Arc<InFlight<D::Rejection>>,
    done: bool,
}

impl<D: Decider> Lead<'_, D> {
    fn finish(&mut self, result: ExecResult<D::Rejection>) {
        self.done = true;
        self.publish(result);
    }

    // 두 잠금은 사용자 코드를 부르지 않고 짧게만 잡으므로 poison되지 않는다.
    // 그래도 poison이면 목록 정리나 결과 기록을 건너뛰고, 기다리는 호출은 poison 오류를 받는다.
    fn publish(&self, result: ExecResult<D::Rejection>) {
        if let Ok(mut map) = self.executor.in_flight.lock() {
            map.remove(self.key);
        }
        if let Ok(mut slot) = self.slot.result.lock() {
            *slot = Some(result);
        }
        self.slot.ready.notify_all();
    }
}

impl<D: Decider> Drop for Lead<'_, D> {
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
    match record.response {
        Some(response) => Ok(Executed {
            response,
            source: Source::Stored,
        }),
        None => Err(ExecError::NoStoredResponse {
            command_id: record.command_id,
            status: record.status,
        }),
    }
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
