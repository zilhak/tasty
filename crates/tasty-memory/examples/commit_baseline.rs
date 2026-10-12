//! 파일 memory.db의 commit 수와 호출 대기를 재는 기준선 harness.
//!
//! 호스트처럼 `Arc<Mutex<MemoryStore>>`를 공유하고 같은 payload로 단일 요청, 100개 burst,
//! 연속 유입, 느린 저장 대조군, 100건을 transaction 하나로 묶는 batch·실행 단위를 실행한다.
//! 같은 burst·유입·느린 저장 대조군을 memory writer에 넣는 시나리오도 수집 창(`max_delay`)별로
//! 실행한다. 결과는 시나리오마다 JSON 한 줄로 출력 파일에 쓴다.
//! 실행법과 결과 해석은 docs/design/systems/storage.md의 "저장 기준선" 절에 있다.
//!
//! 사용: `cargo run --release -p tasty-memory --example commit_baseline -- <빈 디렉터리> <결과.jsonl>`

#[path = "commit_baseline/writer.rs"]
mod writer;

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_memory::{
    BatchMode, MemoryConfig, MemoryStorage, MemoryStore, MemoryValue, PutOpts, STORE_LOCK_POISONED,
    STORE_LOCK_WHAT, Scope, WriteOp,
};

/// 미리 채우는 키 수. 빈 DB보다 실제 사용 상태에 가깝게 B-tree 깊이를 만든다.
const PREFILL: usize = 1000;
/// 단일 요청 표본 수와 표본 사이 간격.
const SINGLE_SAMPLES: usize = 200;
const SINGLE_GAP: Duration = Duration::from_millis(5);
/// burst 크기.
const BURST: usize = 100;
/// 연속 유입의 요청 간격과 지속 시간.
const INFLOW_GAP: Duration = Duration::from_millis(2);
const INFLOW_FOR: Duration = Duration::from_secs(3);
/// 메인 루프 대리 측정: 이 간격마다 저장소 잠금을 잡고 get 한 번을 한다.
const TICK: Duration = Duration::from_millis(1);
/// 느린 저장 대조군에서 쓰기마다 더하는 지연. 디스크 지연을 흉내 내는 주입값이다.
const SLOW_WRITE: Duration = Duration::from_millis(5);
/// writer 시나리오의 수집 창 변형. 가운데 값이 `Limits::default()`의 값이다.
const WRITER_DELAYS: [Duration; 3] = [
    Duration::from_millis(1),
    Duration::from_millis(2),
    Duration::from_millis(5),
];
/// batch·실행 단위 시나리오의 반복 횟수. 회차마다 BURST건을 transaction 하나로 쓴다.
const BATCH_ROUNDS: usize = 20;

fn payload() -> MemoryValue {
    // 직렬화하면 236바이트인 JSON. 모든 시나리오가 같은 값을 쓴다.
    MemoryValue::Json(json!({
        "kind": "baseline",
        "text": "x".repeat(200),
        "n": 42,
    }))
}

/// 쓰기 뒤에 지연을 더하는 저장소. 잠금을 잡은 채 기다리므로 느린 디스크에서 뒤 요청이
/// 줄을 서는 현재 구조를 재현한다.
struct SlowStore {
    inner: MemoryStore,
    delay: Duration,
}

fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    tasty_utils::poison::recover_mutex(m.lock(), STORE_LOCK_WHAT, &STORE_LOCK_POISONED)
}

trait Put: Send {
    fn put_one(&mut self, key: &str) -> u64;
    fn get_one(&self, key: &str);
}

/// 여러 쓰기를 transaction 하나로 보내는 방식.
#[derive(Clone, Copy)]
enum Grouping {
    Batch(BatchMode),
    Unit,
}

