//! 호스트 소비자가 memory.db에 요청을 넣는 핸들과 확정된 변경을 메인 루프에 넘기는 버퍼.
//!
//! - writer가 있는 구성(제품 부팅)은 요청을 writer 큐에 넣고 바로 돌아온다. 결과는 ticket으로 받는다.
//! - writer가 없는 구성(시험)은 같은 실행 규칙(`execute_on`)으로 호출 스레드에서 바로 실행하고
//!   ticket에 결과를 담아 돌려준다. 제품 부팅은 writer 없이 시작하지 않으므로 이 경로는 시험용이다.
//!
//! 두 경로 모두 실행 단위가 끝나면 저장소의 변경 버퍼를 [`ChangeFeed`]로 옮긴다. 메인 루프는
//! 저장소 잠금 없이 이 버퍼에서 `memory.changed` 원본을 꺼낸다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tasty_utils::batch_writer::{self, SubmitError};

use super::{
    MemoryCompletion, MemoryExecutor, MemoryOutput, MemoryRequest, SharedStore, WriterError,
    execute_on,
};
use crate::{MemoryChange, MemoryError, MemoryStorage};

static FEED_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// commit이 끝난 regular 변경. writer worker(또는 즉시 실행)가 넣고 메인 루프가 꺼낸다.
#[derive(Default)]
pub struct ChangeFeed(Mutex<Vec<MemoryChange>>);

impl ChangeFeed {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<MemoryChange>> {
        tasty_utils::poison::recover_mutex(self.0.lock(), "memory change feed", &FEED_POISONED)
    }

    pub(crate) fn extend(&self, changes: Vec<MemoryChange>) {
        if !changes.is_empty() {
            self.lock().extend(changes);
        }
    }

    /// 쌓인 변경을 접수 순서대로 꺼낸다. 저장소 잠금을 잡지 않는다.
    pub fn take(&self) -> Vec<MemoryChange> {
        std::mem::take(&mut *self.lock())
    }
}

/// 요청 하나의 결과를 받는 쪽. 버려도 요청은 취소되지 않는다.
pub struct MemoryTicket(Ticket);

enum Ticket {
    Queued(batch_writer::RequestReceipt<MemoryExecutor<SharedStore>>),
    Ready(Option<MemoryCompletion>),
}

impl MemoryTicket {
    /// 결과가 있으면 꺼낸다. 기다리지 않는다. 한 번 꺼낸 뒤에는 None이다.
    pub fn try_take(&mut self) -> Option<MemoryCompletion> {
        match &mut self.0 {
            Ticket::Queued(receipt) => receipt.try_take(),
            Ticket::Ready(slot) => slot.take(),
        }
    }

    /// 결과를 `timeout`까지 기다린다. 메인 루프는 `try_take`를 쓴다.
    pub fn wait_timeout(&mut self, timeout: Duration) -> Option<MemoryCompletion> {
        match &mut self.0 {
            Ticket::Queued(receipt) => receipt.wait_timeout(timeout),
            Ticket::Ready(slot) => slot.take(),
        }
    }
}

impl std::fmt::Debug for MemoryTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Ticket::Queued(receipt) => f.debug_tuple("Queued").field(&receipt.seq()).finish(),
            Ticket::Ready(slot) => f.debug_tuple("Ready").field(&slot.is_some()).finish(),
        }
    }
}

#[derive(Clone)]
enum Route {
    Writer(batch_writer::Handle<MemoryExecutor<SharedStore>>),
    Inline(Arc<Mutex<dyn MemoryStorage>>),
}

/// memory.db에 요청을 넣는 복제 가능한 핸들.
#[derive(Clone)]
pub struct MemoryAccess {
    route: Route,
    changes: Arc<ChangeFeed>,
    detached: Arc<DetachedWrites>,
}

/// 결과로 할 일. 실패는 [`MemoryAccess::reap_detached`]가 로그로 남긴다.
type Done = Box<dyn FnOnce(MemoryOutput) + Send>;

struct DetachedWrite {
    ticket: MemoryTicket,
    what: &'static str,
    done: Option<Done>,
}

