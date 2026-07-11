#![forbid(unsafe_code)]

//! Tasty Claude Design plugin — `claude.ai/design` 캔버스 통합 (외부 plugin).
//!
//! `tasty design login|logout|status|projects|detect|probe|chat` CLI 세트를 제공한다.
//! **시스템에 이미 설치된** Playwright(off-screen 헤드풀)를 자식 프로세스로 띄워
//! `claude.ai/design` 에 attach 하고 Omelette RPC 로 Chat 을 주고받는다.
//!
//! claude plugin(`com.tasty.claude`, 로컬 CLI 자식 제어)과는 코드 공유가 없으며
//! 실패·권한·버전이 독립적으로 격리된다. 호스트 코드에 의존하지 않고
//! `tasty-plugin-sdk` 만 사용한다.
//!
//! 설계: `.claude-workspace/plans/claude-design-plugin.md`.
//! 현재 단계: M5 — 기계적 chat(composer+send+Chat스트림 종료대기) + projects 스크랩.
//! 셀렉터/턴종료 신호는 실측 관찰로 확정(chat-composer-input / chat-send-button /
//! OmeletteService/Chat 응답 종료). 자격증명 저장은 평문(ADR-0018).

mod auth;
mod detect;
mod runner;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use detect::RuntimeDetection;
use runner::{PROBE_TIMEOUT, Runner};
use serde_json::{Value, json};
use tasty_plugin_sdk::{IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceResult};

const PLUGIN_ID: &str = "com.tasty.claude-design";
const PLUGIN_VERSION: &str = "0.1.16"; // tasty-plugin.toml / Cargo.toml 과 일치

/// `design.protocol` 정본 텍스트 — 바이너리에 임베드. 동시성 lock 규약 전문 + AI 부트스트랩
/// 절차. 호스트 패키징(manifest+binary+lang 만 복사)에 의존하지 않고 CLI 가 직접 출력한다.
const PROTOCOL_MD: &str = include_str!("../protocol/protocol.md");
const BOOTSTRAP_MD: &str = include_str!("../protocol/bootstrap.md");

/// 로그인은 사용자가 브라우저에서 직접 인증해야 하므로 최대 대기를 길게 둔다.
/// runner JS 의 폴링 한계(5분)보다 약간 길게 잡아 마지막 응답을 받는다.
const LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(330);

struct ClaudeDesignPlugin {
    /// 자식 node 런너. Arc 로 보관해 비동기 로그인/chat thread 와 공유한다(런너
    /// 프로토콜은 id 기반이라 동시 in-flight 요청 안전).
    runner: Option<Arc<Runner>>,
    /// 임베드 런너 스크립트 기록 + auth.json 저장 위치. 호스트가 주입.
    data_dir: Option<PathBuf>,
    /// 최근 chat 결과. 디자인 턴은 수 분 걸려 worker 를 막으면 host health-check(60s)가
    /// 플러그인을 재시작하므로, chat 은 bg thread 에서 돌리고 결과를 여기 적재해 status/
    /// chat_status 로 노출한다. `{state: idle|running|done|error, reply?, error?}`.
    last_chat: Arc<Mutex<Value>>,
}

impl ClaudeDesignPlugin {
    fn new() -> Self {
        Self {
            runner: None,
            data_dir: std::env::var_os("TASTY_PLUGIN_DATA_DIR").map(PathBuf::from),
            last_chat: Arc::new(Mutex::new(json!({ "state": "idle" }))),
        }
    }