fn put_many(store: &mut MemoryStore, keys: &[String], grouping: Grouping) {
    match grouping {
        Grouping::Batch(mode) => {
            let ops: Vec<WriteOp> = keys
                .iter()
                .map(|key| WriteOp::Put {
                    owner: "_host".into(),
                    scope: Scope::Global,
                    key: key.clone(),
                    value: payload(),
                    opts: PutOpts::default(),
                })
                .collect();
            let results = store.write_batch(mode, &ops).expect("write_batch");
            assert!(results.iter().all(|r| r.is_ok()), "batch request rejected");
        }
        Grouping::Unit => store
            .run_unit(|s| {
                for key in keys {
                    s.put_one(key);
                }
                Ok(())
            })
            .expect("run_unit"),
    }
}

impl Put for MemoryStore {
    fn put_one(&mut self, key: &str) -> u64 {
        MemoryStorage::put(
            self,
            "_host",
            &Scope::Global,
            key,
            &payload(),
            &PutOpts::default(),
        )
        .expect("put")
    }
    fn get_one(&self, key: &str) {
        MemoryStorage::get(self, &Scope::Global, key).expect("get");
    }
}

impl Put for SlowStore {
    fn put_one(&mut self, key: &str) -> u64 {
        let v = self.inner.put_one(key);
        thread::sleep(self.delay);
        v
    }
    fn get_one(&self, key: &str) {
        self.inner.get_one(key);
    }
}

/// 공유 저장소 하나로 실행하는 시나리오.
type Scenario = fn(&Arc<Mutex<dyn Put>>) -> Value;

#[derive(Default)]
struct Dist(Vec<u64>);

impl Dist {
    fn push(&mut self, d: Duration) {
        self.0
            .push(u64::try_from(d.as_micros()).unwrap_or(u64::MAX));
    }
    fn json(&mut self) -> Value {
        self.0.sort_unstable();
        let n = self.0.len();
        if n == 0 {
            return json!({ "n": 0 });
        }
        let at = |q: f64| self.0[((n as f64 - 1.0) * q).round() as usize];
        json!({
            "n": n,
            "p50_us": at(0.50),
            "p95_us": at(0.95),
            "p99_us": at(0.99),
            "max_us": self.0[n - 1],
            "mean_us": self.0.iter().sum::<u64>() / n as u64,
        })
    }
}

fn open(dir: &Path, name: &str) -> MemoryStore {
    let path = dir.join(name);
    MemoryStore::open_with_config(&path, MemoryConfig::default()).expect("open file memory.db")
}

fn prefill(store: &mut MemoryStore) {
    for i in 0..PREFILL {
        store.put_one(&format!("prefill.{i:06}"));
    }
}

fn pragmas(store: &MemoryStore) -> Value {
    let p = store.applied_pragmas();
    let mut out = serde_json::Map::new();
    for r in &p.readings {
        out.insert(r.name.to_string(), json!(r.effective));
    }
    json!({ "in_memory": p.in_memory, "degraded": p.degraded(), "effective": out })
}

/// 고립된 단일 요청. 다른 요청과 겹치지 않으므로 잠금 대기가 없다.
fn single(store: &Arc<Mutex<dyn Put>>) -> Value {
    let mut d = Dist::default();
    for i in 0..SINGLE_SAMPLES {
        let t = Instant::now();
        lock(store).put_one(&format!("single.{i:06}"));
        d.push(t.elapsed());
        thread::sleep(SINGLE_GAP);
    }
    json!({ "latency": d.json() })
}

