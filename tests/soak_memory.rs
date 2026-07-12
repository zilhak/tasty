//! 메모리 누수 soak 하네스 — 장시간(시간~하루) 반복 워크로드를 돌리며 4계층
//! 지표(트리 RSS / 핸들·fd / 자식 프로세스 / GPU 리소스 카운트)를 JSONL 로 기록한다.
//!
//! CI 대상이 아니다 — `#[ignore]` 라 명시 실행으로만 돈다:
//!
//! ```bash
//! SOAK_SCENARIO=s9 SOAK_DURATION_SECS=86400 \
//!   cargo test --release --test soak_memory -- --ignored --nocapture
//! ```
//!
//! 판정은 하네스가 하지 않는다 — `scripts/soak/analyze.py` 가 JSONL 을 읽어
//! warmup 제외 OLS 기울기 + 기준선 복귀를 판정한다. 절차 전체:
//! `docs/dev-guide/memory-leak-soak.md`.
//!
//! env:
//! - `SOAK_SCENARIO`       s1|s4|s7|s8|s9 (기본 s9=mixed)
//! - `SOAK_DURATION_SECS`  soak 시간 (기본 600)
//! - `SOAK_CYCLES`         사이클 수 상한 (기본 무제한 — 시간으로만 종료)
//! - `SOAK_CHECKPOINT_EVERY` 체크포인트 간격(사이클, 기본 10)
//! - `SOAK_OUT_DIR`        JSONL 출력 디렉토리 (기본 .claude-workspace/temp/soak)

mod common;

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TastyInstance;
use serde_json::{Value, json};

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ── 외부 측정 (sysinfo + 플랫폼별 핸들/fd) ──────────────────────────────

/// tasty 루트 + 모든 자손 프로세스의 RSS 합산과 이름별 자손 카운트.
/// 셸/conhost/plugin 프로세스 누수(L4)와 트리 전체 메모리(L2)를 함께 본다.
fn sample_process_tree(root_pid: u32) -> Value {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let procs = sys.processes();

    // parent → children 인덱스를 만들어 루트에서 BFS.
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (pid, p) in procs {
        if let Some(parent) = p.parent() {
            children
                .entry(parent.as_u32())
                .or_default()
                .push(pid.as_u32());
        }
    }
    let mut tree: Vec<u32> = vec![root_pid];
    let mut queue: Vec<u32> = vec![root_pid];
    while let Some(pid) = queue.pop() {
        if let Some(kids) = children.get(&pid) {
            for k in kids {
                tree.push(*k);
                queue.push(*k);
            }
        }
    }

    let mut rss_total: u64 = 0;
    let mut root_rss: u64 = 0;
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for pid in &tree {
        if let Some(p) = procs.get(&sysinfo::Pid::from_u32(*pid)) {
            rss_total += p.memory();
            if *pid == root_pid {
                root_rss = p.memory();
            } else {
                *by_name
                    .entry(p.name().to_string_lossy().into_owned())
                    .or_insert(0) += 1;
            }
        }
    }
    json!({
        "rss_tree_bytes": rss_total,
        "rss_root_bytes": root_rss,
        "proc_count": tree.len(),
        "children_by_name": by_name,
    })
}

/// 루트 프로세스의 OS 핸들(Windows) / fd(Linux/macOS) 수. 체크포인트에서만
/// 호출되므로 셸-아웃 비용은 무시 가능.
fn sample_handle_count(pid: u32) -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {pid}).HandleCount"),
            ])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir(format!("/proc/{pid}/fd"))
            .ok()
            .map(|d| d.count() as u64)
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("lsof")
            .args(["-p", &pid.to_string()])
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).lines().count() as u64)
    }
}

// ── 시나리오 ────────────────────────────────────────────────────────────

/// S1: tab churn — 탭 생성 → 셸 준비 대기 → 닫기. ConPTY/셸 프로세스 수명(L4)과
/// surface 단위 GPU/모델 정리 경로(L2·L3)를 두드린다.
fn cycle_tab_churn(inst: &TastyInstance, pane_id: u64) {
    let r = inst.call("tab.create", json!({ "pane_id": pane_id }));
    let surface_id = r["surface_id"]
        .as_u64()
        .expect("tab.create returned surface_id");
    // tab.create 응답에는 tab_id 가 없다 — tab.list 에서 surface_id 로 역조회.
    let tabs = inst.call("tab.list", json!({ "pane_id": pane_id }));
    let tab_id = tabs["tabs"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|t| t["surface_id"].as_u64() == Some(surface_id))
        })
        .and_then(|t| t["id"].as_u64())
        .expect("created tab not found in tab.list");
    // 셸이 실제로 프롬프트를 그릴 때까지 대기 — PTY 가 완전히 살아난 뒤 닫아야
    // "생성 직후 파괴" 편법 경로가 아니라 정상 수명 경로를 검증한다.
    let start = Instant::now();
    loop {
        if !inst.screen_text_of(surface_id).trim().is_empty() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "S1: surface {surface_id} never became ready"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    inst.call("tab.close", json!({ "tab_id": tab_id }));
    std::thread::sleep(Duration::from_millis(100));
}

