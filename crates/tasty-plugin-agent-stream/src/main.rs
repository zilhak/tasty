// Windows 에서만 파일 지문 하나를 Win32 FFI 로 얻는다(`tail.rs` 의 `file_identity`) —
// std 의 대응 API 가 unstable 이라 대안이 없다. 그 한 함수는 `#[allow(unsafe_code)]` 로
// 열려 있고 나머지는 여전히 금지다. 다른 플랫폼에서는 crate 전체가 unsafe 금지(`forbid`).
#![cfg_attr(not(windows), forbid(unsafe_code))]
#![cfg_attr(windows, deny(unsafe_code))]

//! Tasty Agent Stream plugin — 외부 plugin.
//!
//! AI 코딩 에이전트가 도는 surface 를 지정하면, 그 **세션 transcript** 를 tail 해
//! `text` / `thinking` / `tool_use` / `turn_end` 구조화 이벤트로 수집한다. 화면 스크레이핑
//! (`tasty read screen`, `output-match` 훅)과 달리 ANSI·박스 문자·줄바꿈이 섞이지 않고,
//! 사고 블록과 응답 텍스트가 소스에서부터 분리돼 있다.
//!
//! `tasty agent-stream watch|unwatch|list|poll` CLI + 같은 이름의 `agent_stream.*` IPC
//! 로 노출된다. 이름이 claude 전용이 아닌 이유는 codex 등 다른 에이전트도 transcript 만
//! 다르고 tail·정규화·전송은 같기 때문이다 — 현재 해석되는 소스는 Claude Code 하나다.
//!
//! **이 crate 는 수집까지만 한다.** 외부 방출(SSE)과 인바운드 웹훅 배선은 별개다.
//!
//! 호스트 코드에 의존하지 않으며 `tasty-plugin-sdk` 만 사용한다. surface → 세션 id 매핑도
//! claude plugin 이 남긴 surface meta 를 host IPC 로 읽어 얻으므로 plugin 간 코드 의존이
//! 없다(`resolve.rs`).

mod handlers;
mod pump;
mod record;
mod registry;
mod resolve;
mod tail;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use registry::StreamRegistry;
use serde_json::Value;
use tasty_plugin_sdk::{
    BusHandle, HostHandle, IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, SurfaceCreateCtx,
    SurfaceResult, i18n::Translator,
};

const PLUGIN_ID: &str = "com.tasty.agent-stream";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

struct AgentStreamPlugin {
    /// watch 대상 · 수집 이벤트 버퍼. IPC 핸들러(worker 스레드)와 tail 스레드가 공유한다.
    registry: Arc<Mutex<StreamRegistry>>,
    /// 사람이 읽는 IPC 에러 문자열 번역용. `main()` 에서 1 회 로드해 재사용한다.
    translator: Translator,
}

impl AgentStreamPlugin {
    fn new(data_dir: Option<PathBuf>, translator: Translator) -> Self {
        let mut registry = StreamRegistry::new(data_dir.as_deref());
        // 강제 재시작 후 watch 를 되살린다 — 등록이 사라지면 소비자는 스트림이 붙어 있다고
        // 믿은 채 아무것도 받지 못한다.
        registry.restore();
        Self {
            registry: Arc::new(Mutex::new(registry)),
            translator,
        }
    }
}

impl Plugin for AgentStreamPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // 자체 surface_kind 를 등록하지 않는다 — 이 plugin 은 화면을 그리지 않는
        // headless 수집기다. 매니페스트에 surface_kinds 가 없으므로 호출되지 않는다.
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
            "agent_stream.watch" => {
                handlers::handle_watch(&host, &self.registry, &self.translator, params)
            }
            "agent_stream.unwatch" => {
                handlers::handle_unwatch(&self.registry, &self.translator, params)
            }
            "agent_stream.list" => handlers::handle_list(&self.registry, &self.translator),
            "agent_stream.poll" => handlers::handle_poll(&self.registry, &self.translator, params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_start(&mut self, host: HostHandle, _bus: BusHandle) {
        let registry = self.registry.clone();
        std::thread::Builder::new()
            .name("agent-stream-tail".into())
            .spawn(move || pump::tail_loop(registry, host))
            .expect("spawn agent-stream-tail thread");
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    // `env` 부재(비정상 기동)에도 안전 폴백: Translator 는 키를 그대로 반환하고,
    // data_dir 이 없으면 영속화를 건너뛴다(조용히 다른 경로에 쓰지 않는다).
    let env = PluginEnv::load().ok();
    let data_dir = env.as_ref().and_then(|e| e.data_dir.clone());
    let translator = env
        .as_ref()
        .map(Translator::from_plugin_env)
        .unwrap_or_default();
    tasty_plugin_sdk::run(AgentStreamPlugin::new(data_dir, translator))
}