/// BURST개 스레드가 장벽에서 동시에 출발해 한 번씩 put 한다. IPC 요청이 몰린 경우다.
fn burst_concurrent(store: &Arc<Mutex<dyn Put>>, tag: &str) -> Value {
    let barrier = Arc::new(Barrier::new(BURST + 1));
    let mut hs = Vec::new();
    for i in 0..BURST {
        let s = store.clone();
        let b = barrier.clone();
        let key = format!("{tag}.{i:06}");
        hs.push(thread::spawn(move || {
            b.wait();
            let t = Instant::now();
            let wait_started = Instant::now();
            let mut g = lock(&s);
            let waited = wait_started.elapsed();
            g.put_one(&key);
            (t.elapsed(), waited)
        }));
    }
    barrier.wait();
    let wall = Instant::now();
    let mut lat = Dist::default();
    let mut wait = Dist::default();
    for h in hs {
        let (l, w) = h.join().expect("burst thread");
        lat.push(l);
        wait.push(w);
    }
    json!({
        "wall_us": u64::try_from(wall.elapsed().as_micros()).unwrap_or(u64::MAX),
        "latency": lat.json(),
        "lock_wait": wait.json(),
    })
}

/// 한 스레드가 BURST개를 연달아 put 한다. observer sink처럼 한 소비자가 쏟아내는 경우다.
fn burst_sequential(store: &Arc<Mutex<dyn Put>>) -> Value {
    let mut lat = Dist::default();
    let wall = Instant::now();
    for i in 0..BURST {
        let t = Instant::now();
        lock(store).put_one(&format!("seq.{i:06}"));
        lat.push(t.elapsed());
    }
    json!({
        "wall_us": u64::try_from(wall.elapsed().as_micros()).unwrap_or(u64::MAX),
        "latency": lat.json(),
    })
}

/// 생산자가 INFLOW_GAP마다 put 하는 동안 메인 루프 대리 스레드가 TICK마다 잠금을 잡고
/// get 한다. 대리 스레드의 잠금 대기와 tick 지연이 업무 루프가 받는 영향이다.
fn inflow(store: &Arc<Mutex<dyn Put>>) -> Value {
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
                lock(&s).get_one("prefill.000000");
                wait.push(t.elapsed());
                next += TICK;
                if next < Instant::now() {
                    next = Instant::now() + TICK;
                }
            }
            (wait, late)
        })
    };
    let mut lat = Dist::default();
    let started = Instant::now();
    let mut i = 0usize;
    while started.elapsed() < INFLOW_FOR {
        let t = Instant::now();
        lock(store).put_one(&format!("inflow.{i:06}"));
        lat.push(t.elapsed());
        i += 1;
        thread::sleep(INFLOW_GAP);
    }
    stop.store(true, Ordering::Relaxed);
    let (mut wait, mut late) = ticker.join().expect("ticker");
    json!({
        "writes": i,
        "write_latency": lat.json(),
        "loop_get_including_lock_wait": wait.json(),
        "loop_tick_lateness": late.json(),
    })
}

/// BURST건을 transaction 하나로 쓰는 회차를 BATCH_ROUNDS번 반복한다. 회차마다 잠금 한 번이다.
fn grouped(store: &mut MemoryStore, tag: &str, grouping: Grouping) -> Value {
    let mut lat = Dist::default();
    let wall = Instant::now();
    for round in 0..BATCH_ROUNDS {
        let keys: Vec<String> = (0..BURST)
            .map(|i| format!("{tag}.{round:03}.{i:06}"))
            .collect();
        let t = Instant::now();
        put_many(store, &keys, grouping);
        lat.push(t.elapsed());
    }
    json!({
        "rounds": BATCH_ROUNDS,
        "writes_per_round": BURST,
        "wall_us": u64::try_from(wall.elapsed().as_micros()).unwrap_or(u64::MAX),
        "round_latency": lat.json(),
    })
}

