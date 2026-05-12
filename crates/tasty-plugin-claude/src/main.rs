//! Tasty Claude Code plugin — 외부 plugin.
//!
//! `tasty claude launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. Phase 2가 끝나면 호스트 내부에 박혀 있던 Claude Code 통합이
//! 이 plugin으로 일원화되며, codex/aider 등 다른 코딩 에이전트 plugin들과 동등한 1급
//! 확장점 위에서 동작한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod state;

use serde_json::Value;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceLifecycleCtx,
    SurfaceResult,
};

use state::ClaudeState;

const PLUGIN_ID: &str = "com.tasty.claude";
const PLUGIN_VERSION: &str = "0.1.0";

struct ClaudePlugin {
    state: ClaudeState,
}

impl ClaudePlugin {
    fn new() -> Self {
        Self {
            state: ClaudeState::load(),
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

    fn handle_ipc_method(&mut self, _ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        // 핸들러 본문은 Phase 2 후속 단계(02~04)에서 호스트 IPC 핸들러를 plugin으로
        // 이주하면서 채운다. 지금은 호스트가 모든 claude.* 메서드를 직접 처리하므로
        // plugin으로는 forward되지 않는다.
        Err(IpcMethodError::not_implemented())
    }

    fn on_surface_lifecycle(&mut self, ctx: SurfaceLifecycleCtx) {
        // 호스트가 일반 terminal surface가 닫혔다고 알려준다. 그 surface가 claude
        // 자식이었다면 child registry에서 제거하고, parent였다면 closed_parents로
        // 마킹한다. plugin은 어떤 surface_id가 자기 자식/부모인지 자체 state로 알고
        // 있으므로 host에 별도 질의는 필요 없다.
        let sid = ctx.surface_id;
        let parent_was_child = self.state.parent_of_child(sid).is_some();
        if parent_was_child {
            self.state.unregister_child(sid);
            self.state.save();
            return;
        }
        if self.state.is_known_parent(sid) {
            self.state.mark_parent_closed(sid);
            self.state.save();
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
