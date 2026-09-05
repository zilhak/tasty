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
//! `tasty agent-stream watch|unwatch|list|poll|serve|serve-stop|serve-info` CLI + 같은
//! 이름의 `agent_stream.*` IPC(`serve` 계열은 `serve` / `serve_stop` / `serve_info`)로
//! 노출된다. 이름이 claude 전용이 아닌 이유는 codex 등 다른 에이전트도 transcript 만
//! 다르고 tail·정규화·전송은 같기 때문이다 — 현재 해석되는 소스는 Claude Code 하나다.
//!
//! **이 crate 는 수집과 SSE 방출까지 한다**(`sse` 모듈, `GET /events`). 외부 요청으로
//! 에이전트를 실행시키는 **인바운드 웹훅 배선은 별개**다 — 그 방향은 호스트 리스너가
//! 담당한다. SSE 노출 정책의 근거는 `docs/adr/0100-agent-stream-sse-endpoint-exposure.md`.
//!
//! 호스트 코드에 의존하지 않으며 `tasty-plugin-sdk` 만 사용한다. surface → 세션 id 매핑도
//! claude plugin 이 남긴 surface meta 를 host IPC 로 읽어 얻으므로 plugin 간 코드 의존이
//! 없다(`resolve.rs`).

mod handlers;
mod pump;
mod record;
mod registry;
mod resolve;
mod sse;
mod tail;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use registry::StreamRegistry;
use serde_json::Value;
use sse::server::SseServer;
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
    /// 떠 있는 SSE 엔드포인트. dispatch 스레드에서만 만지므로 별도 락이 필요 없다.
    server: Option<SseServer>,
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
            server: None,
        }
    }
}

impl AgentStreamPlugin {
    /// 스냅샷에 남아 있던 SSE 설정으로 엔드포인트를 다시 연다.
    ///
    /// 호스트는 healthcheck 무응답 시 plugin 을 **강제 재시작**한다. 그때 엔드포인트가
    /// 되살아나지 않으면 watch 는 복구됐는데 소비자가 붙을 곳이 없어, 재구독으로 복구
    /// 한다는 이 설계의 전제가 무너진다. bind 실패는 경고만 남기고 plugin 은 계속 뜬다 —
    /// 수집은 엔드포인트 없이도 유효하다(`poll` 로 읽을 수 있다).
    fn restore_endpoint(&mut self) {
        let reg = match self.registry.lock() {
            Ok(reg) => reg,
            Err(e) => {
                tracing::warn!(
                    "agent-stream: registry lock poisoned at start — the SSE endpoint is not reopened: {e}"
                );
                return;
            }
        };
        let Some(config) = reg.serve_config() else {
            return;
        };
        let hub = reg.hub();
        drop(reg);
        match sse::server::start(config.clone(), hub, self.registry.clone()) {
            Ok(server) => self.server = Some(server),
            Err(e) => tracing::warn!(
                "agent-stream: cannot reopen the SSE endpoint on {}:{} after restart: {e} — collection continues, subscribe again after `tasty agent-stream serve`",
                config.bind,
                config.port
            ),
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
            "agent_stream.turn_start" => {
                handlers::handle_turn_start(&host, &self.registry, &self.translator, params)
            }
            "agent_stream.unwatch" => {
                handlers::handle_unwatch(&host, &self.registry, &self.translator, params)
            }
            "agent_stream.list" => handlers::handle_list(&self.registry, &self.translator),
            "agent_stream.poll" => handlers::handle_poll(&self.registry, &self.translator, params),
            "agent_stream.serve" => {
                handlers::handle_serve(&self.registry, &mut self.server, &self.translator, params)
            }
            "agent_stream.serve_stop" => {
                handlers::handle_serve_stop(&self.registry, &mut self.server, &self.translator)
            }
            "agent_stream.serve_info" => handlers::handle_serve_info(&self.server),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_start(&mut self, host: HostHandle, _bus: BusHandle) {
        self.restore_endpoint();
        let registry = self.registry.clone();
        // spawn 실패 시 패닉을 유지한다 — 호스트(tasty)가 아니라 **이 plugin
        // 프로세스만** 죽고, 호스트는 plugin 사망을 이미 감지·복구한다. 호스트 쪽
        // 스레드 spawn 이 에러 반환으로 바뀐 것과 대칭이 아닌 이유가 이것이다:
        // 실패 폭발 반경이 다르다(`docs/dev-guide/error-handling.md`).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 임시 포트를 리스너째 예약해 반환한다. 소비자(`restore_endpoint`)는 port 번호만 받으므로,
    /// 실제 bind 직전에 이 리스너를 drop 해 예약~bind 사이의 TOCTOU 창을 최소화한다
    /// (ADR-0129 형태 B 정방향, `tasty-ssh::reserve_local_port` 와 동형).
    fn reserve_port() -> (std::net::TcpListener, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        (listener, port)
    }

    #[test]
    fn a_persisted_endpoint_is_reopened_on_the_same_address_after_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (reservation, port) = reserve_port();

        // 1 회차: 설정을 영속한다. (예약 리스너를 쥔 채라 이 사이 포트를 남이 못 가져간다.)
        {
            let mut reg = StreamRegistry::new(Some(dir.path()));
            reg.set_serve_config(Some(sse::ServeConfig {
                bind: "127.0.0.1".into(),
                port,
                token: Some("t".into()),
            }));
            reg.save_if_dirty();
        }

        // 2 회차: 재시작 상당. `new()` 가 스냅샷을 읽고 `restore_endpoint` 가 다시 연다.
        // restore_endpoint 가 이 port 에 실제 bind 하므로, 그 직전에 예약을 놓는다.
        drop(reservation);
        let mut plugin =
            AgentStreamPlugin::new(Some(dir.path().to_path_buf()), Translator::default());
        plugin.restore_endpoint();
        let info = plugin
            .server
            .as_ref()
            .expect("the endpoint is reopened")
            .to_json();
        assert_eq!(
            info["url"],
            Value::from(format!("http://127.0.0.1:{port}/events")),
            "주소가 바뀌면 붙어 있던 소비자가 조용히 떨어진다"
        );
        // 토큰은 공개 뷰에 실리지 않는다.
        assert!(!info.to_string().contains("\"t\""), "{info}");
        plugin.server.take().expect("server").shutdown();
    }

    #[test]
    fn no_persisted_endpoint_means_nothing_is_opened() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plugin =
            AgentStreamPlugin::new(Some(dir.path().to_path_buf()), Translator::default());
        plugin.restore_endpoint();
        assert!(plugin.server.is_none());
    }
}
