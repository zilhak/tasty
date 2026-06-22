//! 자식 node 런너 감독자 (M3).
//!
//! 시스템 설치 Playwright 를 구동하는 `design-runner.js`(임베드)를 data_dir 에 기록한 뒤
//! 자식 node 프로세스로 띄우고, NDJSON 으로 요청/응답을 주고받는다. 핸들러는 동기지만
//! 런너 통신은 reader thread + per-request 채널로 비차단 라우팅한다.
//!
//! 프로토콜은 `design-runner.js` 상단 주석 및 설계 §5 참조. M3 범위는 ping/status/probe/
//! shutdown. Chat 스트리밍(다중 응답)은 M5 에서 `request_stream` 으로 확장한다.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::detect::RuntimeDetection;

/// 자식 node 프로세스로 실행할 런너 스크립트. 바이너리에 임베드해 호스트 패키징
/// (manifest+binary+lang 만 복사)에 의존하지 않고 data_dir 에 직접 기록한다.
const RUNNER_JS: &str = include_str!("../runner/design-runner.js");

/// 기본 런너 op 타임아웃. probe 처럼 브라우저를 띄우는 op 는 호출 측에서 더 길게 준다.
pub const DEFAULT_OP_TIMEOUT: Duration = Duration::from_secs(8);

/// off-screen 헤드풀 기동 + claude.ai/design 이동 + CF 자동통과까지 여유.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(45);

pub struct Runner {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    next_id: AtomicU64,
    /// id → 응답 채널. reader thread 가 도착 메시지를 해당 id 채널로 라우팅한다.
    pending: Arc<Mutex<HashMap<u64, Sender<Value>>>>,
}

impl Runner {
    /// 런타임 탐지 결과로 런너를 띄운다. node/playwright 가 없으면 에러.
    pub fn start(det: &RuntimeDetection, data_dir: &Path) -> Result<Runner, String> {
        let node = det
            .node
            .as_ref()
            .ok_or_else(|| "node not found".to_string())?;
        let playwright = det
            .playwright
            .as_ref()
            .ok_or_else(|| "playwright module not found".to_string())?;
        // NODE_PATH 는 playwright 모듈의 부모 (node_modules) — require('playwright') 해석용.
        let node_path = playwright
            .parent()
            .ok_or_else(|| "playwright path has no parent".to_string())?;
        let runner_js = materialize_runner(data_dir)?;

        let mut cmd = Command::new(node);
        cmd.arg(&runner_js)
            .env("NODE_PATH", node_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // 런너 진단 로그(stderr)는 plugin 로그로 흘려보낸다.
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: GUI 호스트가 콘솔 앱(node)을 spawn 할 때 빈 콘솔 창이
            // 뜨지 않게 한다(claude/codex 는 터미널 surface 로 띄워 무관하나, 본 런너는
            // raw spawn 이라 명시 필요).
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn node runner failed: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "runner has no stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "runner has no stdout".to_string())?;
        let pending: Arc<Mutex<HashMap<u64, Sender<Value>>>> = Arc::new(Mutex::new(HashMap::new()));

        // reader thread: stdout NDJSON 을 파싱해 id 별 채널로 라우팅.
        {
            let pending = Arc::clone(&pending);
            std::thread::Builder::new()
                .name("design-runner-reader".into())
                .spawn(move || reader_loop(stdout, pending))
                .map_err(|e| format!("spawn reader thread failed: {e}"))?;
        }

        tracing::info!("design runner started");
        Ok(Runner {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            next_id: AtomicU64::new(1),
            pending,
        })
    }

    /// 단일 응답 op 를 보내고 첫 응답을 기다린다. `kind == "error"` 면 Err.
    pub fn request(&self, op: &str, extra: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel::<Value>();
        self.pending
            .lock()
            .map_err(|_| "pending lock poisoned".to_string())?
            .insert(id, tx);

        let mut req = json!({ "id": id, "op": op });
        if let Value::Object(map) = extra {
            for (k, v) in map {
                req[k] = v;
            }
        }
        let line = serde_json::to_string(&req).map_err(|e| e.to_string())? + "\n";

        let write_result = (|| -> Result<(), String> {
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| "stdin lock poisoned".to_string())?;
            stdin
                .write_all(line.as_bytes())
                .map_err(|e| format!("write to runner failed: {e}"))?;
            stdin
                .flush()
                .map_err(|e| format!("flush to runner failed: {e}"))?;
            Ok(())
        })();

        if let Err(e) = write_result {
            // 실패한 요청은 pending 에서 제거하고 에러.
            self.remove_pending(id);
            return Err(e);
        }

        let result = rx.recv_timeout(timeout);
        self.remove_pending(id);
        match result {
            Ok(msg) => {
                if msg.get("kind").and_then(Value::as_str) == Some("error") {
                    let detail = msg
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("runner error");
                    Err(detail.to_string())
                } else {
                    Ok(msg)
                }
            }
            Err(_) => Err(format!("runner op '{op}' timed out")),
        }
    }

