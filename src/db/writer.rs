//! state.db 전담 writer. worker 한 개가 연결을 소유하고 최근 파일 저장과 튜토리얼 진행 저장을
//! 접수 순서대로 묶어 SQLite transaction 하나로 commit한다.
//!
//! 접수·수집·예산·재시도·종료 규칙은 `tasty_utils::batch_writer`가 정하고 이 모듈은 요청 종류와
//! 묶음 실행을 정한다. 계약 전체는 docs/design/systems/storage-contract.md의 "저장 실행기" 절에 있다.
//!
//! - 최근 파일은 종류별 전체 목록(snapshot)이다. 같은 묶음 안의 같은 종류는 뒤 목록만 저장한다.
//! - 튜토리얼 읽기는 barrier다. 앞서 접수한 저장이 확정된 뒤 혼자 읽는다.
//! - 튜토리얼 저장은 View별 버전 체인을 묶음 안의 작업 사본으로 잇고, commit이 성공한 뒤에만 그
//!   사본을 채택한다. busy·저장 실패·결과 불명·panic이면 사본을 버린다.
//! - 저장 실패는 묶음 전체를 되돌린다. CAS가 맞지 않은 튜토리얼 저장은 실패가 아니라 결과다.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Weak};
use std::time::Instant;

use rusqlite::Connection;
use tasty_utils::batch_writer::{
    self, Clock, Completion, Execution, Executor, Handle, Limits, RequestReceipt, SubmitError,
    WakeLatch, Writer,
};

use crate::recent_files::{RecentFiles, RecentOutcome, RecentSink, RecentSnapshot};
use crate::store::tutorial_progress::Record;

/// 튜토리얼 요청의 성공 결과.
#[derive(Debug)]
pub(crate) enum Reply {
    Loaded(Vec<Record>),
    Saved { record: Record, updated: bool },
}

/// 튜토리얼 재개 위치 저장. `expected`는 View가 마지막으로 확인한 row_version이다.
#[derive(Debug, Clone)]
pub(crate) struct Save {
    pub(crate) topic: &'static str,
    pub(crate) revision: u32,
    pub(crate) step: &'static str,
    pub(crate) completed: bool,
    pub(crate) expected: i64,
}

/// writer에 넣는 요청. 튜토리얼 요청의 `owner`는 View 식별자이며 요청이 가지고 있으므로 View가
/// 닫혀도 접수한 저장은 실행된다.
pub(crate) enum StateRequest {
    Recent {
        key: String,
        snapshot: RecentSnapshot,
    },
    TutorialLoad {
        owner: Arc<()>,
        topics: Vec<(&'static str, u32)>,
    },
    TutorialSave {
        owner: Arc<()>,
        save: Save,
    },
}

impl StateRequest {
    pub(crate) fn recent(snapshot: RecentSnapshot) -> Self {
        Self::Recent {
            key: format!("recent:{}", snapshot.kind),
            snapshot,
        }
    }

    fn writes(&self) -> bool {
        !matches!(self, Self::TutorialLoad { .. })
    }
}

impl batch_writer::Request for StateRequest {
    fn bytes(&self) -> usize {
        match self {
            Self::Recent { snapshot, .. } => {
                snapshot.kind.len()
                    + snapshot
                        .entries
                        .iter()
                        .map(|e| e.path.len() + std::mem::size_of::<i64>())
                        .sum::<usize>()
            }
            Self::TutorialLoad { topics, .. } => topics.iter().map(|(t, _)| t.len() + 4).sum(),
            Self::TutorialSave { save, .. } => save.topic.len() + save.step.len() + 16,
        }
    }

    fn barrier(&self) -> bool {
        matches!(self, Self::TutorialLoad { .. })
    }

    fn snapshot_key(&self) -> Option<&str> {
        match self {
            Self::Recent { key, .. } => Some(key),
            _ => None,
        }
    }
}

/// 요청 하나의 성공 결과.
#[derive(Debug)]
pub(crate) enum StateOutput {
    Recent,
    Tutorial(Reply),
}

/// 요청이 효과를 내지 못한 이유. 원래 SQLite 오류 문구를 싣는다.
#[derive(Debug, Clone)]
pub(crate) struct StateError(Arc<str>);

impl StateError {
    fn of(error: &rusqlite::Error) -> Self {
        Self(error.to_string().into())
    }

