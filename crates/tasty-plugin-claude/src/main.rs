//! Tasty Claude Code plugin — 외부 plugin.
//!
//! `tasty claude launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. Phase 2가 끝나면 호스트 내부에 박혀 있던 Claude Code 통합이
//! 이 plugin으로 일원화되며, codex/aider 등 다른 코딩 에이전트 plugin들과 동등한 1급
//! 확장점 위에서 동작한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod error_scan;
mod hook;
mod install;
mod state;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tasty_plugin_sdk::{
    HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx,
    SurfaceLifecycleCtx, SurfaceResult,
};

use error_scan::ErrorScanner;
use state::ClaudeState;

const PLUGIN_ID: &str = "com.tasty.claude";
const PLUGIN_VERSION: &str = "0.1.0";

/// PTY 에러 폴링 간격. 호스트 메모리 스캔(O(1))과의 정확도 차이를 좁히기 위해
/// 짧게. 자식 N명에 대해 N IPC/주기지만 N이 10 이하인 일상 시나리오에서는 무시
/// 가능한 부하 (8 calls/sec @ 10 children).
const ERROR_SCAN_INTERVAL: Duration = Duration::from_millis(800);

struct ClaudePlugin {
    state: ClaudeState,
    scanner: Arc<Mutex<ErrorScanner>>,
}

impl ClaudePlugin {
    fn new() -> Self {
        Self {
            state: ClaudeState::load(),
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
        // 만들지 않는다. 매니페스트에 surface_kinds가 없으므로 이 콜백은 호출되지
        // 않을 것이다.
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        // step 03: hook/install/uninstall만 plugin이 처리. 나머지 claude.* 메서드
        // (launch/spawn/children/parent/kill/respawn/tell/wait/broadcast)는 step 04
        // cutover에서 합류한다. 그 전까지는 호스트 정적 핸들러가 살아 있고, plugin은
        // BUILTINS에 미등록이라 IPC가 forward되지 않으므로 이 분기들은 실 트래픽을
        // 받지 않는다. 단위 테스트 + cutover 시점 동작을 위해 미리 연결.
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
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_surface_lifecycle(&mut self, ctx: SurfaceLifecycleCtx) {
        // 호스트가 일반 terminal surface가 닫혔다고 알려준다. 그 surface가 claude
        // 자식이었다면 child registry에서 제거하고, parent였다면 closed_parents로
        // 마킹한다. error scan에서도 함께 제외한다.
        let sid = ctx.surface_id;
        let parent_was_child = self.state.parent_of_child(sid).is_some();
        if parent_was_child {
            self.state.unregister_child(sid);
            self.state.save();
            if let Ok(mut s) = self.scanner.lock() {
                s.disable(sid);
            }
            return;
        }
        if self.state.is_known_parent(sid) {
            self.state.mark_parent_closed(sid);
            self.state.save();
        }
    }

    fn on_start(&mut self, host: HostHandle) {
        // worker dispatch가 시작되기 직전에 1회 호출. PTY error scan을 위한
        // background polling thread를 띄운다. 호스트가 메모리 스캔하던 패턴을
        // 1:1로 옮겼고 (`error_scan.rs::CLAUDE_ERROR_PATTERN`), polling 간격은
        // 800ms로 호스트 tick에 근접하게 맞춘다.
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
        // 끼어들 수 있게 한다. snapshot 후 surface가 disable되면 다음 tick에 자연
        // 반영.
        let surfaces = match scanner.lock() {
            Ok(s) => s.enabled_snapshot(),
            Err(e) => {
                tracing::error!("claude scanner mutex poisoned: {e}");
                return;
            }
        };
        for sid in surfaces {
            // 각 IPC call은 최대 60초까지 block 가능하지만 정상 응답은 ms 단위.
            // 한 surface에서 timeout이 나도 나머지에 영향 없도록 그냥 진행.
            if let Ok(mut s) = scanner.lock() {
                let _ = s.scan_one(&host, sid);
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ClaudePlugin::new())
}