/// 정리 경로(TTL 정리·prune·scope 정리)를 실행하고 각 호출이 센 commit 수를 본다.
/// 행을 지운 정리 호출은 commit 1회로 세어진다.
fn maintenance_paths(store: &mut MemoryStore) -> Value {
    let lat = store.db_latency();
    let before = lat.snapshot().commits;
    let expired = MemoryStorage::put(
        store,
        "_host",
        &Scope::Global,
        "ttl.000000",
        &payload(),
        &PutOpts {
            expires_at: Some(1),
            cas: None,
        },
    )
    .expect("put ttl");
    MemoryStorage::put(
        store,
        "_host",
        &Scope::Surface(1),
        "scoped.000000",
        &payload(),
        &PutOpts::default(),
    )
    .expect("put scoped");
    let after_put = lat.snapshot().commits;
    let purged = MemoryStorage::purge_expired(store).expect("purge_expired");
    let after_purge = lat.snapshot().commits;
    let pruned = MemoryStorage::prune_prefix_keep_recent(store, "single.", 10).expect("prune");
    let after_prune = lat.snapshot().commits;
    let scope_purged = MemoryStorage::purge_scope(store, &Scope::Surface(1)).expect("purge_scope");
    let after_scope = lat.snapshot().commits;
    json!({
        "put_version": expired,
        "commits_by_puts": after_put - before,
        "purge_expired_rows": purged.regular + purged.secret,
        "commits_by_purge_expired": after_purge - after_put,
        "prune_rows": pruned,
        "commits_by_prune": after_prune - after_purge,
        "purge_scope_rows": scope_purged.regular + scope_purged.secret,
        "commits_by_purge_scope": after_scope - after_prune,
    })
}

/// 시나리오 결과를 한 줄씩 결과 파일에 쓴다. 중간에 실패해도 앞 시나리오의 결과는 남는다.
struct Out(File);

impl Out {
    fn line(&mut self, v: &Value) {
        let mut buf = serde_json::to_vec(v).expect("serialize result");
        buf.push(b'\n');
        self.0.write_all(&buf).expect("write result");
        self.0.flush().expect("flush result");
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let usage = "usage: commit_baseline <empty dir> <result.jsonl>";
    let dir = args.next().expect(usage);
    let out_path = args.next().expect(usage);
    std::fs::create_dir_all(&dir).expect("create dir");
    let mut out = Out(File::create(&out_path).expect("create result file"));

    let fast_db = {
        let mut s = open(&dir, "fast.db");
        prefill(&mut s);
        s
    };
    let lat = fast_db.db_latency();
    out.line(&json!({
            "scenario": "setup",
            "db": dir.join("fast.db"),
            "prefill": PREFILL,
            "payload_bytes": serde_json::to_vec(&match payload() {
                MemoryValue::Json(v) => v,
                _ => Value::Null,
            }).map(|b| b.len()).unwrap_or(0),
            "pragmas": pragmas(&fast_db),
            "prefill_commits": lat.snapshot().commits,
    }));
    let fast: Arc<Mutex<dyn Put>> = Arc::new(Mutex::new(fast_db));
    let runs: [(&str, Scenario); 4] = [
        ("single", single),
        ("burst100_concurrent", |s| burst_concurrent(s, "burst")),
        ("burst100_sequential", burst_sequential),
        ("inflow", inflow),
    ];
    for (name, f) in runs {
        let c0 = lat.snapshot();
        let mut v = f(&fast);
        let c1 = lat.snapshot();
        v["scenario"] = json!(name);
        v["commits"] = json!(c1.commits - c0.commits);
        v["commit_us_sum"] = json!(c1.commit_us_sum - c0.commit_us_sum);
        out.line(&v);
    }

    let slow_db = {
        let mut s = open(&dir, "slow.db");
        prefill(&mut s);
        s
    };
    let slow_lat = slow_db.db_latency();
    let slow: Arc<Mutex<dyn Put>> = Arc::new(Mutex::new(SlowStore {
        inner: slow_db,
        delay: SLOW_WRITE,
    }));
    for (name, f) in [
        (
            "slow_burst100_concurrent",
            burst_concurrent as fn(&_, &str) -> Value,
        ),
        ("slow_inflow", |s, _| inflow(s)),
    ] {
        let c0 = slow_lat.snapshot().commits;
        let mut v = f(&slow, "slowburst");
        v["scenario"] = json!(name);
        v["injected_write_delay_us"] = json!(SLOW_WRITE.as_micros() as u64);
        v["commits"] = json!(slow_lat.snapshot().commits - c0);
        out.line(&v);
    }
    drop(slow);

    // 앞 시나리오의 쓰기를 모두 받은 연결을 닫는다. 이 연결이 파일의 마지막 연결이라
    // drop에 WAL checkpoint가 포함된다. 다른 스레드는 모두 join돼 참조가 남아 있지 않다.
    let wal_bytes = wal_len(&dir, "fast.db");
    let t = Instant::now();
    drop(fast);
    out.line(&json!({
        "scenario": "close",
        "connection": "fast.db after all fast scenarios",
        "wal_bytes_before": wal_bytes,
        "drop_us": u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX),
    }));

