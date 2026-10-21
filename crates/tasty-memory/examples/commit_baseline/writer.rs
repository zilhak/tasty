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
    BATCH_SIZE_BUCKETS, Completion, Handle, Limits, Receipt, SystemClock, WakeLatch,
    WriterStatsSnapshot,
};

use super::{BURST, Dist, INFLOW_FOR, INFLOW_GAP, TICK, lock, payload};

/// 결과를 기다리는 한도. 넘으면 시나리오를 실패로 끝낸다.
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
/// 깨우기 묶음 측정에서 메인 루프 대리가 깨어날 때마다 쓰는 회차 시간. 기본 1ms이고
/// `COMMIT_BASELINE_LOOP_ROUND_MS`로 바꾼다(느린 메인 루프 대조).
fn loop_round() -> Duration {
    std::env::var("COMMIT_BASELINE_LOOP_ROUND_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(Duration::from_millis(1), Duration::from_millis)
}

/// 호스트처럼 worker 알림을 `WakeLatch`로 묶고, 대리 소비자가 깨어날 때마다 확인한 뒤 한 회차를
/// 쓴다. 알림 수와 실제로 보낸 깨우기 수를 비교한다.
struct WakeProbe {
    latch: Arc<WakeLatch>,
    stop: Arc<AtomicBool>,
    consumer: thread::JoinHandle<u64>,
}

impl WakeProbe {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        let latch = WakeLatch::new(Arc::new(move || {
            if tx.send(()).is_err() {
                // 소비자가 먼저 끝난 시나리오 끝이다.
            }
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let round = loop_round();
        let consumer = {
            let latch = latch.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                let mut rounds = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    if rx.recv_timeout(Duration::from_millis(10)).is_ok() {
                        latch.acknowledge();
                        rounds += 1;
                        thread::sleep(round);
                    }
                }
                rounds
            })
        };
        Self {
            latch,
            stop,
            consumer,
        }
    }

    fn finish(self) -> Value {
        self.stop.store(true, Ordering::Relaxed);
        let rounds = self.consumer.join().expect("wake consumer");
        let counts = self.latch.counts();
        json!({
            "notified": counts.notified,
            "woken": counts.woken,
            "consumer_rounds": rounds,
            "loop_round_us": us(loop_round()),
        })
    }
}

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
    let probe = WakeProbe::start();
    let mut writer = spawn(
        access,
        Arc::default(),
        limits,
        Arc::new(SystemClock),
        probe.latch.notifier(),
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
    v["wake"] = probe.finish();
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

/// 요청 게이트 대리. `inflow`와 같은 쓰기 유입 중에 메인 루프 대리 스레드가 TICK마다 결과를
/// 기다리지 않고 확인만 한다. 진행 중인 게이트가 없으면 세션 판정 대리(G: 읽기 실행 단위)를 넣고,
/// G가 끝나면 기록 대리(R: 읽기와 쓰기 실행 단위)를 넣는다. 루프 회차마다 접수·확인에 쓴 시간,
/// G 완료, G 접수부터 R 완료까지를 잰다. 게이트는 한 번에 하나만 진행한다(접수 순서 보존).
pub fn gate_inflow(handle: &MemHandle, tag: &str) -> Value {
    let stop = Arc::new(AtomicBool::new(false));
    let gate = {
        let h = handle.clone();
        let stop = stop.clone();
        let tag = tag.to_string();
        thread::spawn(move || {
            let mut round = Dist::default();
            let mut g_done = Dist::default();
            let mut g_r_done = Dist::default();
            let mut late = Dist::default();
            let mut inflight: Option<(Instant, bool, MemReceipt)> = None;
            let mut n = 0usize;
            let mut next = Instant::now() + TICK;
            while !stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                if now < next {
                    thread::sleep(next - now);
                }
                late.push(Instant::now().saturating_duration_since(next));
                let t = Instant::now();
                inflight = match inflight.take() {
                    None => {
                        let g = MemoryRequest::unit(0, |s| {
                            s.get(&Scope::Global, "prefill.000000").map(|_| ())
                        });
                        Some((t, false, h.submit(g).expect("submit G")))
                    }
                    Some((started, recorded, receipt)) => match receipt.try_take() {
                        None => Some((started, recorded, receipt)),
                        Some(Completion::Done(Ok(_))) if !recorded => {
                            g_done.push(started.elapsed());
                            let key = format!("{tag}.{n:06}");
                            n += 1;
                            let r = MemoryRequest::unit(0, move |s| {
                                s.get(&Scope::Global, "prefill.000001")?;
                                s.put(
                                    "_host",
                                    &Scope::Global,
                                    &key,
                                    &payload(),
                                    &PutOpts::default(),
                                )
                                .map(|_| ())
                            });
                            Some((started, true, h.submit(r).expect("submit R")))
                        }
                        Some(Completion::Done(Ok(_))) => {
                            g_r_done.push(started.elapsed());
                            None
                        }
                        Some(other) => panic!("gate request did not commit: {other:?}"),
                    },
                };
                round.push(t.elapsed());
                next += TICK;
                if next < Instant::now() {
                    next = Instant::now() + TICK;
                }
            }
            (round, g_done, g_r_done, late)
        })
    };
    let (tx, rx) = mpsc::channel::<MemReceipt>();
    let collector = thread::spawn(move || {
        for receipt in rx {
            wait_done(&receipt);
        }
    });
    let started = Instant::now();
    let mut i = 0usize;
    while started.elapsed() < INFLOW_FOR {
        let receipt = handle
            .submit(put_request(format!("{tag}.load.{i:06}")))
            .expect("submit");
        tx.send(receipt).expect("collector alive");
        i += 1;
        thread::sleep(INFLOW_GAP);
    }
    drop(tx);
    collector.join().expect("collector");
    stop.store(true, Ordering::Relaxed);
    let (mut round, mut g_done, mut g_r_done, mut late) = gate.join().expect("gate loop");
    json!({
        "writes": i,
        "loop_round_without_waiting": round.json(),
        "gate_session_completion": g_done.json(),
        "gate_session_and_record_completion": g_r_done.json(),
        "loop_tick_lateness": late.json(),
    })
}
