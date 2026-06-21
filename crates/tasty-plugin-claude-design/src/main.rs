//! Tasty Claude Design plugin — `claude.ai/design` 캔버스 통합 (외부 plugin).
//!
//! `tasty design login|status|projects|detect|probe|chat` CLI 세트를 제공한다.
//! **시스템에 이미 설치된** Playwright(off-screen 헤드풀)를 자식 프로세스로 띄워
//! `claude.ai/design` 에 attach 하고 Omelette RPC 로 Chat 을 주고받는다.
//!
//! claude plugin(`com.tasty.claude`, 로컬 CLI 자식 제어)과는 코드 공유가 없으며
//! 실패·권한·버전이 독립적으로 격리된다. 호스트 코드에 의존하지 않고
//! `tasty-plugin-sdk` 만 사용한다.
//!
//! 설계: `.claude-workspace/plans/claude-design-plugin.md`.
//! 현재 단계: M3 — 자식 node 런너 감독 + off-screen 헤드풀 기동(`design.probe`).
//! login/projects/chat 은 후속 마일스톤.

mod detect;
mod runner;

use std::path::PathBuf;

use detect::RuntimeDetection;
use runner::{PROBE_TIMEOUT, Runner};
use serde_json::{Value, json};
use tasty_plugin_sdk::{
    BusHandle, HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx,
    SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.claude-design";
const PLUGIN_VERSION: &str = "0.1.2"; // tasty-plugin.toml / Cargo.toml 과 일치

struct ClaudeDesignPlugin {
    /// 자식 node 런너. on_start 에서 런타임이 갖춰져 있으면 기동한다. 런타임 부재/
    /// 기동 실패 시 `None` 으로 두고, design.* 호출 시 명시적 에러로 안내한다.
    runner: Option<Runner>,
    /// 임베드 런너 스크립트를 기록할 위치. 호스트가 `TASTY_PLUGIN_DATA_DIR` 로 주입.
    data_dir: Option<PathBuf>,
}

impl ClaudeDesignPlugin {
    fn new() -> Self {
        Self {
            runner: None,
            data_dir: std::env::var_os("TASTY_PLUGIN_DATA_DIR").map(PathBuf::from),
        }
    }

    /// 런너가 살아있으면 그대로, 죽었거나 없으면 (재)기동한다. 런타임 부재 시 Err.
    fn ensure_runner(&mut self) -> Result<&Runner, IpcMethodError> {
        let alive = self.runner.as_ref().is_some_and(Runner::is_alive);
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
            match Runner::start(&det, &data_dir) {
                Ok(r) => self.runner = Some(r),
                Err(e) => return Err(IpcMethodError::new(format!("runner start failed: {e}"))),
            }
        }
        Ok(self.runner.as_ref().expect("runner just ensured"))
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
        // 자체 surface_kind 를 등록하지 않는다 — 이 plugin 은 CLI/IPC 만 노출하며
        // 브라우저 자동화는 자식 node 프로세스가 담당한다. 매니페스트에
        // surface_kinds 가 없으므로 이 콜백은 호출되지 않는다.
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    /// 부트스트랩 후 1회. 런타임이 갖춰져 있으면 런너를 미리 기동해 둔다(브라우저는
    /// lazy — 런너 node 프로세스만 상주). 런타임이 없으면 조용히 건너뛰고 status/
    /// probe 호출 시 안내한다.
    fn on_start(&mut self, _host: HostHandle, _bus: BusHandle) {
        let det = RuntimeDetection::run();
        if det.missing().is_some() {
            tracing::info!(status = %det.runtime_status(), "runtime not ready — runner deferred");
            return;
        }
        let Some(data_dir) = self.data_dir.clone() else {
            tracing::warn!("TASTY_PLUGIN_DATA_DIR not set — runner deferred");
            return;
        };
        match Runner::start(&det, &data_dir) {
            Ok(r) => self.runner = Some(r),
            Err(e) => tracing::warn!(error = %e, "runner start at on_start failed — will retry on demand"),
        }
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            // M2: 시스템 Playwright/node/chromium 탐지.
            "design.detect" => Ok(handle_detect()),
            // M3: 런타임 탐지 + 런너 상태 보고.
            "design.status" => Ok(self.handle_status()),
            // M3: off-screen 헤드풀 기동 → claude.ai/design 도달성/CF 통과 진단.
            "design.probe" => self.handle_probe(),
            // M4: 헤드풀 1회 로그인 → storageState → keyring.
            "design.login" => Err(not_yet("design.login", "M4")),
            // M5: ListProjects 위임.
            "design.projects" => Err(not_yet("design.projects", "M5")),
            // M5: Chat 전송 + 스트림 응답.
            "design.chat" => Err(not_yet("design.chat", "M5")),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

impl ClaudeDesignPlugin {
    /// `design.status` — 런타임 탐지 + 런너 상태. 부작용 없음(브라우저 강제 기동 안 함).
    fn handle_status(&mut self) -> Value {
        let det = RuntimeDetection::run();
        let detail = det.to_json();
        let mut out = json!({
            "runtime": det.runtime_status(),
            "node": detail["node"],
            "playwright": detail["playwright"],
            "chromium": detail["chromium"],
            "project": Value::Null, // M4/M5
        });

        // 런너가 살아있으면 cheap status op 으로 브라우저/로그인/CF 상태를 묻는다.
        // (브라우저는 강제로 띄우지 않는다 — probe/login/chat 에서만 기동.)
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

    /// `design.probe` — off-screen 헤드풀을 실제로 띄워 claude.ai/design 도달성과
    /// Cloudflare 통과를 진단한다(자격증명 불필요). M3 end-to-end 검증 + 운영 진단용.
    fn handle_probe(&mut self) -> Result<Value, IpcMethodError> {
        let runner = self.ensure_runner()?;
        runner
            .request("probe", json!({}), PROBE_TIMEOUT)
            .map_err(IpcMethodError::new)
    }
}

/// `design.detect` — 시스템 Playwright/node/chromium 을 탐지해 경로를 보고한다.
/// 설정 UI 의 "자동 감지" 버튼 백엔드이기도 하다 (설계 §4·§12).
fn handle_detect() -> Value {
    let det = RuntimeDetection::run();
    let mut out = det.to_json();
    out["runtime"] = Value::String(det.runtime_status());
    out
}

/// 후속 마일스톤에서 구현될 메서드의 임시 응답. 빈 핸들러가 조용히 성공한 것처럼
/// 보이지 않도록 명시적 에러를 돌려 호출자(CLI/에이전트)가 미구현임을 알 수 있게 한다.
fn not_yet(method: &str, milestone: &str) -> IpcMethodError {
    IpcMethodError::new(format!("{method} is not implemented yet (planned for {milestone})"))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ClaudeDesignPlugin::new())
}
