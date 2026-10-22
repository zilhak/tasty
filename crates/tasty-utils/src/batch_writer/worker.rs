//! worker 스레드: 수집 창을 닫아 묶음을 만들고 실행해 결과를 요청마다 전한다.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, MutexGuard};
use std::time::Instant;

use super::slot::Ticket;
use super::{
    Completion, Execution, Executor, FlushFailure, FlushPoint, FlushResult, Queue, Request, Shared,
    Work,
};
use crate::poison::recover_poisoned;

static WAIT_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 처리한 요청 중 효과가 없었던 것의 누계. flush가 구간 결과를 계산한다.
#[derive(Default)]
struct Tally {
    rejected: u64,
    failed: u64,
}

impl Tally {
    fn count<E>(&mut self, completion: &Completion<(), E>) {
        match completion {
            Completion::Done(Ok(())) => {}
            Completion::Done(Err(_)) => self.rejected += 1,
            Completion::Superseded { outcome, .. } => self.count(outcome),
            Completion::Failed(_) | Completion::Unknown(_) | Completion::Lost => self.failed += 1,
        }
    }
}

type RequestTicket<X> = Ticket<Completion<<X as Executor>::Output, <X as Executor>::Error>>;

struct Pending<X: Executor> {
    seq: u64,
    request: X::Request,
    ticket: RequestTicket<X>,
}

enum Cut<X: Executor> {
    Flush {
        seq: u64,
        since: FlushPoint,
        ticket: Ticket<FlushResult>,
    },
    Batch(Vec<Pending<X>>),
}

/// worker가 어떻게 끝나든(정상·panic) 접수를 막고 남은 요청을 Lost로 끝낸다.
struct ExitGuard<'a, X: Executor>(&'a Shared<X>);

impl<X: Executor> Drop for ExitGuard<'_, X> {
    fn drop(&mut self) {
        let mut queue = self.0.lock();
        queue.worker_alive = false;
        queue.admitting = false;
        let left: Vec<_> = queue.entries.drain(..).collect();
        drop(queue);
        if !left.is_empty() {
            tracing::error!(
                requests = left.len(),
                "batch writer ended with queued requests; they end as lost"
            );
        }
    }
}

pub(crate) fn run<X: Executor>(
    shared: Arc<Shared<X>>,
    mut executor: X,
    notify: Arc<dyn Fn() + Send + Sync>,
) {
    let _exit = ExitGuard(&shared);
    let mut tally = Tally::default();
    while let Some(cut) = next_cut(&shared) {
        match cut {
            Cut::Flush { seq, since, ticket } => {
                shared.stats.record_flush();
                let point = FlushPoint {
                    seq,
                    rejected: tally.rejected,
                    failed: tally.failed,
                };
                let rejected = point.rejected.saturating_sub(since.rejected);
                let failed = point.failed.saturating_sub(since.failed);
                let result = if rejected == 0 && failed == 0 {
                    Ok(point)
                } else {
                    Err(FlushFailure::NotClean {
                        point,
                        rejected,
                        failed,
                    })
                };
                ticket.complete(result, 0);
            }
            Cut::Batch(batch) => run_batch(&shared, &mut executor, &mut tally, batch),
        }
        notify();
    }
}

fn wait<'a, X: Executor>(
    shared: &'a Shared<X>,
    queue: MutexGuard<'a, Queue<X>>,
    deadline: Option<Instant>,
) -> MutexGuard<'a, Queue<X>> {
    const WHAT: &str = "batch writer queue (wait)";
    match deadline {
        None => shared
            .arrived
            .wait(queue)
            .unwrap_or_else(|poisoned| recover_poisoned(poisoned, WHAT, &WAIT_POISONED)),
        Some(at) => {
            shared
                .arrived
                .wait_timeout(queue, shared.clock.wait_for(at))
                .unwrap_or_else(|poisoned| recover_poisoned(poisoned, WHAT, &WAIT_POISONED))
                .0
        }
    }
}

