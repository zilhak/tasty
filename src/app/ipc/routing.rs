//! 플러그인 namespace로 전달하거나 요청 대상을 가진 창·parked engine으로 라우팅한다.

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::app::window_access::engines_mut;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};

/// namespace 소유 메서드면 플러그인에 넘기고 true 를 돌려준다.
/// 공용 메서드 표의 변경 요청은 멱등 키를 확인한다. 플러그인 고유 메서드는 개입하지 않는다.
/// GUI App 은 winit 이벤트 루프 없이 만들 수 없어 매니저만 받는다. 명령당 전달 횟수와 넘긴
/// 번호는 `namespace_forward_tests` 가 잰다.
fn forward_owned_namespace(
    plugin_manager: Option<&mut crate::plugin::PluginManager>,
    caller: &host_ipc::caller::CallerContext,
    cmd: &IpcCommand,
) -> bool {
    let Some(mgr) = plugin_manager else {
        return false;
    };
    if !mgr.owns_namespace(&cmd.request.method)
        || crate::app::journal::commands::host_converts(&cmd.request.method)
    {
        return false;
    }
    let owner = mgr.namespace_owner(&cmd.request.method);
    host_ipc::handler::idempotency::forward_keeping_the_key(caller, cmd, |c| {
        let id = c.request.id.clone().unwrap_or(serde_json::Value::Null);
        mgr.forward_namespace_call(
            &c.request.method,
            c.request.params.clone(),
            None, // CLI/사용자 호출. plugin → plugin 호출은 별도 경로.
            id,
            hand_runs_to_caller(&c.request.method, owner, caller, c.response_tx.clone()),
            Some(c.request_seq()),
        );
    });
    true
}

/// 플러그인이 호스트로 다시 보내 결과 run을 시작하는 namespace 메서드.
const RELAYED_RUN_METHODS: &[&str] = &["markdown.navigate"];

/// 플러그인이 받은 요청을 `host.call`로 다시 보내면 run의 소유자는 그 플러그인이 된다.
/// 응답이 원래 호출자에게 가기 전에 run을 그 호출자에게 넘겨, 요청한 에이전트가 `run.result`로
/// 읽게 한다. 멱등 키를 최초 전달에서 검사하는 것과 같은 이유다. 대상이 아니거나 relay를
/// 시작하지 못하면 응답 채널을 그대로 쓴다.
fn hand_runs_to_caller(
    method: &str,
    plugin: Option<String>,
    caller: &host_ipc::caller::CallerContext,
    response_tx: std::sync::mpsc::SyncSender<host_ipc::protocol::JsonRpcResponse>,
) -> std::sync::mpsc::SyncSender<host_ipc::protocol::JsonRpcResponse> {
    let Some(plugin) =
        plugin.filter(|_| RELAYED_RUN_METHODS.contains(&tasty_ipc::alias::canonicalize(method)))
    else {
        return response_tx;
    };
    let reader = crate::request_runs::reader_of(caller);
    let (relay_tx, relay_rx) =
        std::sync::mpsc::sync_channel::<host_ipc::protocol::JsonRpcResponse>(1);
    let reply = response_tx.clone();
    let spawned = std::thread::Builder::new()
        .name("ipc-run-owner-relay".into())
        .spawn(move || {
            let Ok(response) = relay_rx.recv() else {
                return;
            };
            if let Some(run) = response
                .result
                .as_ref()
                .and_then(|result| result.get("run"))
                .and_then(serde_json::Value::as_u64)
            {
                crate::request_runs::hand_over(run, &plugin, reader);
            }
            send_response(&reply, response);
        });
    match spawned {
        Ok(_) => relay_tx,
        Err(error) => {
            tracing::warn!("run owner relay did not start; the plugin keeps the run: {error}");
            response_tx
        }
    }
}

