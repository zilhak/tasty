//! 저장 요청을 큐로 받아 전담 worker 한 개가 여러 요청을 묶어 실행하는 실행기.
//!
//! 호출자는 [`Handle::submit`]으로 요청을 넘기고 접수 결과와 [`Receipt`]를 바로 받는다. 접수는 큐
//! 잠금만 잡고 디스크·저장소 잠금·완료 대기를 하지 않는다. 저장소(DB 연결 등)는 [`Executor`]가
//! 소유하고 worker 스레드만 실행한다. 이 모듈은 수집·순서·결과·예산·종료 계약만 정하고 무엇을
//! 어떻게 저장하는지는 Executor가 정한다.
//!
//! - 수집 창: 큐가 비어 있다가 첫 요청이 들어온 시각에 열린다. 창 경과 `max_delay`, 항목 수
//!   `max_items`, 요청 바이트 `max_bytes` 중 먼저 닿은 조건에서 묶음을 실행한다. 한 요청이
//!   `max_bytes`를 넘으면 그 요청만으로 묶음을 만든다. barrier 요청·flush가 뒤에 오면 창을 바로
//!   닫고 그 앞까지만 묶는다. barrier 요청은 혼자 실행한다.
//! - 순서: 요청은 접수 순서(`seq`)대로 실행하고 결과를 전한다. 같은 snapshot 키의 요청은 한 묶음
//!   안에서만 마지막 요청으로 합치고, 대체된 요청은 대체한 요청의 결과를 받는다.
//! - 예산: 접수부터 결과 소비(또는 receipt 버림)까지 항목 수와 바이트를 함께 센다. 넘치면 접수를
//!   바로 거절한다. 호출 스레드가 대신 저장하는 경로는 없다.
//! - 요청별 결과: 요청 사이에 transaction이 없는 Executor는 요청마다 끝난 결과
//!   ([`Execution::Finished`])를 돌려준다. 앞 요청의 확정은 뒤 요청의 실패로 되돌리거나 다시
//!   실행하지 않는다.
//! - 재시도: Executor가 아무것도 확정하지 않았다고 알린 busy만 `retry_for` 안에서 같은 묶음을
//!   다시 실행한다. 그동안 뒤 요청은 기다린다. 결과 불명은 다시 실행하지 않는다.
//! - 종료: 접수 중단(`close`) → 마지막 flush cut → 남은 요청 실행 → worker 종료 순서다.
//!
//! 계약 전체는 docs/design/systems/storage-contract.md의 "저장 실행기" 절에 있다.

mod clock;
mod slot;
mod stats;
mod wake;
mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use clock::{Clock, ManualClock, SystemClock};
pub use slot::Receipt;
pub use stats::{BATCH_SIZE_BUCKETS, WriterStatsSnapshot};
pub use wake::{WakeCounts, WakeLatch};

use crate::poison::recover_mutex;
use slot::{Budget, Slot, Ticket};

static QUEUE_POISONED: AtomicBool = AtomicBool::new(false);
const QUEUE_WHAT: &str = "batch writer queue";

/// 수집·예산·재시도 한도.
#[derive(Debug, Clone)]
pub struct Limits {
    /// 수집 창이 열린 뒤 묶음을 실행하기까지의 최대 대기.
    pub max_delay: Duration,
    /// 한 묶음의 최대 요청 수.
    pub max_items: usize,
    /// 한 묶음의 요청 바이트 상한. 한 요청이 넘으면 그 요청만 실행한다.
    pub max_bytes: usize,
    /// 접수부터 결과 소비까지 잡혀 있을 수 있는 요청 수.
    pub queue_items: usize,
    /// 같은 범위의 바이트(대기·실행·재시도 중에는 요청 바이트, 완료 뒤에는 결과 바이트).
    pub queue_bytes: usize,
    /// busy 재시도를 계속하는 시간. 첫 실행 시각부터 잰다.
    pub retry_for: Duration,
    /// 첫 재시도 대기. 실패할 때마다 두 배로 늘린다.
    pub retry_backoff: Duration,
    pub retry_backoff_max: Duration,
}