    /// 런너가 살아있으면 그 Arc 를, 죽었거나 없으면 (재)기동한다. 런타임 부재 시 Err.
    fn ensure_runner(&mut self) -> Result<Arc<Runner>, IpcMethodError> {
        let alive = self.runner.as_ref().is_some_and(|r| r.is_alive());
        if !alive {
            self.runner = None;
            let det = RuntimeDetection::run();
            if let Some(missing) = det.missing() {
                return Err(IpcMethodError::new(format!(
                    "runtime_missing: {missing} — install node + Playwright (the plugin does not bundle them)"
                )));
            }
            let data_dir = self.data_dir.clone().ok_or_else(|| {
                IpcMethodError::new("TASTY_PLUGIN_DATA_DIR not set — cannot materialize runner")
            })?;
            let runner = Runner::start(&det, &data_dir)
                .map_err(|e| IpcMethodError::new(format!("runner start failed: {e}")))?;
            let runner = Arc::new(runner);
            // 저장된 세션이 있으면 런너에 주입.
            inject_saved_auth(&runner, &data_dir);
            self.runner = Some(runner);
        }
        Ok(Arc::clone(
            self.runner.as_ref().expect("runner just ensured"),
        ))
    }
}

impl Plugin for ClaudeDesignPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // 자체 surface_kind 를 등록하지 않는다 — CLI/IPC 만 노출하며 브라우저 자동화는
        // 자식 node 프로세스가 담당한다. 이 콜백은 호출되지 않는다.
        SurfaceResult::default()
    }

    // on_start 없음 — 런너는 lazy 기동(첫 probe/login/chat 시 ensure_runner 가 spawn).
    // 디자인을 쓰지 않는 세션에서 매 부팅마다 node 를 띄우지 않기 위함. 저장된 세션
    // 주입도 ensure_runner 안에서 처리한다.

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            // M2: 시스템 Playwright/node/chromium 탐지.
            "design.detect" => Ok(handle_detect()),
            // M3: 런타임 탐지 + 런너/세션 상태.
            "design.status" => Ok(self.handle_status()),
            // M3: off-screen 헤드풀 → claude.ai/design 도달성/CF 진단.
            "design.probe" => self.handle_probe(),
            // M4: 화면 안 헤드풀로 사용자 로그인 → storageState 저장(비동기).
            "design.login" => self.handle_login(),
            // M4: 저장된 세션 삭제.
            "design.logout" => self.handle_logout(),
            // M5: 런너로 프로젝트 목록 스크랩.
            "design.projects" => self.handle_projects(),
            // M5: 기계적 chat — bg thread 시작(즉시 반환), 결과는 chat_status 로.
            "design.chat" => self.handle_chat(&ctx.params),
            // M5: 최근 chat 상태/응답 (auto_wait 폴링 대상).
            "design.chat_status" => Ok(self.handle_chat_status()),
            // 동시성 lock 프로토콜 정본 출력(+부트스트랩 절차). 브라우저/로그인 불필요 —
            // 순수 텍스트. AI 가 이 규약을 발견·설치하는 경로.
            "design.protocol" => Ok(handle_protocol(&ctx.params)),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

impl ClaudeDesignPlugin {
    /// `design.status` — 런타임 탐지 + 런너 + 저장 세션 상태. 부작용 없음.
    fn handle_status(&mut self) -> Value {
        let det = RuntimeDetection::run();
        let detail = det.to_json();
        let stored_auth = self.data_dir.as_ref().is_some_and(|d| auth::has_auth(d));
        let mut out = json!({
            "runtime": det.runtime_status(),
            "node": detail["node"],
            "playwright": detail["playwright"],
            "chromium": detail["chromium"],
            "stored_auth": stored_auth, // 디스크에 저장된 로그인 세션 존재 여부.
            "project": Value::Null,     // M5
            "last_chat": self.handle_chat_status(), // 최근 chat 상태/응답.
        });

        match self.runner.as_ref().filter(|r| r.is_alive()) {
            Some(r) => match r.request("status", json!({}), runner::DEFAULT_OP_TIMEOUT) {
                Ok(msg) => {
                    out["runner"] = json!("up");
                    out["browser"] = msg.get("browser").cloned().unwrap_or(Value::Null);
                    out["cf_ok"] = msg.get("cf_ok").cloned().unwrap_or(Value::Null);
                    out["logged_in"] = msg.get("logged_in").cloned().unwrap_or(Value::Null);
                }
                Err(e) => {
                    out["runner"] = json!("unresponsive");
                    out["runner_error"] = json!(e);
                }
            },
            None => {
                out["runner"] = json!("not_started");
            }
        }
        out
    }