impl App {
    pub(crate) fn ipc_step_routing(
        &mut self,
        cmd: &IpcCommand,
        checked: &host_ipc::handler::CheckedRequest<'_>,
    ) -> IpcStep {
        if forward_owned_namespace(self.plugin_manager.as_mut(), checked.caller(), cmd) {
            return IpcStep::Handled;
        }

        if self.defer_preset_capture(cmd, checked) {
            return IpcStep::Handled;
        }

        if self.journal.admit_host_fallback_ipc(cmd, checked.caller()) {
            return IpcStep::Handled;
        }

        // Global acknowledgment bypasses the engine handler that normally retains keys.
        // It is synchronous, so retain its result before publishing the response.
        if cmd.request.method == "notification.read" {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = match host_ipc::handler::idempotency::begin(
                std::time::Instant::now(),
                checked.caller(),
                &cmd.request,
                &id,
            ) {
                Ok(pending) => {
                    let response = self
                        .dispatch_list_global(&cmd.request)
                        .expect("notification.read has a global handler");
                    host_ipc::handler::idempotency::finish(
                        std::time::Instant::now(),
                        pending,
                        &response,
                    );
                    response
                }
                Err(response) => response,
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }

        if let Some(resp) = self.dispatch_list_global(&cmd.request) {
            send_response(&cmd.response_tx, resp);
            return IpcStep::Handled;
        }

        // 명시한 자원이 없으면 오류다. 대상이 없는 요청만 workspace 이름·포커스·첫 parked 상태를 사용한다.
        let named = crate::core::request_target::request_resource_id(
            &cmd.request.method,
            &cmd.request.params,
        );
        let with_open_surfaces = self.with_open_surfaces_if_needed(checked);
        let checked = with_open_surfaces.as_ref().unwrap_or(checked);
        let target_id = match self.find_request_owner(&cmd.request.method, &cmd.request.params) {
            Ok(id) if named.is_some() => id,
            Ok(id) => id.or(self.view.focused_view_id),
            Err(msg) => {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(id, msg),
                );
                return IpcStep::Handled;
            }
        };
        if let Some(id) = target_id {
            let core = &mut self.services;
            let resp_opt = engines_mut!(self).window_pair(id).map(|(w, mut engine)| {
                let r = host_ipc::handler::handle_checked_request(
                    core,
                    &mut w.state,
                    &mut engine,
                    checked,
                );
                w.base.state.dirty = true;
                r
            });
            if let Some(response) = resp_opt {
                self.send_routed_response(cmd, response);
                self.dispatch_pending_intents();
                return IpcStep::Handled;
            }
        }
        // 창과 parked 상태가 같은 종류의 자원을 찾도록 공용 판정을 사용한다.
        let owner_in_parked =
            named.and_then(|rid| engines_mut!(self).parked_session_with_resource(rid));
        if let Some((state, mut engine)) = owner_in_parked {
            let response = host_ipc::handler::handle_checked_request(
                &mut self.services,
                state,
                &mut engine,
                checked,
            );
            self.send_routed_response(cmd, response);
            self.dispatch_pending_intents();
            return IpcStep::Handled;
        }
        if let Some(rid) = named {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            send_response(
                &cmd.response_tx,
                host_ipc::protocol::JsonRpcResponse::invalid_params(
                    id,
                    crate::core::request_target::unowned_target_message(rid, &cmd.request.method),
                ),
            );
            return IpcStep::Handled;
        }
        if let Some((state, mut engine)) = engines_mut!(self).first_parked_session() {
            let response = host_ipc::handler::handle_checked_request(
                &mut self.services,
                state,
                &mut engine,
                checked,
            );
            self.send_routed_response(cmd, response);
            self.dispatch_pending_intents();
        }
        IpcStep::Handled
    }
}

impl App {
    /// 창·parked 상태가 만든 응답을 보낸다. debug ui.state는 App이 소유한 활성 모달을 덧붙인다.
    /// memory writer 결과를 기다리는 요청은 이 연결로 나중에 응답한다.
    fn send_routed_response(&self, cmd: &IpcCommand, handled: host_ipc::handler::Handled) {
        let response = match handled {
            host_ipc::handler::Handled::Now(response) => response,
            host_ipc::handler::Handled::Later(deferred) => {
                self.services.defer_reply(
                    deferred,
                    host_ipc::handler::ReplyTo::Ipc(cmd.response_tx.clone()),
                );
                return;
            }
        };
        #[cfg(debug_assertions)]
        let response = self.project_active_modal(&cmd.request.method, response);
        send_response(&cmd.response_tx, response);
    }
}

/// IPC 명령 하나가 namespace 소유 메서드로 오면 플러그인 전달이 정확히 한 번이고, 그 대기
/// 항목이 명령의 요청 번호를 드는지 stub 플러그인으로 잰다. `forward_owned_namespace` 안에서
/// 파일 밖 헬퍼를 거친 추가 전달은 stub 이 받은 요청 수로 드러난다. GUI App 을 시험에서 만들 수
/// 없어 `ipc_step_routing` 에서 이 함수 밖에 둔 전달은 여기서 덮지 않는다. 그 전달은 실제
/// 인스턴스 e2e `a_plugin_namespace_call_is_forwarded_to_the_plugin_exactly_once` 가 plugin
/// 왕복 수의 차분으로 잰다.
#[cfg(test)]
mod run_owner_relay_tests {
    use super::*;
    use crate::request_runs::{begin, lookup};
    use host_ipc::caller::CallerContext;
    use host_ipc::protocol::JsonRpcResponse;

    fn agent(id: &str) -> CallerContext {
        CallerContext::Agent {
            agent_id: id.to_string(),
            permissions: std::sync::Arc::new(std::collections::HashSet::new()),
        }
    }

