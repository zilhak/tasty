//! observer memory sink. 채널에 쌓인 항목을 한 번에 꺼내 writer에 독립 요청 묶음 하나로 넣는다.
//!
//! - 모든 append 기록은 저마다 키를 받아 저장된다. 같은 키로 합치지 않는다.
//! - 보존 삭제(`max_records`)는 저장이 확정된 키만 센다. 넘친 옛 키의 삭제는 다음 묶음에 싣는다.
//! - writer가 접수하지 않은 기록(`submit_rejected`)과 writer가 거절했거나 확정하지 못한 기록
//!   (`store_failed`)을 따로 센다. 채널이 차서 버린 기록은 router의 `dropped`가 센다.
//! - sink는 결과를 기다리지 않고 다음 항목을 받는다. 채널이 끊기면 남은 결과를 기다린 뒤 끝난다.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::json;
use tasty_memory::writer::{Failure, MemoryAccess, MemoryOutput, MemoryRequest, MemoryTicket};
use tasty_memory::{
    BatchMode, HOST_OWNER, MemoryError, MemoryValue, PutOpts, Scope, WriteOp, WriteOutput,
};
use tasty_output::ParsedItem;

use super::{ObserverId, unix_ms_now};

/// 결과를 기다리는 묶음이 있을 때 채널을 기다리는 시간. 그사이 끝난 결과를 거둔다.
const RESULT_POLL: Duration = Duration::from_millis(5);
/// 채널이 끊긴 뒤 남은 결과를 기다리는 시간. 넘기면 결과를 모른 채 끝낸다.
const FINAL_WAIT: Duration = Duration::from_secs(30);
/// 한 묶음에 싣는 기록 수. 채널 용량과 같다.
const BATCH_RECORDS: usize = super::SINK_CHANNEL_CAP;

/// router의 정보 조회와 공유하는 sink 집계.
#[derive(Debug, Default)]
pub(super) struct SinkTally {
    pub(super) submit_rejected: AtomicU64,
    pub(super) store_failed: AtomicU64,
}

enum Op {
    Put(String),
    Delete(String),
}

struct InFlight {
    ticket: MemoryTicket,
    ops: Vec<Op>,
}

struct MemorySink {
    observer_id: ObserverId,
    max_records: usize,
    memory: MemoryAccess,
    tally: Arc<SinkTally>,
    /// 저장이 확정된 키. 보존 삭제의 대상이다.
    confirmed: VecDeque<String>,
    /// 다음 묶음에 실을 보존 삭제.
    deletes: Vec<String>,
    in_flight: VecDeque<InFlight>,
    /// 같은 밀리초의 여러 항목이 같은 키를 덮어쓰지 않도록 sink 순번을 덧붙인다.
    seq: u64,
}

