//! 같은 쓰기를 memory writer에 넣어 직접 잠금 시나리오와 비교한다.
//!
//! writer는 호스트처럼 `SharedStore`로 저장소에 닿고, 메인 루프 대리 스레드는 같은 잠금을 직접
//! 잡는다. 호출 스레드가 기다리는 시간(`submit`)과 결과가 준비되기까지의 시간(`completion`)을
//! 따로 잰다. 느린 저장 대조군은 묶음(= commit)마다 잠금을 잡은 채 지연을 더한다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_memory::writer::{MemoryRequest, SharedStore, StoreAccess, spawn};
use tasty_memory::{MemoryStorage, PutOpts, Scope, WriteOp};
use tasty_utils::batch_writer::{
    BATCH_SIZE_BUCKETS, Completion, Handle, Limits, Receipt, SystemClock, WriterStatsSnapshot,
};

use super::{BURST, Dist, INFLOW_FOR, INFLOW_GAP, TICK, lock, payload};

/// 결과를 기다리는 한도. 넘으면 시나리오를 실패로 끝낸다.
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);

/// 묶음마다 잠금을 잡은 채 지연을 더하는 접근. 직접 잠금 대조군의 쓰기마다 지연과 같은
/// commit마다 지연이다.
pub struct SlowAccess {
    pub inner: SharedStore,
    pub delay: Option<Duration>,
}

impl StoreAccess for SlowAccess {
    fn with_store<R>(&mut self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        let delay = self.delay;
        self.inner.with_store(|s| {
            let r = f(s);
            if let Some(d) = delay {
                thread::sleep(d);
            }
            r
        })
    }
}

type MemHandle = Handle<tasty_memory::writer::MemoryExecutor<SlowAccess>>;
type MemReceipt = Receipt<tasty_memory::writer::MemoryCompletion>;

fn put_request(key: String) -> MemoryRequest {
    MemoryRequest::Write(WriteOp::Put {
        owner: "_host".into(),
        scope: Scope::Global,
        key,
        value: payload(),
        opts: PutOpts::default(),
    })
}

fn us(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

fn wait_done(receipt: &MemReceipt) {
    match receipt.wait_timeout(COMPLETION_TIMEOUT) {
        Some(Completion::Done(Ok(_))) => {}
        other => panic!("writer request did not commit: {other:?}"),
    }
}

/// 한 writer로 한 시나리오를 실행하고 writer 계측 차분을 붙인다.
pub fn run(
    store: &Arc<Mutex<dyn MemoryStorage>>,
    max_delay: Duration,
    slow: Option<Duration>,
    body: impl FnOnce(&MemHandle, &Arc<Mutex<dyn MemoryStorage>>) -> Value,
) -> Value {
    let limits = Limits {
        max_delay,
        ..Limits::default()
    };
    let access = SlowAccess {
        inner: SharedStore(store.clone()),
        delay: slow,
    };
    let mut writer = spawn(
        access,
        Arc::default(),
        limits,
        Arc::new(SystemClock),
        Arc::new(|| {}),
    )
    .expect("spawn writer");
    let mut v = body(&writer.handle(), store);
    let stats = writer.stats();
    let t = Instant::now();
    writer.begin_shutdown();
    while writer.poll_shutdown() != 0 {
        thread::sleep(Duration::from_millis(1));
    }
    v["shutdown_us"] = json!(us(t.elapsed()));
    v["max_delay_us"] = json!(us(max_delay));
    v["writer"] = stats_json(&stats);
    v
}

fn stats_json(s: &WriterStatsSnapshot) -> Value {
    json!({
        "admitted": s.admitted,
        "rejected_saturated": s.rejected_saturated,
        "batches": s.batches,
        "batch_size_max": s.batch_size_max,
        "batch_sizes": { "le": BATCH_SIZE_BUCKETS, "counts": s.batch_sizes },
        "retries": s.retries,
        "peak_items": s.peak_items,
        "peak_bytes": s.peak_bytes,
    })
}

/// BURST개 스레드가 장벽에서 동시에 출발해 한 번씩 접수하고 결과를 기다린다.
pub fn burst_concurrent(handle: &MemHandle, tag: &str) -> Value {
    let barrier = Arc::new(Barrier::new(BURST + 1));
    let mut hs = Vec::new();
    for i in 0..BURST {
        let h = handle.clone();
        let b = barrier.clone();
        let key = format!("{tag}.{i:06}");
        hs.push(thread::spawn(move || {
            b.wait();
            let t = Instant::now();
            let receipt = h.submit(put_request(key)).expect("submit");
            let submitted = t.elapsed();
            wait_done(&receipt);
            (submitted, t.elapsed())
        }));
    }
    barrier.wait();
    let wall = Instant::now();
    let mut submit = Dist::default();
    let mut done = Dist::default();
    for h in hs {
        let (s, d) = h.join().expect("burst thread");
        submit.push(s);
        done.push(d);
    }
    json!({
        "wall_us": us(wall.elapsed()),
        "submit": submit.json(),
        "completion": done.json(),
    })
}

/// 생산자가 INFLOW_GAP마다 접수하고 결과는 수거 스레드가 기다린다. 메인 루프 대리 스레드는
/// TICK마다 같은 저장소 잠금을 직접 잡고 get 한다.
pub fn inflow(handle: &MemHandle, store: &Arc<Mutex<dyn MemoryStorage>>, tag: &str) -> Value {
    let stop = Arc::new(AtomicBool::new(false));
    let ticker = {
        let s = store.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            let mut wait = Dist::default();
            let mut late = Dist::default();
            let mut next = Instant::now() + TICK;
            while !stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                if now < next {
                    thread::sleep(next - now);
                }
                late.push(Instant::now().saturating_duration_since(next));
                let t = Instant::now();
                lock(&s).get(&Scope::Global, "prefill.000000").expect("get");
                wait.push(t.elapsed());
                next += TICK;
                if next < Instant::now() {
                    next = Instant::now() + TICK;
                }
            }
            (wait, late)
        })
    };
    let (tx, rx) = mpsc::channel::<(Instant, MemReceipt)>();
    let collector = thread::spawn(move || {
        let mut done = Dist::default();
        for (t, receipt) in rx {
            wait_done(&receipt);
            done.push(t.elapsed());
        }
        done
    });
    let mut submit = Dist::default();
    let started = Instant::now();
    let mut i = 0usize;
    while started.elapsed() < INFLOW_FOR {
        let t = Instant::now();
        let receipt = handle
            .submit(put_request(format!("{tag}.{i:06}")))
            .expect("submit");
        submit.push(t.elapsed());
        tx.send((t, receipt)).expect("collector alive");
        i += 1;
        thread::sleep(INFLOW_GAP);
    }
    drop(tx);
    let mut done = collector.join().expect("collector");
    stop.store(true, Ordering::Relaxed);
    let (mut wait, mut late) = ticker.join().expect("ticker");
    json!({
        "writes": i,
        "submit": submit.json(),
        "completion": done.json(),
        "loop_get_including_lock_wait": wait.json(),
        "loop_tick_lateness": late.json(),
    })
}