static DETACHED_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 결과를 기다리지 않는 요청. 모든 복제 핸들이 공유하고 메인 루프가 회차마다 거둔다.
#[derive(Default)]
struct DetachedWrites(Mutex<Vec<DetachedWrite>>);

impl DetachedWrites {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<DetachedWrite>> {
        tasty_utils::poison::recover_mutex(
            self.0.lock(),
            "detached memory writes",
            &DETACHED_POISONED,
        )
    }
}

fn finish_detached(what: &'static str, done: Option<Done>, completion: MemoryCompletion) {
    match settle(completion) {
        Ok(output) => {
            if let Some(done) = done {
                done(output);
            }
        }
        Err(failure) => tracing::warn!(what, ?failure, "memory write did not land"),
    }
}

impl MemoryAccess {
    /// writer 큐로 보내는 핸들. `changes`는 writer를 만들 때 넘긴 버퍼와 같아야 한다.
    pub fn writer(
        handle: batch_writer::Handle<MemoryExecutor<SharedStore>>,
        changes: Arc<ChangeFeed>,
    ) -> Self {
        Self {
            route: Route::Writer(handle),
            changes,
            detached: Arc::default(),
        }
    }

    /// writer 없는 구성(시험). 요청을 호출 스레드에서 바로 실행한다.
    pub fn inline(store: Arc<Mutex<dyn MemoryStorage>>) -> Self {
        Self {
            route: Route::Inline(store),
            changes: Arc::default(),
            detached: Arc::default(),
        }
    }

    /// writer 큐로 보내는지. 아니면 호출 스레드에서 바로 실행한다.
    pub fn is_queued(&self) -> bool {
        matches!(self.route, Route::Writer(_))
    }

    /// 요청을 접수한다. writer 경로는 큐 잠금만 잡고 돌아온다.
    pub fn submit(&self, request: MemoryRequest) -> Result<MemoryTicket, SubmitError> {
        match &self.route {
            Route::Writer(handle) => handle
                .submit(request)
                .map(|r| MemoryTicket(Ticket::Queued(r))),
            Route::Inline(store) => Ok(MemoryTicket(Ticket::Ready(Some(run_inline(
                store,
                &self.changes,
                request,
            ))))),
        }
    }

    /// 결과를 기다리지 않는 요청을 넣는다. writer 순서를 따르므로 뒤에 넣은 요청은 이 요청의 반영을
    /// 본다. 성공하면 `done`을 부르고 실패는 로그로 남긴다. 둘 다 [`Self::reap_detached`]를 부르는
    /// 스레드(메인 루프)에서 일어난다.
    pub fn submit_detached(&self, request: MemoryRequest, what: &'static str, done: Option<Done>) {
        match self.submit(request) {
            Ok(mut ticket) => match ticket.try_take() {
                Some(completion) => finish_detached(what, done, completion),
                None => self
                    .detached
                    .lock()
                    .push(DetachedWrite { ticket, what, done }),
            },
            Err(error) => tracing::warn!(what, %error, "memory writer did not accept a write"),
        }
    }

    /// 끝난 기다리지 않는 요청을 거둔다.
    pub fn reap_detached(&self) {
        let finished: Vec<_> = {
            let mut pending = self.detached.lock();
            let mut finished = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                match pending[index].ticket.try_take() {
                    Some(completion) => {
                        let entry = pending.swap_remove(index);
                        finished.push((entry.what, entry.done, completion));
                    }
                    None => index += 1,
                }
            }
            finished
        };
        for (what, done, completion) in finished {
            finish_detached(what, done, completion);
        }
    }

    /// 아직 결과를 거두지 않은 기다리지 않는 요청 수.
    pub fn detached_len(&self) -> usize {
        self.detached.lock().len()
    }

    /// 요청을 넣고 결과를 기다린다. 메인 루프처럼 writer 결과를 거두는 스레드에서는 쓰지 않는다.
    /// 시한 안에 결과가 없으면 `Waited::TimedOut`이며 요청은 취소되지 않는다.
    pub fn run_and_wait(
        &self,
        request: MemoryRequest,
        timeout: std::time::Duration,
    ) -> std::result::Result<MemoryOutput, Waited> {
        let mut ticket = self.submit(request).map_err(Waited::Refused)?;
        let completion = ticket.wait_timeout(timeout).ok_or(Waited::TimedOut)?;
        settle(completion).map_err(Waited::Failed)
    }

    /// commit된 변경 버퍼.
    pub fn changes(&self) -> &Arc<ChangeFeed> {
        &self.changes
    }
}