    /// 런너 프로세스가 아직 실행 중인지.
    pub fn is_alive(&self) -> bool {
        match self.child.lock() {
            Ok(mut child) => matches!(child.try_wait(), Ok(None)),
            Err(_) => false,
        }
    }

    /// 정상 종료 요청(브라우저 close) 후 best-effort kill.
    pub fn shutdown(&self) {
        if let Err(e) = self.request("shutdown", json!({}), Duration::from_secs(5)) {
            tracing::debug!(error = %e, "runner shutdown request failed (killing)");
        }
        if let Ok(mut child) = self.child.lock()
            && let Err(e) = child.kill()
        {
            tracing::warn!(error = %e, "runner kill failed");
        }
    }

    fn remove_pending(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        // 브라우저까지 정상 닫고 종료 (best-effort).
        self.shutdown();
    }
}

fn reader_loop(
    stdout: std::process::ChildStdout,
    pending: Arc<Mutex<HashMap<u64, Sender<Value>>>>,
) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(error = %e, "runner stdout read error");
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, line = %trimmed, "runner emitted non-JSON line");
                continue;
            }
        };
        let Some(id) = msg.get("id").and_then(Value::as_u64) else {
            tracing::debug!(?msg, "runner message without id");
            continue;
        };
        let sender = pending.lock().ok().and_then(|p| p.get(&id).cloned());
        if let Some(tx) = sender {
            // 수신자가 이미 사라졌으면(호출 측 타임아웃) 조용히 버린다.
            if tx.send(msg).is_err() {
                tracing::debug!(id, "runner reply arrived after caller dropped");
            }
        }
    }
    tracing::info!("design runner reader thread ended (stdout closed)");
}

/// 임베드된 런너 스크립트를 data_dir/runner/design-runner.js 로 기록하고 경로를 반환.
fn materialize_runner(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("runner");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create runner dir failed: {e}"))?;
    let path = dir.join("design-runner.js");
    std::fs::write(&path, RUNNER_JS).map_err(|e| format!("write runner script failed: {e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialize_writes_runner_script() {
        let tmp = std::env::temp_dir().join(format!("tasty-design-test-{}", std::process::id()));
        let path = materialize_runner(&tmp).expect("materialize");
        let content = std::fs::read_to_string(&path).expect("read back");
        assert!(content.contains("design-runner"));
        assert!(content.contains("claude.ai/design"));
        // 정리 (실패해도 테스트 결과엔 영향 없음 — temp dir).
        if let Err(e) = std::fs::remove_dir_all(&tmp) {
            eprintln!("test cleanup failed: {e}");
        }
    }

    /// 실제 node + playwright + chromium 을 띄우는 end-to-end 검증. 환경 의존이라
    /// 기본 비활성. 로컬에서 `cargo test -p tasty-plugin-claude-design -- --ignored`.
    #[test]
    #[ignore = "spawns real node/playwright/chromium; run with --ignored locally"]
    fn end_to_end_ping_and_probe() {
        let det = RuntimeDetection::run();
        if let Some(missing) = det.missing() {
            eprintln!("runtime missing: {missing} — skipping e2e");
            return;
        }
        let tmp = std::env::temp_dir().join(format!("tasty-design-e2e-{}", std::process::id()));
        let runner = Runner::start(&det, &tmp).expect("start runner");

        let pong = runner
            .request("ping", json!({}), Duration::from_secs(10))
            .expect("ping");
        assert_eq!(pong.get("kind").and_then(Value::as_str), Some("pong"));

        let status = runner
            .request("probe", json!({}), PROBE_TIMEOUT)
            .expect("probe");
        assert_eq!(status.get("kind").and_then(Value::as_str), Some("status"));
        // off-screen 헤드풀은 Cloudflare 를 통과해야 한다 (조사 §6 Test 5).
        assert_eq!(status.get("cf_ok").and_then(Value::as_bool), Some(true));

        runner.shutdown();
        if let Err(e) = std::fs::remove_dir_all(&tmp) {
            eprintln!("e2e cleanup failed: {e}");
        }
    }
}