/// 다음에 실행할 묶음 또는 flush. 종료 중이고 큐가 비면 None이다.
fn next_cut<X: Executor>(shared: &Shared<X>) -> Option<Cut<X>> {
    let limits = &shared.limits;
    let mut queue = shared.lock();
    loop {
        let Some(front) = queue.entries.front() else {
            if queue.stopping {
                return None;
            }
            queue = wait(shared, queue, None);
            continue;
        };
        let opened = front.admitted_at;
        let barrier = match &front.work {
            Work::Flush { .. } => {
                let Some(entry) = queue.entries.pop_front() else {
                    continue;
                };
                let Work::Flush { since, ticket } = entry.work else {
                    continue;
                };
                return Some(Cut::Flush {
                    seq: entry.seq,
                    since,
                    ticket,
                });
            }
            Work::Request { request, .. } => request.barrier(),
        };
        if barrier {
            return Some(Cut::Batch(take(&mut queue, 1)));
        }
        let mut count = 0usize;
        let mut bytes = 0usize;
        let mut full = false;
        let mut blocked = false;
        for entry in &queue.entries {
            let joinable =
                matches!(&entry.work, Work::Request { request, .. } if !request.barrier());
            if !joinable {
                blocked = true;
                break;
            }
            if count == limits.max_items || (count > 0 && bytes + entry.bytes > limits.max_bytes) {
                full = true;
                break;
            }
            count += 1;
            bytes = bytes.saturating_add(entry.bytes);
            if bytes >= limits.max_bytes {
                full = true;
                break;
            }
        }
        full |= count == limits.max_items;
        let deadline = opened + limits.max_delay;
        if full || blocked || queue.stopping || shared.clock.now() >= deadline {
            return Some(Cut::Batch(take(&mut queue, count)));
        }
        queue = wait(shared, queue, Some(deadline));
    }
}

fn take<X: Executor>(queue: &mut Queue<X>, count: usize) -> Vec<Pending<X>> {
    queue
        .entries
        .drain(..count)
        .filter_map(|entry| match entry.work {
            Work::Request { request, ticket } => Some(Pending {
                seq: entry.seq,
                request,
                ticket,
            }),
            // 수집은 요청만 묶는다. 오지 않는 경우이며 버린 flush는 Lost로 끝난다.
            Work::Flush { .. } => None,
        })
        .collect()
}

/// 대체된 요청. `by`의 결과를 받는다.
struct Superseded<X: Executor> {
    by: u64,
    ticket: RequestTicket<X>,
}

/// 실행할 요청과 그 결과를 받을 쪽. 같은 위치끼리 짝이다.
struct Run<X: Executor> {
    seqs: Vec<u64>,
    requests: Vec<X::Request>,
    tickets: Vec<RequestTicket<X>>,
    links: Vec<Superseded<X>>,
}

impl<X: Executor> Run<X> {
    /// 같은 snapshot 키는 마지막 요청만 실행하고 앞 요청은 그 결과를 받게 한다.
    fn new(batch: Vec<Pending<X>>) -> Self {
        let mut last_of_key: HashMap<String, u64> = HashMap::new();
        let mut supersede_by = vec![None; batch.len()];
        for (i, pending) in batch.iter().enumerate().rev() {
            let Some(key) = pending.request.snapshot_key() else {
                continue;
            };
            match last_of_key.get(key) {
                Some(&by) => supersede_by[i] = Some(by),
                None => {
                    last_of_key.insert(key.to_owned(), pending.seq);
                }
            }
        }
        let mut run = Self {
            seqs: Vec::with_capacity(batch.len()),
            requests: Vec::with_capacity(batch.len()),
            tickets: Vec::with_capacity(batch.len()),
            links: Vec::new(),
        };
        for (pending, by) in batch.into_iter().zip(supersede_by) {
            match by {
                Some(by) => run.links.push(Superseded {
                    by,
                    ticket: pending.ticket,
                }),
                None => {
                    run.seqs.push(pending.seq);
                    run.requests.push(pending.request);
                    run.tickets.push(pending.ticket);
                }
            }
        }
        run
    }

    fn commit(mut self, tally: &mut Tally, results: Vec<Result<X::Output, X::Error>>) {
        let mut results = results.into_iter();
        for (seq, ticket) in self.seqs.drain(..).zip(self.tickets.drain(..)) {
            let completion = results.next().map_or(Completion::Lost, Completion::Done);
            finish::<X>(tally, &mut self.links, seq, ticket, completion);
        }
        finish_rest::<X>(tally, self.links);
    }

    /// 요청마다 따로 끝난 결과를 전한다. 결과가 모자란 요청은 확정 여부를 알 수 없어 Lost다.
    fn settle(mut self, tally: &mut Tally, results: Vec<super::Settled<X::Output, X::Error>>) {
        let mut results = results.into_iter();
        for (seq, ticket) in self.seqs.drain(..).zip(self.tickets.drain(..)) {
            let completion = results.next().map_or(Completion::Lost, Completion::from);
            finish::<X>(tally, &mut self.links, seq, ticket, completion);
        }
        finish_rest::<X>(tally, self.links);
    }