/// S4: heavy output — 대량 출력으로 스크롤백 링버퍼·VTE 파서·glyph atlas 를
/// 두드린다. scrollback_lines 가 유한하므로 워밍업 후 RSS 는 평평해야 정상.
fn cycle_heavy_output(inst: &TastyInstance, surface_id: u64) {
    inst.set_mark(surface_id);
    inst.send_text(surface_id, "seq 1 20000\n");
    // 명령 echo("seq 1 20000")에는 "19999" 가 없으므로 완료 판정으로 안전.
    inst.wait_for_output(surface_id, "19999", Duration::from_secs(60));
}

/// S7: IPC churn — 상태 무변화 조회를 연타한다. `call` 은 매번 새 TCP 연결을
/// 열므로 per-connection 상태와 telemetry 버킷 축적(L2 용의 지점)을 검증한다.
fn cycle_ipc_churn(inst: &TastyInstance) {
    for _ in 0..25 {
        inst.call("surface.list", json!({}));
        inst.call("system.info", json!({}));
    }
}

/// S8: idle — 아무것도 안 함. 타이머/폴링 루프의 바닥 드리프트 측정.
fn cycle_idle() {
    std::thread::sleep(Duration::from_secs(30));
}

// ── 체크포인트 ──────────────────────────────────────────────────────────

fn checkpoint(inst: &TastyInstance, scenario: &str, cycle: u64) -> Value {
    // quiesce — 닫힌 surface 의 지연 정리(다음 프레임 retain, 자식 프로세스 종료)가
    // 카운트에 반영될 시간을 준다.
    std::thread::sleep(Duration::from_secs(2));
    let gpu = inst.call("system.gpu_stats", json!({}));
    let surfaces = inst
        .call("surface.list", json!({}))
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    let tree = sample_process_tree(inst.pid());
    json!({
        "ts": now_epoch(),
        "scenario": scenario,
        "cycle": cycle,
        "tree": tree,
        "handles": sample_handle_count(inst.pid()),
        "surfaces": surfaces,
        "gpu": gpu,
    })
}

// ── 메인 루프 ───────────────────────────────────────────────────────────

#[test]
#[ignore = "장시간 soak — SOAK_* env 로 명시 실행 (docs/dev-guide/memory-leak-soak.md)"]
fn soak() {
    let scenario = std::env::var("SOAK_SCENARIO").unwrap_or_else(|_| "s9".into());
    let duration = Duration::from_secs(env_u64("SOAK_DURATION_SECS", 600));
    let max_cycles = env_u64("SOAK_CYCLES", u64::MAX);
    let checkpoint_every = env_u64("SOAK_CHECKPOINT_EVERY", 10).max(1);
    let out_dir = std::env::var("SOAK_OUT_DIR")
        .unwrap_or_else(|_| ".claude-workspace/temp/soak".into());

    std::fs::create_dir_all(&out_dir).expect("failed to create SOAK_OUT_DIR");
    let out_path = format!("{}/soak-{}-{}.jsonl", out_dir, scenario, now_epoch() as u64);
    let mut out = std::fs::File::create(&out_path).expect("failed to create output file");

    let inst = TastyInstance::spawn();
    let pane_id = inst.first_pane_id();
    let surface0 = inst.first_surface_id();

    let meta = json!({
        "meta": {
            "ts": now_epoch(),
            "scenario": scenario,
            "duration_secs": duration.as_secs(),
            "checkpoint_every": checkpoint_every,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "pid": inst.pid(),
        }
    });
    writeln!(out, "{meta}").unwrap();
    println!("soak: scenario={scenario} duration={duration:?} out={out_path}");

    let start = Instant::now();
    let mut cycle: u64 = 0;
    while start.elapsed() < duration && cycle < max_cycles {
        match scenario.as_str() {
            "s1" => cycle_tab_churn(&inst, pane_id),
            "s4" => cycle_heavy_output(&inst, surface0),
            "s7" => cycle_ipc_churn(&inst),
            "s8" => cycle_idle(),
            // s9 mixed — 결정적 가중 혼합 (재현성 위해 난수 없이 cycle 인덱스로).
            "s9" => match cycle % 6 {
                0 | 1 | 2 => cycle_tab_churn(&inst, pane_id),
                3 => cycle_heavy_output(&inst, surface0),
                4 => cycle_ipc_churn(&inst),
                _ => std::thread::sleep(Duration::from_secs(5)),
            },
            other => panic!("unknown SOAK_SCENARIO '{other}' (s1|s4|s7|s8|s9)"),
        }
        cycle += 1;
        if cycle % checkpoint_every == 0 {
            let cp = checkpoint(&inst, &scenario, cycle);
            writeln!(out, "{cp}").unwrap();
            out.flush().unwrap();
            println!(
                "cycle {cycle}: rss_tree={}MB procs={} handles={:?}",
                cp["tree"]["rss_tree_bytes"].as_u64().unwrap_or(0) / 1_048_576,
                cp["tree"]["proc_count"],
                cp["handles"],
            );
        }
    }

    // 최종 체크포인트 — 기준 상태(탭 1개) 복귀 후 카운트. analyze.py 의
    // 기준선 복귀 판정(L3·L4 정수 엄격)이 이 마지막 레코드를 쓴다.
    let cp = checkpoint(&inst, &scenario, cycle);
    writeln!(out, "{cp}").unwrap();
    out.flush().unwrap();
    println!("soak done: {cycle} cycles, output at {out_path}");
}