    /// `design.probe` — off-screen 헤드풀 도달성/CF 진단(자격증명 불필요).
    fn handle_probe(&mut self) -> Result<Value, IpcMethodError> {
        let runner = self.ensure_runner()?;
        runner
            .request("probe", json!({}), PROBE_TIMEOUT)
            .map_err(IpcMethodError::new)
    }

    /// `design.login` — 화면 안 브라우저를 열어 사용자가 직접 로그인하게 하고, 완료되면
    /// storageState 를 디스크에 저장한다. 로그인 대기는 수 분이 걸릴 수 있어 **백그라운드
    /// thread** 에서 처리하고 즉시 반환한다(핸들러가 막히면 호스트 health ping 이 끊긴다).
    /// 사용자는 로그인 후 `tasty design status` 로 결과를 확인한다.
    fn handle_login(&mut self) -> Result<Value, IpcMethodError> {
        let runner = self.ensure_runner()?;
        let data_dir = self
            .data_dir
            .clone()
            .ok_or_else(|| IpcMethodError::new("TASTY_PLUGIN_DATA_DIR not set"))?;

        std::thread::Builder::new()
            .name("design-login".into())
            .spawn(move || run_login(runner, data_dir))
            .map_err(|e| IpcMethodError::new(format!("spawn login thread failed: {e}")))?;

        Ok(json!({
            "kind": "login_started",
            "message": "A browser window opened. Complete the login there, then run `tasty design status` to confirm.",
        }))
    }

    /// `design.projects` — 로그인된 런너로 `/design` 홈의 프로젝트 목록을 스크랩한다.
    fn handle_projects(&mut self) -> Result<Value, IpcMethodError> {
        self.require_logged_in()?;
        let runner = self.ensure_runner()?;
        runner
            .request("list_projects", json!({}), runner::PROBE_TIMEOUT)
            .map_err(IpcMethodError::new)
    }

    /// `design.chat` — 대상 프로젝트 캔버스에 메시지를 보낸다. 디자인 턴은 수 분 걸릴 수
    /// 있어 worker 를 막으면 host health-check(60s)가 플러그인을 재시작하므로, **bg
    /// thread** 에서 돌리고 즉시 `{state:"running"}` 을 반환한다. 결과는 `design.chat_status`
    /// (또는 status 의 last_chat)로 확인한다. CLI 는 매니페스트 auto_wait 로 자동 폴링.
    fn handle_chat(&mut self, params: &Value) -> Result<Value, IpcMethodError> {
        self.require_logged_in()?;
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| IpcMethodError::invalid_params("message is required"))?
            .to_string();
        let project = self.resolve_project(params.get("project").and_then(Value::as_str))?;
        // 디자인 턴(고충실 시안)은 흔히 수 분~십수 분 걸린다. 옛 기본값 180s 는 정상 턴을
        // 조기 절단해 reply 를 못 받았다(CLI help 의 "omit = 턴 종료까지"와도 어긋남).
        // 생략 시 넉넉히 30분까지 기다린다. 완료는 lock 프로토콜(design-tasks/.DONE)로도
        // 추적할 수 있으므로 이 값은 상한일 뿐이다.
        let timeout_s = params.get("timeout").and_then(Value::as_u64).unwrap_or(1800);

        let mut req = json!({ "message": message, "timeout_ms": timeout_s * 1000 });
        if let Some(uuid) = project {
            req["project"] = json!(uuid);
        }
        let runner = self.ensure_runner()?;
        let wait = std::time::Duration::from_secs(timeout_s + 30);

        let last_chat = Arc::clone(&self.last_chat);
        if let Ok(mut g) = last_chat.lock() {
            *g = json!({ "state": "running" });
        }
        std::thread::Builder::new()
            .name("design-chat".into())
            .spawn(move || run_chat(runner, req, wait, last_chat))
            .map_err(|e| IpcMethodError::new(format!("spawn chat thread failed: {e}")))?;