    // dyn Put에서 MemoryStore를 되찾을 수 없으므로 같은 파일을 다시 연다.
    let mut store = open(&dir, "fast.db");
    let store_lat = store.db_latency();
    for (name, grouping) in [
        (
            "batch100_independent",
            Grouping::Batch(BatchMode::Independent),
        ),
        ("batch100_atomic", Grouping::Batch(BatchMode::Atomic)),
        ("unit100", Grouping::Unit),
    ] {
        let c0 = store_lat.snapshot();
        let mut v = grouped(&mut store, name, grouping);
        let c1 = store_lat.snapshot();
        v["scenario"] = json!(name);
        v["commits"] = json!(c1.commits - c0.commits);
        v["commit_us_sum"] = json!(c1.commit_us_sum - c0.commit_us_sum);
        out.line(&v);
    }
    let mut v = maintenance_paths(&mut store);
    v["scenario"] = json!("maintenance_paths");
    out.line(&v);

    let wal_bytes = wal_len(&dir, "fast.db");
    let t = Instant::now();
    drop(store);
    out.line(&json!({
        "scenario": "close_reopened",
        "connection": "fast.db reopened for batch, unit and maintenance_paths",
        "wal_bytes_before": wal_bytes,
        "drop_us": u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX),
    }));

    writer_scenarios(&dir, &mut out);
}

/// 직접 잠금 시나리오와 같은 부하를 writer에 넣는다. 저장소는 따로 연 파일이다.
fn writer_scenarios(dir: &Path, out: &mut Out) {
    for (file, slow) in [("writer.db", None), ("writer_slow.db", Some(SLOW_WRITE))] {
        let mut db = open(dir, file);
        prefill(&mut db);
        let lat = db.db_latency();
        let store: Arc<Mutex<dyn MemoryStorage>> = Arc::new(Mutex::new(db));
        let prefix = if slow.is_some() {
            "writer_slow"
        } else {
            "writer"
        };
        for delay in WRITER_DELAYS {
            let ms = delay.as_millis();
            for name in ["burst100_concurrent", "inflow"] {
                let body = |h: &_, s: &_| match name {
                    "inflow" => writer::inflow(h, s, &format!("winflow{ms}")),
                    _ => writer::burst_concurrent(h, &format!("wburst{ms}")),
                };
                let c0 = lat.snapshot();
                let mut v = writer::run(&store, delay, slow, body);
                let c1 = lat.snapshot();
                v["scenario"] = json!(format!("{prefix}_{name}"));
                v["commits"] = json!(c1.commits - c0.commits);
                v["commit_us_sum"] = json!(c1.commit_us_sum - c0.commit_us_sum);
                if let Some(d) = slow {
                    v["injected_commit_delay_us"] = json!(d.as_micros() as u64);
                }
                out.line(&v);
            }
        }
    }
}

/// WAL 파일 크기. 없으면 0이다.
fn wal_len(dir: &Path, name: &str) -> u64 {
    std::fs::metadata(dir.join(format!("{name}-wal")))
        .map(|m| m.len())
        .unwrap_or(0)
}