pub(super) fn run(
    observer_id: ObserverId,
    max_records: usize,
    rx: Receiver<ParsedItem>,
    memory: MemoryAccess,
    tally: Arc<SinkTally>,
) {
    let mut sink = MemorySink {
        observer_id,
        max_records,
        memory,
        tally,
        confirmed: VecDeque::with_capacity(max_records.min(1024)),
        deletes: Vec::new(),
        in_flight: VecDeque::new(),
        seq: 0,
    };
    loop {
        let first = if sink.in_flight.is_empty() {
            match rx.recv() {
                Ok(item) => Some(item),
                Err(_) => break,
            }
        } else {
            match rx.recv_timeout(RESULT_POLL) {
                Ok(item) => Some(item),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        let mut items: Vec<ParsedItem> = first.into_iter().collect();
        while items.len() < BATCH_RECORDS
            && let Ok(item) = rx.try_recv()
        {
            items.push(item);
        }
        sink.reap(false);
        if !items.is_empty() {
            sink.submit(items);
        }
    }
    sink.finish();
}

impl MemorySink {
    fn record_op(&mut self, item: ParsedItem) -> (String, WriteOp) {
        let now = unix_ms_now();
        let key = format!("tasty.observer.{}.{now}.{:06}", self.observer_id, self.seq);
        self.seq += 1;
        let record = json!({
            "kind": item.kind,
            "line": item.line,
            "byte_start": item.byte_start,
            "byte_end": item.byte_end,
            "data": item.data,
            "at_ms": now,
        });
        let op = WriteOp::Put {
            owner: HOST_OWNER.into(),
            scope: Scope::Global,
            key: key.clone(),
            value: MemoryValue::Json(record),
            opts: PutOpts::default(),
        };
        (key, op)
    }

    fn submit(&mut self, items: Vec<ParsedItem>) {
        let mut ops = Vec::with_capacity(items.len() + self.deletes.len());
        let mut writes = Vec::with_capacity(items.len() + self.deletes.len());
        for item in items {
            let (key, op) = self.record_op(item);
            ops.push(Op::Put(key));
            writes.push(op);
        }
        let records = ops.len();
        self.push_deletes(&mut ops, &mut writes);
        self.send(ops, writes, records);
    }

    fn push_deletes(&mut self, ops: &mut Vec<Op>, writes: &mut Vec<WriteOp>) {
        for key in self.deletes.drain(..) {
            writes.push(WriteOp::Delete {
                owner: HOST_OWNER.into(),
                scope: Scope::Global,
                key: key.clone(),
                cas: None,
            });
            ops.push(Op::Delete(key));
        }
    }

    fn send(&mut self, ops: Vec<Op>, writes: Vec<WriteOp>, records: usize) {
        let request = MemoryRequest::Batch {
            mode: BatchMode::Independent,
            ops: writes,
        };
        match self.memory.submit(request) {
            Ok(ticket) => self.in_flight.push_back(InFlight { ticket, ops }),
            Err(error) => {
                let total = self
                    .tally
                    .submit_rejected
                    .fetch_add(records as u64, Ordering::Relaxed)
                    + records as u64;
                tracing::warn!(
                    observer = self.observer_id,
                    records,
                    total,
                    %error,
                    "observer memory sink: writer did not accept records"
                );
                // 보존 삭제는 기록이 아니므로 버리지 않고 다음 묶음에 다시 싣는다.
                for op in ops {
                    if let Op::Delete(key) = op {
                        self.deletes.push(key);
                    }
                }
            }
        }
    }

    /// 끝난 결과를 접수 순서대로 거둔다. `wait`이면 남은 결과를 모두 기다린다.
    fn reap(&mut self, wait: bool) {
        while let Some(front) = self.in_flight.front_mut() {
            let completion = if wait {
                front.ticket.wait_timeout(FINAL_WAIT)
            } else {
                front.ticket.try_take()
            };
            let Some(completion) = completion else {
                if wait {
                    tracing::warn!(
                        observer = self.observer_id,
                        batches = self.in_flight.len(),
                        "observer memory sink: gave up waiting for writer results"
                    );
                    self.in_flight.clear();
                }
                return;
            };
            let Some(done) = self.in_flight.pop_front() else {
                return;
            };
            self.settle(done.ops, tasty_memory::writer::settle(completion));
        }
    }

    fn settle(&mut self, ops: Vec<Op>, result: Result<MemoryOutput, Failure>) {
        match result {
            Ok(MemoryOutput::Batch(results)) => {
                for (op, result) in ops.into_iter().zip(results) {
                    self.settle_op(op, result);
                }
            }
            Ok(other) => {
                tracing::error!(
                    observer = self.observer_id,
                    ?other,
                    "unexpected sink output"
                );
            }
            Err(failure) => self.fail_batch(&ops, &failure),
        }
    }

    fn fail_batch(&self, ops: &[Op], failure: &Failure) {
        let records = ops.iter().filter(|op| matches!(op, Op::Put(_))).count();
        self.tally
            .store_failed
            .fetch_add(records as u64, Ordering::Relaxed);
        tracing::warn!(
            observer = self.observer_id,
            records,
            ?failure,
            "observer memory sink: records were not stored"
        );
    }

    fn settle_op(&mut self, op: Op, result: tasty_memory::Result<WriteOutput>) {
        match (op, result) {
            (Op::Put(key), Ok(WriteOutput::Version(_))) => self.confirm(key),
            (Op::Put(key), other) => {
                self.tally.store_failed.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    observer = self.observer_id,
                    key,
                    ?other,
                    "observer memory put failed"
                );
            }
            // 이미 없는 키는 상한이 지켜진 것이라 경고하지 않는다.
            (Op::Delete(_), Ok(_) | Err(MemoryError::NotFound { .. })) => {}
            // 키는 이미 confirmed에서 빠졌으므로 재시도하지 않는다. 상한이 그만큼 무너진다.
            (Op::Delete(key), Err(error)) => tracing::warn!(
                observer = self.observer_id,
                key,
                %error,
                "observer retention delete failed"
            ),
        }
    }

    fn confirm(&mut self, key: String) {
        if self.max_records == 0 {
            return;
        }
        self.confirmed.push_back(key);
        while self.confirmed.len() > self.max_records {
            if let Some(old) = self.confirmed.pop_front() {
                self.deletes.push(old);
            }
        }
    }

    fn finish(&mut self) {
        self.reap(true);
        if !self.deletes.is_empty() {
            let mut ops = Vec::new();
            let mut writes = Vec::new();
            self.push_deletes(&mut ops, &mut writes);
            self.send(ops, writes, 0);
            self.reap(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc::sync_channel;
    use tasty_memory::writer::{ChangeFeed, MemoryWriter, SharedStore};
    use tasty_utils::batch_writer::{Limits, SystemClock};

    fn item(n: u64) -> ParsedItem {
        ParsedItem {
            kind: "path",
            line: 0,
            byte_start: 0,
            byte_end: 0,
            data: json!({ "n": n }),
        }
    }

    fn store() -> Arc<Mutex<tasty_memory::MemoryStore>> {
        Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ))
    }

    fn writer(
        store: &Arc<Mutex<tasty_memory::MemoryStore>>,
        limits: Limits,
    ) -> (MemoryWriter<SharedStore>, MemoryAccess) {
        let shared: Arc<Mutex<dyn tasty_memory::MemoryStorage>> = store.clone();
        let changes = Arc::new(ChangeFeed::default());
        let writer = tasty_memory::writer::spawn(
            SharedStore(shared),
            changes.clone(),
            limits,
            Arc::new(SystemClock),
            Arc::new(|| {}),
        )
        .unwrap();
        let access = MemoryAccess::writer(writer.handle(), changes);
        (writer, access)
    }

    fn records(store: &Arc<Mutex<tasty_memory::MemoryStore>>, id: ObserverId) -> Vec<u64> {
        store
            .lock()
            .unwrap()
            .list(
                &Scope::Global,
                &tasty_memory::ListOpts {
                    prefix: Some(format!("tasty.observer.{id}.")),
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|e| match e.value {
                MemoryValue::Json(v) => v["data"]["n"].as_u64().unwrap(),
                other => panic!("observer record is JSON: {other:?}"),
            })
            .collect()
    }

    fn commits(store: &Arc<Mutex<tasty_memory::MemoryStore>>) -> u64 {
        store.lock().unwrap().db_latency().snapshot().commits
    }

    /// 채널에 함께 쌓인 기록은 한 요청 묶음, 곧 commit 한 번으로 저장된다.
    #[test]
    fn a_burst_waiting_in_the_channel_lands_in_one_commit() {
        let store = store();
        let (mut writer, access) = writer(&store, Limits::default());
        let (tx, rx) = sync_channel::<ParsedItem>(BATCH_RECORDS);
        for n in 0..100 {
            tx.send(item(n)).unwrap();
        }
        drop(tx);
        let before = commits(&store);
        let tally = Arc::new(SinkTally::default());
        run(51, 0, rx, access, tally.clone());
        assert_eq!(records(&store, 51), (0..100).collect::<Vec<_>>());
        assert_eq!(commits(&store) - before, 1, "100 records, one commit");
        assert_eq!(tally.submit_rejected.load(Ordering::Relaxed), 0);
        assert_eq!(tally.store_failed.load(Ordering::Relaxed), 0);
        writer.begin_shutdown();
        while writer.poll_shutdown() != 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// 보존 삭제는 확정된 키만 세고 다음 묶음에 실린다. 채널이 끊긴 뒤 남은 삭제도 실행한다.
    #[test]
    fn retention_deletes_follow_confirmed_records_through_the_writer() {
        let store = store();
        let (_writer, access) = writer(&store, Limits::default());
        let (tx, rx) = sync_channel::<ParsedItem>(BATCH_RECORDS);
        for n in 0..10 {
            tx.send(item(n)).unwrap();
        }
        drop(tx);
        run(52, 3, rx, access, Default::default());
        assert_eq!(records(&store, 52), vec![7, 8, 9]);
    }

    /// writer가 접수하지 않은 기록은 submit_rejected로 센다. 대신 저장하는 경로는 없다.
    #[test]
    fn records_the_writer_does_not_accept_are_counted_apart_from_store_failures() {
        let store = store();
        let (_writer, access) = writer(
            &store,
            Limits {
                queue_items: 0,
                ..Limits::default()
            },
        );
        let (tx, rx) = sync_channel::<ParsedItem>(BATCH_RECORDS);
        for n in 0..5 {
            tx.send(item(n)).unwrap();
        }
        drop(tx);
        let tally = Arc::new(SinkTally::default());
        run(53, 0, rx, access, tally.clone());
        assert!(records(&store, 53).is_empty());
        assert_eq!(tally.submit_rejected.load(Ordering::Relaxed), 5);
        assert_eq!(tally.store_failed.load(Ordering::Relaxed), 0);
    }

    /// 요청 자신의 거절(크기 초과)은 그 기록만 store_failed로 세고 나머지는 저장한다.
    #[test]
    fn a_rejected_record_is_a_store_failure_and_its_neighbours_are_kept() {
        let config = tasty_memory::MemoryConfig {
            entry_max_bytes: 512,
            ..Default::default()
        };
        let store = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory_with_config(config).unwrap(),
        ));
        let (_writer, access) = writer(&store, Limits::default());
        let (tx, rx) = sync_channel::<ParsedItem>(4);
        let mut big = item(0);
        big.data = json!({ "n": 0, "big": "x".repeat(4096) });
        tx.send(big).unwrap();
        tx.send(item(1)).unwrap();
        drop(tx);
        let tally = Arc::new(SinkTally::default());
        run(54, 0, rx, access, tally.clone());
        assert_eq!(records(&store, 54), vec![1]);
        assert_eq!(tally.store_failed.load(Ordering::Relaxed), 1);
        assert_eq!(tally.submit_rejected.load(Ordering::Relaxed), 0);
    }
}
