#![forbid(unsafe_code)]

//! Codex 기동·훅·설정 설치를 제공한다. 자식 관리는 호스트의 terminal.* IPC에 맡긴다.
//! spawn/tell은 호스트 응답을 기다리며 이후 상태 변화는 별도 알림 훅으로 받는다.

// 이유: 시험의 let _는 제품 코드에서 반환값을 버리는 목록에 포함하지 않는다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

// New-session commands already use POSIX environment/prompt syntax, including
// Git Bash on Windows. Keep execution in that shell instead of nesting cmd.exe.
const POSIX_CODEX_COMMAND: &str = "command codex";

mod handlers;
mod install;
mod reboot;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, SurfaceCreateCtx, SurfaceResult,
    i18n::Translator,
};

const PLUGIN_ID: &str = "com.tasty.codex";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
struct CodexPlugin {
    /// reboot 시퀀스 진행 중인 surface 집합 — 같은 surface 중복 reboot 가드.
    rebooting: Arc<Mutex<HashSet<u32>>>,
    /// 이 플러그인의 lang 디렉터리에서 읽은 번역 문구.
    translator: Translator,
}

impl Plugin for CodexPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // 자식은 호스트가 만든 일반 터미널에서 실행한다.
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
            "codex.launch" => handlers::handle_launch(&host, &params, &self.translator),
            "codex.spawn" => handlers::handle_spawn(&host, &params, &self.translator),
            "codex.children" => handlers::handle_children(&host, &params, &self.translator),
            "codex.parent" => handlers::handle_parent(&host, &params, &self.translator),
            "codex.state" => handlers::handle_state(&host, &params, &self.translator),
            "codex.tell" => handlers::handle_tell(&host, &params, &self.translator),
            "codex.notify_caller" => {
                handlers::handle_notify_caller(&host, &params, &self.translator)
            }
            "codex.broadcast" => handlers::handle_broadcast(&host, &params, &self.translator),
            "codex.kill" => handlers::handle_kill(&host, &params, &self.translator),
            "codex.respawn" => handlers::handle_respawn(&host, &params, &self.translator),
            "codex.install" => install::handle_install(&params, &self.translator),
            "codex.uninstall" => install::handle_uninstall(&params, &self.translator),
            "codex.hook" => handlers::handle_hook(&host, &params, &self.translator),
            "codex.reboot" => {
                reboot::handle_reboot(&self.rebooting, &host, &params, &self.translator)
            }
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
    // 환경을 읽지 못하면 번역 키를 그대로 반환한다.
    let translator = PluginEnv::load()
        .ok()
        .as_ref()
        .map(Translator::from_plugin_env)
        .unwrap_or_default();
    tasty_plugin_sdk::run(CodexPlugin {
        translator,
        ..Default::default()
    })
}

#[cfg(test)]
mod install_tests;