impl Default for Limits {
    /// 기본값과 근거는 docs/design/systems/storage-contract.md의 "한도 기본값" 절에 있다.
    fn default() -> Self {
        Self {
            max_delay: Duration::from_millis(2),
            max_items: 256,
            max_bytes: 1024 * 1024,
            queue_items: 4096,
            queue_bytes: 16 * 1024 * 1024,
            retry_for: Duration::from_secs(1),
            retry_backoff: Duration::from_millis(1),
            retry_backoff_max: Duration::from_millis(50),
        }
    }
}

/// 큐에 넣는 요청.
pub trait Request: Send + 'static {
    /// 예산에 셀 요청 크기.
    fn bytes(&self) -> usize;
    /// 앞선 요청이 모두 끝난 뒤 혼자 실행해야 하는 요청(강한 읽기). 수집 창을 그 앞에서 닫는다.
    fn barrier(&self) -> bool {
        false
    }
    /// 전체 snapshot 요청의 대상 키. 같은 묶음 안에서 뒤 요청이 앞 요청을 대체한다.
    fn snapshot_key(&self) -> Option<&str> {
        None
    }
}

/// 묶음 하나를 실행한 결과.
#[derive(Debug)]
pub enum Execution<O, E> {
    /// 확정했다. 요청별 결과를 요청 순서로 담는다. `Err`는 그 요청만의 거절이다.
    Committed(Vec<Result<O, E>>),
    /// 아무것도 확정하지 않았고 다시 실행하면 될 수 있다(잠금 경합).
    Busy(E),
    /// 아무것도 확정하지 않았다. `index` 요청은 실행할 수 없어 `error`로 끝내고 나머지를 다시
    /// 실행한다(요청 본문의 panic 등).
    Poisoned { index: usize, error: E },
    /// 아무것도 확정하지 않았다. 묶음의 모든 요청이 이 오류로 끝난다.
    Failed(E),
    /// 확정됐는지 알 수 없다(commit 단계의 저장 실패). 다시 실행하지 않는다.
    Unknown(E),
    /// 요청마다 따로 끝났다. 요청 순서로 담는다. 요청 사이에 transaction이 없는 저장소(파일)가
    /// 쓴다. 앞 요청의 확정은 뒤 요청의 실패와 관계없이 남으며 다시 실행하지 않는다.
    Finished(Vec<Settled<O, E>>),
}

/// [`Execution::Finished`]의 요청 하나의 결과.
#[derive(Debug)]
pub enum Settled<O, E> {
    /// 실행했다. `Ok`면 확정됐고 `Err`는 이 요청만의 거절이다(아무것도 바꾸지 않았다).
    Done(Result<O, E>),
    /// 확정하지 않았다.
    Failed(E),
    /// 확정됐는지 알 수 없다.
    Unknown(E),
}

impl<O, E> From<Settled<O, E>> for Completion<O, E> {
    fn from(settled: Settled<O, E>) -> Self {
        match settled {
            Settled::Done(result) => Self::Done(result),
            Settled::Failed(error) => Self::Failed(error),
            Settled::Unknown(error) => Self::Unknown(error),
        }
    }
}

/// worker 스레드에서 저장을 실행하는 쪽. 저장소 연결을 소유한다.
pub trait Executor: Send + 'static {
    type Request: Request;
    type Output: Send + 'static;
    type Error: Clone + Send + 'static;

    /// 묶음을 실행한다. busy·Poisoned 뒤에는 같은 요청(Poisoned는 그 요청을 뺀 나머지)으로
    /// 다시 부르므로 요청은 다시 실행할 수 있어야 한다.
    fn execute(&mut self, batch: &mut [Self::Request]) -> Execution<Self::Output, Self::Error>;

    /// 결과를 꺼낼 때까지 예산에 셀 결과 크기.
    fn output_bytes(_output: &Self::Output) -> usize {
        0
    }
}

/// 요청 하나의 끝난 결과.
#[derive(Debug)]
pub enum Completion<O, E> {
    /// 실행했다. 쓰기는 확정됐다. `Err`는 이 요청만의 거절이다.
    Done(Result<O, E>),
    /// 같은 묶음의 뒤 요청 `by`가 이 요청을 대체했다. `outcome`은 그 요청의 결과다.
    Superseded {
        by: u64,
        outcome: Box<Completion<(), E>>,
    },
    /// 확정하지 않았다(저장 실패, busy 재시도 시한 초과, 요청 panic).
    Failed(E),
    /// 확정됐는지 알 수 없다.
    Unknown(E),
    /// worker가 결과를 정하지 못하고 끝났다(worker 종료·Executor panic). 확정 여부를 알 수 없다.
    Lost,
}