        Ok(json!({ "state": "running" }))
    }

    /// `design.chat_status` — 최근 chat 의 상태/응답. auto_wait 폴링 대상.
    fn handle_chat_status(&self) -> Value {
        self.last_chat
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| json!({ "state": "error", "error": "last_chat poisoned" }))
    }

    /// `--project` 값을 UUID 로 해석한다. UUID 형식이면 그대로, 별칭이면 data_dir/
    /// projects.json(`{ "alias": "uuid" }`)에서 찾는다. 사용자가 UUID/별칭을 미리
    /// 적어두면 런너가 `/design/p/<uuid>` 로 바로 점프한다.
    fn resolve_project(&self, project: Option<&str>) -> Result<Option<String>, IpcMethodError> {
        let Some(p) = project.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None); // 미지정 — 런너가 현재 열린 프로젝트를 사용.
        };
        if is_uuid(p) {
            return Ok(Some(p.to_string()));
        }
        // 별칭 → UUID.
        let data_dir = self
            .data_dir
            .as_ref()
            .ok_or_else(|| IpcMethodError::new("TASTY_PLUGIN_DATA_DIR not set"))?;
        let map_path = data_dir.join("projects.json");
        let raw = std::fs::read_to_string(&map_path).map_err(|_| {
            IpcMethodError::invalid_params(&format!(
                "'{p}' is not a UUID and no projects.json alias map found at {}",
                map_path.display()
            ))
        })?;
        let map: Value = serde_json::from_str(&raw)
            .map_err(|e| IpcMethodError::new(format!("projects.json parse error: {e}")))?;
        match map.get(p).and_then(Value::as_str) {
            Some(uuid) => Ok(Some(uuid.to_string())),
            None => Err(IpcMethodError::invalid_params(&format!(
                "alias '{p}' not found in projects.json"
            ))),
        }
    }

    /// 저장된 세션이 없으면 명확한 안내 에러.
    fn require_logged_in(&self) -> Result<(), IpcMethodError> {
        let logged_in = self.data_dir.as_ref().is_some_and(|d| auth::has_auth(d));
        if logged_in {
            Ok(())
        } else {
            Err(IpcMethodError::new(
                "not logged in — run `tasty design login` first",
            ))
        }
    }

    /// `design.logout` — 저장된 세션을 삭제하고 런너의 in-memory auth 도 비운다.
    fn handle_logout(&mut self) -> Result<Value, IpcMethodError> {
        let data_dir = self
            .data_dir
            .clone()
            .ok_or_else(|| IpcMethodError::new("TASTY_PLUGIN_DATA_DIR not set"))?;
        auth::clear_auth(&data_dir)
            .map_err(|e| IpcMethodError::new(format!("clear auth failed: {e}")))?;
        if let Some(runner) = self.runner.as_ref().filter(|r| r.is_alive())
            && let Err(e) = runner.request(
                "set_auth",
                json!({ "storage_state": null }),
                runner::DEFAULT_OP_TIMEOUT,
            )
        {
            tracing::warn!(error = %e, "runner set_auth(null) on logout failed");
        }
        Ok(json!({ "ok": true }))
    }
}

/// 백그라운드 chat: 런너에 chat op 를 보내 턴 종료까지 기다리고, 결과를 last_chat 에
/// 적재한다(worker 비차단). chat_status/status 가 노출.
fn run_chat(
    runner: Arc<Runner>,
    req: Value,
    wait: std::time::Duration,
    last_chat: Arc<Mutex<Value>>,
) {
    let result = match runner.request("chat", req, wait) {
        Ok(msg) => json!({
            "state": "done",
            "reply": msg.get("reply").cloned().unwrap_or(Value::Null),
            "url": msg.get("url").cloned().unwrap_or(Value::Null),
        }),
        Err(e) => json!({ "state": "error", "error": e }),
    };
    if let Ok(mut g) = last_chat.lock() {
        *g = result;
    }
}

