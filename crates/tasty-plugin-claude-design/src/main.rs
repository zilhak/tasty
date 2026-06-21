//! Tasty Claude Design plugin — `claude.ai/design` 캔버스 통합 (외부 plugin).
//!
//! `tasty design login|status|projects|detect|chat` CLI 세트를 제공한다.
//! **시스템에 이미 설치된** Playwright(off-screen 헤드풀)를 자식 프로세스로 띄워
//! `claude.ai/design` 에 attach 하고 Omelette RPC 로 Chat 을 주고받는다.
//!
//! claude plugin(`com.tasty.claude`, 로컬 CLI 자식 제어)과는 코드 공유가 없으며
//! 실패·권한·버전이 독립적으로 격리된다. 호스트 코드에 의존하지 않고
//! `tasty-plugin-sdk` 만 사용한다.
//!
//! 설계: `.claude-workspace/plans/claude-design-plugin.md`.
//! 현재 단계: M1 — 크레이트 골격 + 매니페스트 + i18n. 각 핸들러는 후속 마일스톤에서 구현.

use serde_json::Value;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.claude-design";
const PLUGIN_VERSION: &str = "0.1.0";

struct ClaudeDesignPlugin;

impl ClaudeDesignPlugin {
    fn new() -> Self {
        Self
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

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        // M1 골격: 라우팅만 세워두고 각 핸들러는 후속 마일스톤에서 채운다.
        // 매니페스트에 선언된 design.* 메서드를 명시적으로 분기해 두면 cutover 가
        // 단계별로 안전하다 (claude plugin 의 step 분할 패턴과 동일).
        match ctx.method.as_str() {
            // M2: 시스템 Playwright/node/chromium 탐지.
            "design.detect" => Err(not_yet("design.detect", "M2")),
            // M2: 런타임·runner·로그인·CF·attach 상태 보고.
            "design.status" => Err(not_yet("design.status", "M2")),
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