    fn panicked() -> Self {
        Self("state writer request panicked".into())
    }
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) type StateCompletion = Completion<StateOutput, StateError>;
pub(crate) type StateQueue = Handle<StateExecutor>;

type Versions = BTreeMap<(&'static str, u32), (i64, i64)>;

/// View별로 자기 저장이 성공한 버전 구간. 빠른 연속 저장이 마지막 확인 버전을 다시 싣고 와도
/// 자기 성공만 따라 잇고 다른 View의 변경은 채택하지 않는다.
#[derive(Default, Clone)]
pub(crate) struct Chains(BTreeMap<usize, (Weak<()>, Versions)>);

impl Chains {
    fn versions(&mut self, owner: &Arc<()>) -> &mut Versions {
        self.0.retain(|_, (owner, _)| owner.strong_count() != 0);
        &mut self
            .0
            .entry(Arc::as_ptr(owner) as usize)
            .or_insert_with(|| (Arc::downgrade(owner), BTreeMap::new()))
            .1
    }

    fn load(
        &mut self,
        conn: &Connection,
        owner: &Arc<()>,
        topics: &[(&'static str, u32)],
    ) -> rusqlite::Result<Reply> {
        let records = topics
            .iter()
            .map(|(topic, revision)| crate::store::tutorial_progress::load(conn, topic, *revision))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.versions(owner).clear();
        Ok(Reply::Loaded(records))
    }

    fn save(&mut self, tx: &Connection, owner: &Arc<()>, save: &Save) -> rusqlite::Result<Reply> {
        let versions = self.versions(owner);
        let key = (save.topic, save.revision);
        let chain = versions
            .get(&key)
            .copied()
            .filter(|(base, last)| save.expected >= *base && save.expected <= *last);
        let expected = chain.map_or(save.expected, |(_, last)| last);
        let (record, updated) = crate::store::tutorial_progress::save(
            tx,
            save.topic,
            save.revision,
            save.step,
            save.completed,
            expected,
        )?;
        if updated {
            versions.insert(
                key,
                (
                    chain.map_or(save.expected, |(base, _)| base),
                    record.version,
                ),
            );
        } else {
            versions.remove(&key);
        }
        Ok(Reply::Saved { record, updated })
    }
}

/// state.db 묶음 실행기. 연결과 튜토리얼 버전 체인을 소유한다.
pub(crate) struct StateExecutor {
    conn: Connection,
    chains: Chains,
    latency: Arc<tasty_memory::DbLatencyStats>,
}

impl StateExecutor {
    pub(crate) fn new(conn: Connection, latency: Arc<tasty_memory::DbLatencyStats>) -> Self {
        Self {
            conn,
            chains: Chains::default(),
            latency,
        }
    }
}

fn is_busy(error: &rusqlite::Error) -> bool {
    tasty_memory::StorageFailure::classify(error) == tasty_memory::StorageFailure::Busy
}

/// 아무것도 확정하지 않은 오류. busy는 다시 실행하고 나머지는 묶음 전체의 실패다.
fn uncommitted(error: &rusqlite::Error) -> Execution<StateOutput, StateError> {
    if is_busy(error) {
        Execution::Busy(StateError::of(error))
    } else {
        Execution::Failed(StateError::of(error))
    }
}

/// SQLite가 오류로 이미 transaction을 끝냈으면 되돌릴 것이 없다.
fn roll_back(tx: rusqlite::Transaction<'_>) {
    if tx.is_autocommit() {
        return;
    }
    if let Err(error) = tx.rollback() {
        tracing::warn!(%error, "state.db batch rollback failed");
    }
}

fn run_one(
    tx: &Connection,
    chains: &mut Chains,
    request: &StateRequest,
) -> rusqlite::Result<StateOutput> {
    match request {
        StateRequest::Recent { snapshot, .. } => {
            crate::recent_files::write_snapshot(tx, snapshot).map(|()| StateOutput::Recent)
        }
        StateRequest::TutorialLoad { owner, topics } => {
            chains.load(tx, owner, topics).map(StateOutput::Tutorial)
        }
        StateRequest::TutorialSave { owner, save } => {
            chains.save(tx, owner, save).map(StateOutput::Tutorial)
        }
    }
}

impl Executor for StateExecutor {
    type Request = StateRequest;
    type Output = StateOutput;
    type Error = StateError;

    fn execute(&mut self, batch: &mut [StateRequest]) -> Execution<StateOutput, StateError> {
        let mut chains = self.chains.clone();
        if let [StateRequest::TutorialLoad { owner, topics }] = batch {
            return match catch_unwind(AssertUnwindSafe(|| chains.load(&self.conn, owner, topics))) {
                Err(_) => Execution::Poisoned {
                    index: 0,
                    error: StateError::panicked(),
                },
                Ok(Ok(reply)) => {
                    self.chains = chains;
                    Execution::Committed(vec![Ok(StateOutput::Tutorial(reply))])
                }
                Ok(Err(error)) => uncommitted(&error),
            };
        }
        let tx = match self.conn.transaction() {
            Ok(tx) => tx,
            Err(error) => return uncommitted(&error),
        };
        let mut results = Vec::with_capacity(batch.len());
        for (index, request) in batch.iter().enumerate() {
            match catch_unwind(AssertUnwindSafe(|| run_one(&tx, &mut chains, request))) {
                Err(_) => {
                    roll_back(tx);
                    return Execution::Poisoned {
                        index,
                        error: StateError::panicked(),
                    };
                }
                Ok(Err(error)) => {
                    roll_back(tx);
                    return uncommitted(&error);
                }
                Ok(Ok(output)) => results.push(Ok(output)),
            }
        }
        let started = Instant::now();
        match tx.commit() {
            Ok(()) => {
                self.latency.record_commit(started.elapsed());
                let writes = batch.iter().filter(|r| r.writes()).count() as u64;
                self.latency.record_commit_writes(writes);
                self.chains = chains;
                Execution::Committed(results)
            }
            // commit의 busy는 transaction이 남은 채 돌아와 drop이 되돌린다.
            Err(error) if is_busy(&error) => Execution::Busy(StateError::of(&error)),
            Err(error) => Execution::Unknown(StateError::of(&error)),
        }
    }

    fn output_bytes(output: &StateOutput) -> usize {
        match output {
            StateOutput::Tutorial(Reply::Loaded(records)) => records
                .iter()
                .map(|r| {
                    std::mem::size_of::<Record>() + r.resume_step.as_ref().map_or(0, String::len)
                })
                .sum(),
            StateOutput::Tutorial(Reply::Saved { record, .. }) => {
                record.resume_step.as_ref().map_or(0, String::len)
            }
            StateOutput::Recent => 0,
        }
    }
}

/// 최근 파일 저장 결과. 대체된 요청은 대체한 요청이 결과를 따로 알린다.
fn recent_outcome(completion: &StateCompletion) -> RecentOutcome {
    match completion {
        Completion::Done(Ok(_)) => RecentOutcome::Committed,
        Completion::Superseded { .. } => RecentOutcome::Superseded,
        Completion::Done(Err(e)) | Completion::Failed(e) => {
            RecentOutcome::NotCommitted(e.to_string())
        }
        Completion::Unknown(e) => RecentOutcome::NotCommitted(format!("outcome unknown: {e}")),
        Completion::Lost => {
            RecentOutcome::NotCommitted("state writer ended before reporting a result".into())
        }
    }
}

/// 튜토리얼 receipt 결과를 View가 쓰는 결과로 바꾼다.
pub(crate) fn tutorial_reply(completion: StateCompletion) -> Result<Reply, String> {
    match completion {
        Completion::Done(Ok(StateOutput::Tutorial(reply))) => Ok(reply),
        Completion::Done(Ok(StateOutput::Recent)) => Err("unexpected recent files result".into()),
        Completion::Done(Err(e)) | Completion::Failed(e) => Err(e.to_string()),
        Completion::Unknown(e) => Err(format!("outcome unknown: {e}")),
        Completion::Superseded { .. } => Err("tutorial request was superseded".into()),
        Completion::Lost => Err("state writer ended before reporting a result".into()),
    }
}

/// 튜토리얼 요청 하나의 receipt. 기다리지 않고 꺼낸다.
pub(crate) struct TutorialReceipt(RequestReceipt<StateExecutor>);

impl TutorialReceipt {
    pub(crate) fn try_take(&self) -> Option<Result<Reply, String>> {
        self.0.try_take().map(tutorial_reply)
    }
}

pub(crate) fn submit_tutorial(
    queue: &StateQueue,
    request: StateRequest,
) -> Result<TutorialReceipt, SubmitError> {
    queue.submit(request).map(TutorialReceipt)
}

/// 최근 파일 캐시가 writer로 저장하는 쪽. 결과는 메인 루프가 `RecentFiles::settle`로 거둔다.
struct WriterRecentSink {
    queue: StateQueue,
    pending: Vec<(String, u64, RequestReceipt<StateExecutor>)>,
}

impl RecentSink for WriterRecentSink {
    fn submit(&mut self, snapshot: RecentSnapshot) -> Result<(), String> {
        let (kind, revision) = (snapshot.kind.clone(), snapshot.revision);
        let receipt = self
            .queue
            .submit(StateRequest::recent(snapshot))
            .map_err(|e| e.to_string())?;
        self.pending.push((kind, revision, receipt));
        Ok(())
    }

    fn reap(&mut self) -> Vec<(String, u64, RecentOutcome)> {
        let mut done = Vec::new();
        self.pending
            .retain(|(kind, revision, receipt)| match receipt.try_take() {
                Some(completion) => {
                    done.push((kind.clone(), *revision, recent_outcome(&completion)));
                    false
                }
                None => true,
            });
        done
    }

    #[cfg(test)]
    fn pending(&self) -> usize {
        self.pending.len()
    }
}

/// AppServices가 소유하는 writer와 그 진단 사본.
pub(crate) struct StateWriterParts {
    pub(crate) writer: Writer<StateExecutor>,
    pub(crate) recent: RecentFiles,
    pub(crate) wake: Arc<WakeLatch>,
    /// 연결을 열 때 확인한 pragma. 진단이 연결에 닿지 않게 한다.
    pub(crate) pragmas: tasty_memory::pragma::AppliedPragmas,
    /// 성공한 쓰기 commit 계측. memory.db 계측과 따로 센다.
    pub(crate) latency: Arc<tasty_memory::DbLatencyStats>,
    end_reported: bool,
    drain_reported: bool,
}

impl StateWriterParts {
    /// 메인 루프 회차마다 부른다. 깨우기를 확인한 뒤 최근 파일 저장 결과를 거두고, 종료를 요청하지
    /// 않았는데 worker가 끝났으면 한 번 알린다. 튜토리얼 결과는 각 View가 거둔다.
    pub(crate) fn poll(&mut self) {
        self.wake.acknowledge();
        self.recent.settle();
        if !self.end_reported && self.writer.worker_ended_unexpectedly() {
            self.end_reported = true;
            tracing::error!("state writer ended without a shutdown request");
        }
    }

    /// 접수를 멈추고 마지막 flush 뒤 worker가 끝나게 하고 남은 worker 수를 돌려준다. 기다리지 않는다.
    /// worker가 끝나면 남은 결과를 거두고, 화면에 보였지만 저장을 확정하지 못한 최근 목록을 한 번 알린다.
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        let remaining = self.writer.poll_shutdown();
        self.recent.settle();
        if remaining == 0 && !self.drain_reported {
            self.drain_reported = true;
            for (kind, revisions) in self.recent.unsaved() {
                tracing::warn!(
                    kind,
                    visible = revisions.visible,
                    committed = revisions.committed,
                    "recent files shown in this run were not all saved"
                );
            }
        }
        remaining
    }
}

/// 창과 파일 열기 경로가 인자 없이 닿는 공유 핸들. 연결과 worker는 들어 있지 않다.
#[derive(Clone)]
pub(crate) struct SharedState {
    pub(crate) recent: RecentFiles,
    pub(crate) queue: StateQueue,
}

/// 부팅 준비 단계에서 최근 목록을 한 번 읽고 연결을 writer로 넘긴다.
pub(crate) fn start(
    db: super::Db,
    limits: Limits,
    clock: Arc<dyn Clock>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> std::io::Result<(StateWriterParts, SharedState)> {
    let super::Db {
        mut conn,
        applied_pragmas,
    } = db;
    let initial = crate::recent_files::load_at_boot(&mut conn);
    let latency = Arc::new(tasty_memory::DbLatencyStats::default());
    let wake = WakeLatch::new(wake);
    let writer = Writer::spawn(
        "state-writer",
        StateExecutor::new(conn, latency.clone()),
        limits,
        clock,
        wake.notifier(),
    )?;
    let queue = writer.handle();
    let recent = RecentFiles::with_sink(
        initial,
        Box::new(WriterRecentSink {
            queue: queue.clone(),
            pending: Vec::new(),
        }),
    );
    let shared = SharedState {
        recent: recent.clone(),
        queue,
    };
    Ok((
        StateWriterParts {
            writer,
            recent,
            wake,
            pragmas: applied_pragmas,
            latency,
            end_reported: false,
            drain_reported: false,
        },
        shared,
    ))
}

#[cfg(test)]
mod tests;