/// 저장된 storageState 를 런너에 주입(set_auth). 없으면 no-op.
fn inject_saved_auth(runner: &Runner, data_dir: &Path) {
    match auth::load_auth(data_dir) {
        Ok(Some(state)) => {
            if let Err(e) = runner.request(
                "set_auth",
                json!({ "storage_state": state }),
                runner::DEFAULT_OP_TIMEOUT,
            ) {
                tracing::warn!(error = %e, "inject saved auth failed");
            } else {
                tracing::info!("saved design session injected into runner");
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "load saved auth failed"),
    }
}

/// 백그라운드 로그인: runner 에 login op 를 보내 사용자 인증을 기다리고, 성공하면
/// storageState 를 디스크에 저장한다(런너 JS 가 자기 in-memory authState 도 갱신).
fn run_login(runner: Arc<Runner>, data_dir: PathBuf) {
    match runner.request("login", json!({}), LOGIN_TIMEOUT) {
        Ok(msg) => match msg.get("kind").and_then(Value::as_str) {
            Some("login_ok") => {
                let Some(state) = msg.get("storage_state").and_then(Value::as_str) else {
                    tracing::error!("login_ok without storage_state");
                    return;
                };
                match auth::save_auth(state, &data_dir) {
                    Ok(()) => tracing::info!("design login captured and saved"),
                    Err(e) => tracing::error!(error = %e, "save auth failed after login"),
                }
            }
            Some("login_needed") => tracing::warn!("design login not completed within timeout"),
            other => tracing::warn!(?other, "unexpected login response kind"),
        },
        Err(e) => tracing::error!(error = %e, "design login op failed"),
    }
}

/// `design.protocol` — 동시성 lock 프로토콜 정본 텍스트를 반환한다. `--bootstrap` 이면
/// 대상 프로젝트에 규약을 심는 AI 절차를, 아니면 규약 전문을 낸다. 부작용 없음(브라우저·
/// 로그인 불필요) — AI 가 "충돌은 이렇게 규칙 세워 회피한다"를 발견하는 값싼 경로.
fn handle_protocol(params: &Value) -> Value {
    let bootstrap = params
        .get("bootstrap")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    json!({
        "mode": if bootstrap { "bootstrap" } else { "print" },
        "folder": "design-tasks/",
        "filename_pattern": "<YYYYMMDD-hhmm>-<slug>.<STATE>.md",
        "states": ["WORKING", "DONE", "FAILED", "NEEDS-INPUT"],
        "ttl_minutes": 10,
        "grace_minutes": 5,
        "text": if bootstrap { BOOTSTRAP_MD } else { PROTOCOL_MD },
    })
}

/// `design.detect` — 시스템 Playwright/node/chromium 탐지. 설정 UI "자동 감지" 백엔드(§12).
fn handle_detect() -> Value {
    let det = RuntimeDetection::run();
    let mut out = det.to_json();
    out["runtime"] = Value::String(det.runtime_status());
    out
}

/// `8-4-4-4-12` hex UUID 형식 검사 (별칭과 구분용).
fn is_uuid(s: &str) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip(groups)
            .all(|(part, n)| part.len() == n && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ClaudeDesignPlugin::new())
}

#[cfg(test)]
mod tests {
    use super::is_uuid;

    #[test]
    fn uuid_detection() {
        assert!(is_uuid("41fd3f5a-4bb9-4877-999f-db5124dc2925"));
        assert!(is_uuid("b57d6c0a-4bdc-4114-9a7d-d63d27284ef3"));
        assert!(!is_uuid("tasty")); // 별칭
        assert!(!is_uuid("41fd3f5a4bb94877999fdb5124dc2925")); // 하이픈 없음
        assert!(!is_uuid("zzzzzzzz-4bb9-4877-999f-db5124dc2925")); // non-hex
        assert!(!is_uuid(""));
    }
}