fn run_inline(
    store: &Arc<Mutex<dyn MemoryStorage>>,
    changes: &ChangeFeed,
    request: MemoryRequest,
) -> MemoryCompletion {
    use batch_writer::{Completion, Execution};
    let mut batch = [request];
    if let [MemoryRequest::Section { body, bytes }] = &mut batch {
        let execution = super::run_section(body, *bytes);
        let mut guard = tasty_utils::poison::recover_mutex(
            store.lock(),
            crate::STORE_LOCK_WHAT,
            &crate::STORE_LOCK_POISONED,
        );
        changes.extend(guard.take_pending_changes());
        return match execution {
            Execution::Committed(mut results) => {
                results.pop().map_or(Completion::Lost, Completion::Done)
            }
            Execution::Unknown(error) => Completion::Unknown(error),
            Execution::Busy(error)
            | Execution::Failed(error)
            | Execution::Poisoned { error, .. } => Completion::Failed(error),
        };
    }
    let mut guard = tasty_utils::poison::recover_mutex(
        store.lock(),
        crate::STORE_LOCK_WHAT,
        &crate::STORE_LOCK_POISONED,
    );
    let execution = execute_on(&mut *guard, &mut batch);
    changes.extend(guard.take_pending_changes());
    match execution {
        Execution::Committed(mut results) => match results.pop() {
            Some(result) => Completion::Done(result),
            None => Completion::Lost,
        },
        Execution::Busy(error) | Execution::Failed(error) => Completion::Failed(error),
        Execution::Poisoned { error, .. } => Completion::Failed(error),
        Execution::Unknown(error) => Completion::Unknown(error),
    }
}

/// 요청이 효과를 내지 못했거나 효과를 냈는지 모르는 이유.
#[derive(Debug, Clone)]
pub enum Failure {
    /// 요청 자신이 거절됐다(CAS·owner·quota·형식, 본문이 돌려준 오류). 확정되지 않았다.
    Rejected {
        error: Arc<MemoryError>,
        index: Option<usize>,
    },
    /// 확정하지 않았다(저장 실패, busy 재시도 시한 초과).
    Storage(Arc<MemoryError>),
    /// 요청 본문이 panic했다. 확정하지 않았다.
    Panicked,
    /// 확정됐는지 알 수 없다(commit 단계 실패, worker 종료·실행기 panic). 원인 오류가 있으면 싣는다.
    Unknown(Option<Arc<MemoryError>>),
}

impl Failure {
    fn of(error: WriterError) -> Self {
        match error {
            WriterError::Rejected { error, index } => Self::Rejected { error, index },
            WriterError::Storage(error) => Self::Storage(error),
            WriterError::Panicked => Self::Panicked,
        }
    }
}

/// [`MemoryAccess::run_and_wait`]가 값을 얻지 못한 이유.
#[derive(Debug, Clone)]
pub enum Waited {
    /// writer가 접수하지 않았다.
    Refused(SubmitError),
    /// 시한 안에 결과가 없었다. 요청은 계속 실행될 수 있다.
    TimedOut,
    /// 결과가 실패였다.
    Failed(Failure),
}

/// 결과 하나를 성공 값과 실패 이유로 나눈다. memory 요청은 snapshot 키가 없어 대체되지 않는다.
pub fn settle(completion: MemoryCompletion) -> std::result::Result<MemoryOutput, Failure> {
    use batch_writer::Completion;
    match completion {
        Completion::Done(Ok(output)) => Ok(output),
        Completion::Done(Err(error)) | Completion::Failed(error) => Err(Failure::of(error)),
        Completion::Unknown(error) => Err(Failure::Unknown(match error {
            WriterError::Rejected { error, .. } | WriterError::Storage(error) => Some(error),
            WriterError::Panicked => None,
        })),
        Completion::Superseded { .. } | Completion::Lost => Err(Failure::Unknown(None)),
    }
}