impl<O, E: Clone> Completion<O, E> {
    /// 결과 값을 뺀 같은 결과.
    pub fn erase(&self) -> Completion<(), E> {
        match self {
            Self::Done(Ok(_)) => Completion::Done(Ok(())),
            Self::Done(Err(e)) => Completion::Done(Err(e.clone())),
            Self::Superseded { by, outcome } => Completion::Superseded {
                by: *by,
                outcome: outcome.clone(),
            },
            Self::Failed(e) => Completion::Failed(e.clone()),
            Self::Unknown(e) => Completion::Unknown(e.clone()),
            Self::Lost => Completion::Lost,
        }
    }

    /// 이 요청이 뜻한 변경이 확정됐는지. 대체된 요청은 대체한 요청의 결과를 따른다.
    pub fn took_effect(&self) -> bool {
        match self {
            Self::Done(r) => r.is_ok(),
            Self::Superseded { outcome, .. } => outcome.took_effect(),
            _ => false,
        }
    }
}

impl<E: Clone> Clone for Completion<(), E> {
    fn clone(&self) -> Self {
        self.erase()
    }
}

/// 접수 거절.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    /// 접수를 멈췄다(종료 중이거나 worker가 끝났다).
    Closed,
    /// 예산이 찼다. 값은 거절 시점에 잡혀 있던 항목 수와 바이트다.
    Saturated { items: usize, bytes: usize },
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "writer is not accepting requests"),
            Self::Saturated { items, bytes } => {
                write!(
                    f,
                    "writer queue is full ({items} requests, {bytes} bytes held)"
                )
            }
        }
    }
}

impl std::error::Error for SubmitError {}

/// flush cut 하나. `seq` 앞의 모든 요청이 끝났고, 그때까지 거절·실패한 요청의 누계를 담는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlushPoint {
    pub seq: u64,
    /// 요청 자신이 거절된 수(누계).
    pub rejected: u64,
    /// 확정하지 못했거나 확정 여부를 모르는 수(누계).
    pub failed: u64,
}

impl FlushPoint {
    /// writer가 시작한 시점.
    pub const START: Self = Self {
        seq: 0,
        rejected: 0,
        failed: 0,
    };
}

/// flush 구간에 효과가 없는 요청이 있었거나 flush 자체가 끝나지 못했다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlushFailure {
    /// cut까지 모두 끝났지만 구간 안에 거절·실패가 있었다.
    NotClean {
        point: FlushPoint,
        rejected: u64,
        failed: u64,
    },
    /// worker가 flush에 닿기 전에 끝났다.
    Lost,
}

pub type FlushResult = Result<FlushPoint, FlushFailure>;

/// 요청 하나의 receipt.
pub type RequestReceipt<X> = Receipt<Completion<<X as Executor>::Output, <X as Executor>::Error>>;

pub(crate) enum Work<X: Executor> {
    Request {
        request: X::Request,
        ticket: Ticket<Completion<X::Output, X::Error>>,
    },
    Flush {
        since: FlushPoint,
        ticket: Ticket<FlushResult>,
    },
}

pub(crate) struct Entry<X: Executor> {
    pub(crate) seq: u64,
    pub(crate) admitted_at: Instant,
    pub(crate) bytes: usize,
    pub(crate) work: Work<X>,
}

pub(crate) struct Queue<X: Executor> {
    pub(crate) entries: std::collections::VecDeque<Entry<X>>,
    next_seq: u64,
    admitting: bool,
    pub(crate) stopping: bool,
    /// worker가 끝나면 false. 그 뒤의 flush는 거절한다.
    pub(crate) worker_alive: bool,
}

pub(crate) struct Shared<X: Executor> {
    pub(crate) queue: Mutex<Queue<X>>,
    pub(crate) arrived: Condvar,
    budget: Arc<Budget>,
    pub(crate) limits: Limits,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) stats: stats::WriterStats,
}

impl<X: Executor> Shared<X> {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Queue<X>> {
        recover_mutex(self.queue.lock(), QUEUE_WHAT, &QUEUE_POISONED)
    }

