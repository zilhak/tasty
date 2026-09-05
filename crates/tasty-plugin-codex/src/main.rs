#![forbid(unsafe_code)]

//! Tasty Codex plugin — 외부 plugin.
//!
//! `tasty codex spawn|children|tell|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. 자식 terminal surface에서 `codex` CLI를 띄우고 Claude Code의
//! `tasty claude` 명령과 동일한 멀티에이전트 워크플로를 제공한다. spawn/tell 은
//! 동기 대기 대신 완료 시 1 회성 알림 훅(`notify-caller`)으로 caller 에게 알린다.
//!
//! 자식 terminal 관리(registry·spawn·wait·kill·reconcile·soft 점유)는 호스트가
//! 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 위임한다 — 이 plugin 은
//! 자체 child registry 를 보유하지 않는다(호스트 registry 가 단일 SoT). codex
//! 특화(command 빌더, hook/trust, install)만 여기 남는다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod handlers;
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
    /// 사람이 읽는 응답 문자열 번역용(현재 소비자: spawn child 임계치 경고).
    /// plugin process 는 호스트 i18n 카탈로그에 접근할 수 없으므로 자기 `lang/` 를
    /// `main()` 에서 1 회 로드해 재사용한다.
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
            "codex.launch" => handlers::handle_launch(&host, params, &self.translator),
            "codex.spawn" => handlers::handle_spawn(&host, &self.translator, params),
            "codex.children" => handlers::handle_children(&host, params, &self.translator),
            "codex.parent" => handlers::handle_parent(&host, params, &self.translator),
            "codex.state" => handlers::handle_state(&host, params, &self.translator),
            "codex.tell" => handlers::handle_tell(&host, params, &self.translator),
            "codex.notify_caller" => {
                handlers::handle_notify_caller(&host, &self.translator, params)
            }
            "codex.broadcast" => handlers::handle_broadcast(&host, params, &self.translator),
            "codex.kill" => handlers::handle_kill(&host, params, &self.translator),
            "codex.respawn" => handlers::handle_respawn(&host, params, &self.translator),
            "codex.install" => handlers::handle_install(&self.translator),
            "codex.uninstall" => handlers::handle_uninstall(&self.translator),
            "codex.hook" => handlers::handle_hook(&host, params, &self.translator),
            "codex.reboot" => {
                reboot::handle_reboot(&self.rebooting, &host, &self.translator, &params)
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
    // `env` 부재(비정상 기동)에도 `Translator::default()` 로 안전 폴백(키 그대로 반환).
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
