#![forbid(unsafe_code)]

//! Tasty Codex plugin — 외부 plugin.
//!
//! `tasty codex spawn|children|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. 자식 terminal surface에서 `codex` CLI를 띄우고 Claude Code의
//! `tasty claude` 명령과 동일한 멀티에이전트 워크플로를 제공한다.
//!
//! 자식 terminal 관리(registry·spawn·wait·kill·reconcile·soft 점유)는 호스트가
//! 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 위임한다 — 이 plugin 은
//! 자체 child registry 를 보유하지 않는다(호스트 registry 가 단일 SoT). codex
//! 특화(command 빌더, hook/trust, install)만 여기 남는다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod handlers;
mod reboot;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tasty_plugin_sdk::{IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceResult};

const PLUGIN_ID: &str = "com.tasty.codex";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
struct CodexPlugin {
    /// reboot 시퀀스 진행 중인 surface 집합 — 같은 surface 중복 reboot 가드.
    rebooting: Arc<Mutex<HashSet<u32>>>,
}

impl Plugin for CodexPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // codex plugin 은 자체 surface_kind 를 등록하지 않는다. 자식 codex 프로세스는
        // 호스트의 일반 terminal surface 에서 실행되며, surface 자체는 plugin 이 만들지
        // 않는다. 매니페스트에 surface_kinds 가 없으므로 이 콜백은 호출되지 않는다.
        SurfaceResult::default()
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        let IpcMethodCtx {
            method,
            params,
            host,
            ..
        } = ctx;
        match method.as_str() {
            "codex.launch" => handlers::handle_launch(&host, params),
            "codex.spawn" => handlers::handle_spawn(&host, params),
            "codex.children" => handlers::handle_children(&host, params),
            "codex.parent" => handlers::handle_parent(&host, params),
            "codex.tell" => handlers::handle_tell(&host, params),
            "codex.wait" => handlers::handle_wait(&host, params),
            "codex.wait_by_surface" => handlers::handle_wait_by_surface(&host, params),
            "codex.broadcast" => handlers::handle_broadcast(&host, params),
            "codex.kill" => handlers::handle_kill(&host, params),
            "codex.respawn" => handlers::handle_respawn(&host, params),
            "codex.install" => handlers::handle_install(),
            "codex.uninstall" => handlers::handle_uninstall(),
            "codex.hook" => handlers::handle_hook(&host, params),
            "codex.reboot" => reboot::handle_reboot(&self.rebooting, &host, &params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(CodexPlugin::default())
}