    fn push_flush(&self, queue: &mut Queue<X>, since: FlushPoint) -> Receipt<FlushResult> {
        queue.next_seq += 1;
        let seq = queue.next_seq;
        let slot = Slot::new(None, 0);
        queue.entries.push_back(Entry {
            seq,
            admitted_at: self.clock.now(),
            bytes: 0,
            work: Work::Flush {
                since,
                ticket: Ticket::new(slot.clone(), || Err(FlushFailure::Lost)),
            },
        });
        Receipt::new(slot, seq)
    }
}

/// 요청을 넣는 쪽. 복제해 여러 스레드에서 쓴다.
pub struct Handle<X: Executor> {
    shared: Arc<Shared<X>>,
}

impl<X: Executor> Clone for Handle<X> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<X: Executor> Handle<X> {
    /// 요청을 접수한다. 큐 잠금만 잡고 바로 돌아온다.
    pub fn submit(&self, request: X::Request) -> Result<RequestReceipt<X>, SubmitError> {
        let bytes = request.bytes();
        let shared = &self.shared;
        let mut queue = shared.lock();
        if !queue.admitting {
            shared.stats.record_rejected_closed();
            return Err(SubmitError::Closed);
        }
        let held_items = shared.budget.items.load(Ordering::Acquire);
        let held_bytes = shared.budget.bytes.load(Ordering::Acquire);
        if held_items >= shared.limits.queue_items
            || held_bytes.saturating_add(bytes) > shared.limits.queue_bytes
        {
            shared.stats.record_rejected_saturated();
            return Err(SubmitError::Saturated {
                items: held_items,
                bytes: held_bytes,
            });
        }
        shared.budget.items.fetch_add(1, Ordering::AcqRel);
        shared.budget.bytes.fetch_add(bytes, Ordering::AcqRel);
        shared
            .stats
            .record_admitted(held_items + 1, held_bytes + bytes);
        queue.next_seq += 1;
        let seq = queue.next_seq;
        let slot = Slot::new(Some(shared.budget.clone()), bytes);
        queue.entries.push_back(Entry {
            seq,
            admitted_at: shared.clock.now(),
            bytes,
            work: Work::Request {
                request,
                ticket: Ticket::new(slot.clone(), || Completion::Lost),
            },
        });
        drop(queue);
        shared.arrived.notify_one();
        Ok(Receipt::new(slot, seq))
    }

    /// 지금까지 접수한 요청 뒤에 flush cut을 넣는다. 결과는 `since` 뒤부터 cut까지의 요청이 모두
    /// 효과를 냈을 때만 `Ok`다. 다른 호출자의 요청도 구간에 들어간다. 접수를 멈춘 뒤에도 worker가
    /// 살아 있으면 받는다. flush는 예산을 쓰지 않는다.
    pub fn flush(&self, since: FlushPoint) -> Result<Receipt<FlushResult>, SubmitError> {
        let shared = &self.shared;
        let mut queue = shared.lock();
        if !queue.worker_alive {
            return Err(SubmitError::Closed);
        }
        let receipt = shared.push_flush(&mut queue, since);
        drop(queue);
        shared.arrived.notify_one();
        Ok(receipt)
    }

    /// 누계 계측.
    pub fn stats(&self) -> WriterStatsSnapshot {
        self.shared.stats.snapshot(
            self.shared.budget.items.load(Ordering::Acquire),
            self.shared.budget.bytes.load(Ordering::Acquire),
        )
    }
}

/// Final flush and actual worker join are observed independently of process exit status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownObservation {
    NotStarted,
    Draining,
    Joined { panicked: bool, flush: FlushResult },
}

/// worker를 소유하고 수명을 관리한다. 요청은 [`Writer::handle`]로 넣는다.
pub struct Writer<X: Executor> {
    handle: Handle<X>,
    worker: Option<JoinHandle<()>>,
    final_flush: Option<Receipt<FlushResult>>,
    shutdown_observation: ShutdownObservation,
    name: String,
}