    fn relay(method: &str, caller: &CallerContext, run: u64) -> JsonRpcResponse {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let relay = hand_runs_to_caller(method, Some("tasty.markdown".into()), caller, tx);
        relay
            .send(JsonRpcResponse::success(
                serde_json::Value::Null,
                serde_json::json!({ "accepted": true, "run": run }),
            ))
            .unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn a_relayed_navigate_run_goes_to_the_agent_that_asked() {
        let run = begin(
            "markdown.navigate",
            Some("tasty.markdown".into()),
            serde_json::Value::Null,
        );
        let answer = relay("markdown.navigate", &agent("claude:a"), run);
        assert_eq!(answer.result.unwrap()["run"], run);
        assert!(lookup(run, Some("claude:a")).is_some());
        assert!(lookup(run, Some("claude:b")).is_none());
        assert!(lookup(run, Some("tasty.markdown")).is_none());
    }

    #[test]
    fn only_listed_methods_and_plugin_owned_runs_are_handed_over() {
        let other = begin(
            "x.method",
            Some("tasty.markdown".into()),
            serde_json::Value::Null,
        );
        relay("markdown.reload", &agent("claude:a"), other);
        assert!(lookup(other, Some("tasty.markdown")).is_some());
        let foreign = begin(
            "markdown.navigate",
            Some("claude:z".into()),
            serde_json::Value::Null,
        );
        relay("markdown.navigate", &agent("claude:a"), foreign);
        assert!(lookup(foreign, Some("claude:z")).is_some());
        assert!(lookup(foreign, Some("claude:a")).is_none());
    }
}

#[cfg(test)]
mod namespace_forward_tests {
    use std::sync::{Arc, mpsc};

    use serde_json::json;

    use super::forward_owned_namespace;
    use crate::ipc::caller::CallerContext;
    use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
    use crate::ipc::server::IpcCommand;
    use crate::plugin::PluginManager;

    const OWNER: &str = "com.test.gui-namespace-forward";

    fn manager() -> PluginManager {
        PluginManager::with_registries(
            Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
            Arc::new(crate::file::format::FileFormatRegistry::new()),
            Arc::new(crate::file::handler::FileHandlerRegistry::new()),
        )
    }

    fn command(method: &str, key: Option<&str>) -> (IpcCommand, mpsc::Receiver<JsonRpcResponse>) {
        let (tx, rx) = mpsc::sync_channel(4);
        let request = JsonRpcRequest {
            caller_agent_id: None,
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: json!({}),
            id: Some(json!(1)),
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: key.map(str::to_string),
        };
        (IpcCommand::new(request, tx), rx)
    }

    fn forwards_once_with_its_seq(prefix: &str, method: &str, key: Option<&str>) {
        let mut mgr = manager();
        let stub = mgr.attach_namespace_stub_for_test(OWNER, prefix);
        let (cmd, _rx) = command(method, key);
        assert!(forward_owned_namespace(
            Some(&mut mgr),
            &CallerContext::local(),
            &cmd
        ));
        let sent = stub.drain_invokes();
        assert_eq!(
            sent.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(),
            [method],
            "명령 하나에 플러그인 전달이 정확히 한 번이어야 한다"
        );
        assert_eq!(
            mgr.pending_origins_for_test(OWNER),
            [(sent[0].0, Some(cmd.request_seq()))],
            "전달의 대기 항목이 명령의 요청 번호를 들어야 한다"
        );
    }

    #[test]
    fn a_plugin_method_is_forwarded_once_with_the_commands_request_seq() {
        forwards_once_with_its_seq("guifwdns", "guifwdns.run", None);
    }

    /// 키가 있는 계약 안 메서드는 보존소의 relay 안에서 같은 전달을 부른다.
    #[test]
    fn a_keyed_table_method_is_forwarded_once_with_the_commands_request_seq() {
        forwards_once_with_its_seq("image", "image.next", Some("gui-namespace-forward-once"));
    }

    /// 호스트 변환을 소유 플러그인에 보내면 그 플러그인 worker 가 host.call 로 되돌려 보내는
    /// 동안 호스트는 같은 worker 의 옛 surface 회수 확인을 기다려 호출 시한까지 멈춘다.
    #[test]
    fn a_host_converted_method_is_not_forwarded_to_its_namespace_owner() {
        let mut mgr = manager();
        let stub = mgr.attach_namespace_stub_for_test(OWNER, "image");
        let (cmd, _rx) = command("image.open", Some("gui-host-conversion"));
        assert!(!forward_owned_namespace(
            Some(&mut mgr),
            &CallerContext::local(),
            &cmd
        ));
        assert!(stub.drain_invokes().is_empty());
    }

    #[test]
    fn a_method_outside_every_namespace_is_not_forwarded() {
        let mut mgr = manager();
        let stub = mgr.attach_namespace_stub_for_test(OWNER, "guifwdns");
        let (cmd, _rx) = command("workspace.list", None);
        assert!(!forward_owned_namespace(
            Some(&mut mgr),
            &CallerContext::local(),
            &cmd
        ));
        assert!(stub.drain_invokes().is_empty());
        assert!(!forward_owned_namespace(
            None,
            &CallerContext::local(),
            &cmd
        ));
    }
}
