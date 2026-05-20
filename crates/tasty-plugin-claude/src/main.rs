//! Tasty Claude Code plugin — 외부 plugin.
//!
//! `tasty claude launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. Phase 2가 끝나면 호스트 내부에 박혀 있던 Claude Code 통합이
//! 이 plugin으로 일원화되며, codex/aider 등 다른 코딩 에이전트 plugin들과 동등한 1급
//! 확장점 위에서 동작한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod error_scan;
mod handlers;
mod hook;
mod install;
mod state;

use error_scan::ErrorScanner;
use handlers::*;
use serde_json::{json, Value};
use state::{ChildEntry, ClaudeState};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tasty_plugin_sdk::{
    EventDispatchCtx, HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx,
    SurfaceEventCtx, SurfaceResult,
};

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
        // 분기를 작게 cutover-안전 단계로 채워나간다. BUILTINS 미등록 동안엔 모든
        // claude.* 트래픽이 호스트로 가므로 본 분기는 실제로는 단위 테스트로만 검증.
        // 호스트 핸들러 제거 + BUILTINS 등록은 step 04e cutover에서 atomic으로.
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
            // step 04a: plugin 자기 ClaudeState만 보면 응답 가능한 핸들러들.
            "claude.set_idle_state" => handle_set_idle_state(&mut self.state, &ctx.params),
            "claude.set_needs_input" => handle_set_needs_input(&mut self.state, &ctx.params),
            "claude.parent" => handle_parent(&self.state, &ctx.params),
            // step 04b: 호스트 IPC(surface.foreground_process / surface.locate /
            // pane.close)와 ClaudeState를 함께 조합하는 핸들러들.
            "claude.children" => handle_children(&self.state, &ctx.host, &ctx.params),
            "claude.wait" => handle_wait(&self.state, &ctx.host, &ctx.params),
            "claude.kill" => handle_kill(&mut self.state, &ctx.host, &ctx.params),
            // step 04c: PTY 송신 핸들러. surface.send IPC를 통해 자식 terminal에
            // text를 보낸다.
            "claude.broadcast" => handle_broadcast(&self.state, &ctx.host, &ctx.params),
            "claude.tell" => handle_tell(&ctx.host, &ctx.params),
            // step 04d.1: 새 workspace에 claude 띄우기.
            "claude.launch" => handle_launch(&self.scanner, &ctx.host, &ctx.params),
            // step 04d.2: 자식 surface의 PTY를 갈아끼우고 claude 재시작.
            "claude.respawn" => handle_respawn(&mut self.state, &ctx.host, &ctx.params),
            // step 04d.3: parent surface가 사는 workspace 내 spawn pane을 자동
            // 관리(필요 시 생성)하고, 2x2 grid에 따라 새 자식 surface를 배치 +
            // claude 실행.
            "claude.spawn" => handle_spawn(&mut self.state, &ctx.host, &ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_event(&mut self, ctx: EventDispatchCtx) {
        // Event Bus 1.0: `surface.closed` 구독 시 호출. 닫힌 surface가 claude
        // 자식이었다면 child registry에서 제거하고, parent였다면 closed_parents로
        // 마킹한다. error scan에서도 함께 제외한다.
        if ctx.envelope.key != "surface.closed" {
            return;
        }
        let sid = match ctx.envelope.payload.get("surface_id").and_then(|v| v.as_u64()) {
            Some(v) => v as u32,
            None => return,
        };
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

    fn on_start(&mut self, host: HostHandle, bus: tasty_plugin_sdk::BusHandle) {
        // worker dispatch가 시작되기 직전에 1회 호출.
        // - `surface.closed` 이벤트 구독 (Event Bus 1.0). 옛 surface_observer
        //   매니페스트 필드의 대체 경로.
        // - PTY error scan을 위한 background polling thread spawn. 호스트가
        //   메모리 스캔하던 패턴을 1:1로 옮겼고 (`error_scan.rs::CLAUDE_ERROR_PATTERN`),
        //   polling 간격은 800ms로 호스트 tick에 근접하게 맞춘다.
        if let Err(e) = bus.subscribe("surface.closed") {
            tracing::warn!("subscribe surface.closed failed: {e}");
        }
        let scanner = self.scanner.clone();
        std::thread::Builder::new()
            .name("claude-error-scan".into())
            .spawn(move || error_scan_loop(scanner, host))
            .expect("spawn claude-error-scan thread");
    }
}

// ─── step 04a 핸들러들 ───────────────────────────────────────────────────────
//
// 호스트 src/ipc/handler/claude.rs의 응답 JSON과 byte-for-byte 동일해야 cutover
// 후 CLI 출력 회귀가 없다. param 키 이름 / 응답 필드 / 누락된 surface_id의 에러
// 분기까지 1:1 보존한다.


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
                // 반환값(매치된 snippet)은 단위 테스트용. polling 루프에서는 무시.
                s.scan_one(&host, sid);
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

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