    fn drop_request(&mut self, tally: &mut Tally, index: usize, error: X::Error) {
        self.requests.remove(index);
        let seq = self.seqs.remove(index);
        let ticket = self.tickets.remove(index);
        finish::<X>(
            tally,
            &mut self.links,
            seq,
            ticket,
            Completion::Failed(error),
        );
    }

    fn end_all(mut self, tally: &mut Tally, end: BatchEnd<X::Error>) {
        for (seq, ticket) in self.seqs.drain(..).zip(self.tickets.drain(..)) {
            finish::<X>(tally, &mut self.links, seq, ticket, end.completion());
        }
        finish_rest::<X>(tally, self.links);
    }
}

/// busy 재시도 시한과 대기.
struct Retry {
    started: Instant,
    backoff: std::time::Duration,
}

impl Retry {
    /// 시한 안이면 기다리고 true를 돌려준다.
    fn wait<X: Executor>(&mut self, shared: &Shared<X>) -> bool {
        let limits = &shared.limits;
        if shared.clock.now().saturating_duration_since(self.started) >= limits.retry_for {
            return false;
        }
        shared.stats.record_retry();
        shared.clock.sleep(self.backoff);
        self.backoff = (self.backoff * 2).min(limits.retry_backoff_max);
        true
    }
}

fn run_batch<X: Executor>(
    shared: &Shared<X>,
    executor: &mut X,
    tally: &mut Tally,
    batch: Vec<Pending<X>>,
) {
    let total = batch.len();
    let mut run = Run::<X>::new(batch);
    shared.stats.record_batch(total, run.links.len());
    let mut retry = Retry {
        started: shared.clock.now(),
        backoff: shared.limits.retry_backoff,
    };
    let end = loop {
        if run.requests.is_empty() {
            return finish_rest::<X>(tally, run.links);
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| executor.execute(&mut run.requests)));
        match outcome {
            Ok(Execution::Committed(results)) => return run.commit(tally, results),
            Ok(Execution::Finished(results)) => return run.settle(tally, results),
            Ok(Execution::Busy(error)) => {
                if retry.wait(shared) {
                    continue;
                }
                tracing::warn!(
                    requests = run.requests.len(),
                    "batch writer gave up retrying a busy batch"
                );
                break BatchEnd::Failed(error);
            }
            Ok(Execution::Poisoned { index, error }) if index < run.requests.len() => {
                run.drop_request(tally, index, error);
            }
            Ok(Execution::Poisoned { error, .. } | Execution::Failed(error)) => {
                break BatchEnd::Failed(error);
            }
            Ok(Execution::Unknown(error)) => break BatchEnd::Unknown(error),
            Err(_) => {
                tracing::error!(
                    requests = run.requests.len(),
                    "batch writer executor panicked; its batch ends as lost"
                );
                break BatchEnd::Lost;
            }
        }
    };
    run.end_all(tally, end);
}

/// 대상이 끝나기 전에 빠진 대체 요청(오지 않는 경우). Lost로 끝낸다.
fn finish_rest<X: Executor>(tally: &mut Tally, links: Vec<Superseded<X>>) {
    for link in links {
        tally.count::<X::Error>(&Completion::Lost);
        link.ticket.complete(Completion::Lost, 0);
    }
}

fn finish<X: Executor>(
    tally: &mut Tally,
    links: &mut Vec<Superseded<X>>,
    seq: u64,
    ticket: RequestTicket<X>,
    completion: Completion<X::Output, X::Error>,
) {
    let erased = completion.erase();
    let mut i = 0;
    while i < links.len() {
        if links[i].by == seq {
            let link = links.swap_remove(i);
            let superseded: Completion<X::Output, X::Error> = Completion::Superseded {
                by: seq,
                outcome: Box::new(erased.clone()),
            };
            tally.count(&superseded.erase());
            link.ticket.complete(superseded, 0);
        } else {
            i += 1;
        }
    }
    tally.count(&erased);
    let out_bytes = match &completion {
        Completion::Done(Ok(output)) => X::output_bytes(output),
        _ => 0,
    };
    ticket.complete(completion, out_bytes);
}

/// 묶음 전체가 같은 결과로 끝났다.
enum BatchEnd<E> {
    Failed(E),
    Unknown(E),
    Lost,
}

impl<E: Clone> BatchEnd<E> {
    fn completion<O>(&self) -> Completion<O, E> {
        match self {
            Self::Failed(e) => Completion::Failed(e.clone()),
            Self::Unknown(e) => Completion::Unknown(e.clone()),
            Self::Lost => Completion::Lost,
        }
    }
}