impl<X: Executor> Writer<X> {
    /// worker 스레드를 시작한다. `notify`는 묶음·flush가 끝날 때마다 worker 스레드에서 부른다.
    pub fn spawn(
        name: &str,
        executor: X,
        limits: Limits,
        clock: Arc<dyn Clock>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                entries: Default::default(),
                next_seq: 0,
                admitting: true,
                stopping: false,
                worker_alive: true,
            }),
            arrived: Condvar::new(),
            budget: Arc::new(Budget::default()),
            limits,
            clock,
            stats: Default::default(),
        });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || worker::run(worker_shared, executor, notify))?;
        Ok(Self {
            handle: Handle { shared },
            worker: Some(worker),
            final_flush: None,
            shutdown_observation: ShutdownObservation::NotStarted,
            name: name.to_owned(),
        })
    }

    pub fn handle(&self) -> Handle<X> {
        self.handle.clone()
    }

    /// 새 요청 접수를 멈춘다. 이미 접수한 요청은 계속 실행하고 flush는 받는다.
    pub fn close(&self) {
        self.handle.shared.lock().admitting = false;
    }

    /// 접수를 멈추고 마지막 flush cut을 넣은 뒤 worker가 남은 요청을 실행하고 끝나게 한다.
    /// 여러 번 불러도 한 번만 시작한다.
    pub fn begin_shutdown(&mut self) {
        if self.final_flush.is_some() {
            return;
        }
        if self.shutdown_observation == ShutdownObservation::NotStarted {
            self.shutdown_observation = ShutdownObservation::Draining;
        }
        let shared = &self.handle.shared;
        let mut queue = shared.lock();
        queue.admitting = false;
        if queue.worker_alive {
            self.final_flush = Some(shared.push_flush(&mut queue, FlushPoint::START));
        }
        queue.stopping = true;
        drop(queue);
        shared.arrived.notify_one();
    }

    /// 종료를 진행하고 아직 끝나지 않은 worker 수(0 또는 1)를 돌려준다. join이 끝나면 마지막
    /// flush 결과를 로그에 남긴다.
    pub fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        if self.worker.as_ref().is_some_and(|w| w.is_finished())
            && let Some(worker) = self.worker.take()
        {
            let panicked = worker.join().is_err();
            if panicked {
                tracing::error!(writer = %self.name, "writer worker panicked");
            }
            self.log_final_flush(panicked);
        }
        usize::from(self.worker.is_some())
    }

    /// A read-only snapshot; never consumes a request or final flush receipt.
    pub fn shutdown_observation(&self) -> ShutdownObservation {
        self.shutdown_observation.clone()
    }

    fn log_final_flush(&mut self, panicked: bool) {
        let result = self
            .final_flush
            .take()
            .and_then(|receipt| receipt.try_take())
            .unwrap_or(Err(FlushFailure::Lost));
        self.shutdown_observation = ShutdownObservation::Joined {
            panicked,
            flush: result.clone(),
        };
        match &result {
            Ok(point) => {
                tracing::debug!(writer = %self.name, seq = point.seq, "writer drained");
            }
            Err(FlushFailure::NotClean {
                point,
                rejected,
                failed,
            }) => log_unclean_drain(&self.name, *point, *rejected, *failed),
            Err(FlushFailure::Lost) => log_lost_drain(&self.name),
        }
    }

    /// 종료를 요청하지 않았는데 worker가 끝났는지. 메인 루프의 점검용이다.
    pub fn worker_ended_unexpectedly(&self) -> bool {
        self.shutdown_observation == ShutdownObservation::NotStarted
            && self.worker.as_ref().is_none_or(|w| w.is_finished())
    }

    pub fn stats(&self) -> WriterStatsSnapshot {
        self.handle.stats()
    }
}

fn log_unclean_drain(name: &str, point: FlushPoint, rejected: u64, failed: u64) {
    tracing::warn!(
        writer = %name,
        seq = point.seq,
        rejected,
        failed,
        "writer drained with requests that did not take effect"
    );
}

fn log_lost_drain(name: &str) {
    tracing::error!(
        writer = %name,
        "writer ended before its final flush; accepted requests may be lost"
    );
}

impl<X: Executor> Drop for Writer<X> {
    fn drop(&mut self) {
        if self.poll_shutdown() != 0 {
            tracing::warn!(
                writer = %self.name,
                "writer remains unjoined at drop; requests it has not committed may be lost"
            );
        }
    }
}

#[cfg(test)]
mod tests;
