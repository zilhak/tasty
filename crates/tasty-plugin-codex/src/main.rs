//! Tasty Codex plugin — 외부 plugin.
//!
//! `tasty codex spawn|children|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. 자식 terminal surface에서 `codex` CLI를 띄우고 Claude Code의
//! `tasty claude` 명령과 동일한 멀티에이전트 워크플로를 제공한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod handlers;
mod state;

use serde_json::Value;
use state::CodexState;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult,
    ui::{label, label_color, vbox},
};

const PLUGIN_ID: &str = "com.tasty.codex";
const PLUGIN_VERSION: &str = "0.1.0";

struct CodexPlugin {
    state: CodexState,
}

impl CodexPlugin {
    fn new() -> Self {
        Self {
            state: CodexState::load(),
        }
    }
}

impl Plugin for CodexPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // codex_session surface는 현재 PoC에서 직접 사용되지 않는다.
        // 자식 codex 프로세스는 host의 일반 terminal surface에서 실행된다.
        // surface가 만들어졌을 때 대비한 안내 stub.
        SurfaceResult {
            tree: Some(vbox([
                label_color("Codex Session", "subtext1"),
                label("Use `tasty codex spawn` from a terminal to create a child."),
            ])),
            display_name: Some("Codex".into()),
        }
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        let IpcMethodCtx {
            method,
            params,
            host,
            ..
        } = ctx;
        match method.as_str() {
            "codex.launch" => handlers::handle_launch(&mut self.state, &host, params),
            "codex.spawn" => handlers::handle_spawn(&mut self.state, &host, params),
            "codex.children" => handlers::handle_children(&self.state, params),
            "codex.parent" => handlers::handle_parent(&self.state, params),
            "codex.tell" => handlers::handle_tell(&host, params),
            "codex.wait" => handlers::handle_wait(&self.state, params),
            "codex.broadcast" => handlers::handle_broadcast(&self.state, &host, params),
            "codex.kill" => handlers::handle_kill(&mut self.state, &host, params),
            "codex.respawn" => handlers::handle_respawn(&mut self.state, &host, params),
            "codex.install" => handlers::handle_install(&mut self.state),
            "codex.uninstall" => handlers::handle_uninstall(&mut self.state),
            "codex.hook" => handlers::handle_hook(&mut self.state, params),
            other => Err(IpcMethodError::not_found(other)),
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
    tasty_plugin_sdk::run(CodexPlugin::new())
}
