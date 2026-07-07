#![forbid(unsafe_code)]

//! Tasty Claude Code plugin — 외부 plugin.
//!
//! `tasty claude launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다.
//!
//! 자식 terminal 관리(registry·spawn·wait·kill·reconcile·soft 점유)는 호스트가
//! 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 위임한다 — 이 plugin 은
//! 자체 child registry 를 보유하지 않는다(호스트 registry 가 단일 SoT). claude
//! 특화(session token 기동, hook fan-out, PTY error scan, install, 텔레메트리)만
//! 여기 남는다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod error_scan;
mod handlers;
mod hook;
mod install;
mod state;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use error_scan::ErrorScanner;
use handlers::*;
use serde_json::{Value, json};
use state::ClaudeState;
use tasty_plugin_sdk::{
    HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.claude";
const PLUGIN_VERSION: &str = "0.1.0";

/// PTY 에러 폴링 간격. 호스트 메모리 스캔(O(1))과의 정확도 차이를 좁히기 위해
/// 짧게. 자식 N명에 대해 N IPC/주기지만 N이 10 이하인 일상 시나리오에서는 무시
/// 가능한 부하 (8 calls/sec @ 10 children).
const ERROR_SCAN_INTERVAL: Duration = Duration::from_millis(800);

struct ClaudePlugin {
    /// claude 특화 상태 — wall-time 텔레메트리 타이밍만(hook 이 소비). child
    /// registry 는 호스트 `terminal.*` 가 소유한다.
    state: ClaudeState,
    scanner: Arc<Mutex<ErrorScanner>>,
}

impl ClaudePlugin {
    fn new() -> Self {
        Self {
            state: ClaudeState::new(),
            scanner: Arc::new(Mutex::new(ErrorScanner::new())),
        }
    }
}

impl Plugin for ClaudePlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // claude plugin은 자체 surface_kind를 등록하지 않는다 — 자식 Claude 프로세스는
        // 호스트의 일반 terminal surface에서 실행되며, surface 자체는 plugin이 직접
        // 만들지 않는다.
        SurfaceResult::default()
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "claude.hook" => hook::handle_claude_hook(&mut self.state, &ctx.host, &ctx.params),
            "claude.install" => match install::run_install() {
                Ok(added) => Ok(json!({ "installed": added })),
                Err(e) => Err(IpcMethodError::new(format!("install failed: {e}"))),
            },
            "claude.uninstall" => match install::run_uninstall() {
                Ok(removed) => Ok(json!({ "uninstalled": removed })),
                Err(e) => Err(IpcMethodError::new(format!("uninstall failed: {e}"))),
            },
            // 자식 관리 명령은 모두 호스트 `terminal.*` 로 위임(handlers.rs).
            "claude.parent" => handle_parent(&ctx.host, &ctx.params),
            "claude.children" => handle_children(&ctx.host, &ctx.params),
            "claude.wait" => handle_wait(&ctx.host, &ctx.params),
            "claude.wait_by_surface" => handle_wait_by_surface(&ctx.host, &ctx.params),
            "claude.wait_any" => handle_wait_any(&ctx.host, &ctx.params),
            "claude.kill" => handle_kill(&ctx.host, &ctx.params),
            "claude.broadcast" => handle_broadcast(&ctx.host, &ctx.params),
            "claude.tell" => handle_tell(&ctx.host, &ctx.params),
            // launch/respawn/spawn — claude 특화 기동 명령을 host registry 위에 얹는다.
            "claude.launch" => handle_launch(&self.scanner, &ctx.host, &ctx.params),
            "claude.respawn" => handle_respawn(&ctx.host, &ctx.params),
            "claude.spawn" => handle_spawn(&ctx.host, &ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_start(&mut self, host: HostHandle, _bus: tasty_plugin_sdk::BusHandle) {
        // PTY error scan 을 위한 background polling thread 만 띄운다. child registry
        // lifecycle(surface.closed 구독 + reconcile)은 호스트가 소유하므로 여기서
        // 하지 않는다 — error_scan 은 launch surface 에 대해 enable 되며, 죽은
        // surface 는 `scan_one` 의 `surface.read_since_mark` 실패 시 자연히 no-op 이 된다.
        let scanner = self.scanner.clone();
        std::thread::Builder::new()
            .name("claude-error-scan".into())
            .spawn(move || error_scan_loop(scanner, host))
            .expect("spawn claude-error-scan thread");
    }
}

fn error_scan_loop(scanner: Arc<Mutex<ErrorScanner>>, host: HostHandle) {
    loop {
        std::thread::sleep(ERROR_SCAN_INTERVAL);
        // lock을 짧게 잡고 snapshot만 떠서 IPC 호출 동안 다른 메서드(enable/disable)가
        // 끼어들 수 있게 한다.
        let surfaces = match scanner.lock() {
            Ok(s) => s.enabled_snapshot(),
            Err(e) => {
                tracing::error!("claude scanner mutex poisoned: {e}");
                return;
            }
        };
        for sid in surfaces {
            if let Ok(mut s) = scanner.lock() {
                // 반환값(매치된 snippet)은 단위 테스트용. polling 루프에서는 무시.
                s.scan_one(&host, sid);
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ClaudePlugin::new())
}
