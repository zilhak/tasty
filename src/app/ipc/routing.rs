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
    if !mgr.owns_namespace(&cmd.request.method) {
        return false;
    }
    host_ipc::handler::idempotency::forward_keeping_the_key(caller, cmd, |c| {
        let id = c.request.id.clone().unwrap_or(serde_json::Value::Null);
        mgr.forward_namespace_call(
            &c.request.method,
            c.request.params.clone(),
            None, // CLI/사용자 호출. plugin → plugin 호출은 별도 경로.
            id,
            c.response_tx.clone(),
            Some(c.request_seq()),
        );
    });
    true
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

        if let Some(resp) = self.dispatch_list_global(&cmd.request) {
            send_response(&cmd.response_tx, resp);
            return IpcStep::Handled;
        }

        // 명시한 자원이 없으면 오류다. 대상이 없는 요청만 workspace 이름·포커스·첫 parked 상태를 사용한다.
        let named = crate::core::request_target::request_resource_id(
            &cmd.request.method,
            &cmd.request.params,
        );
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
    fn send_routed_response(
        &self,
        cmd: &IpcCommand,
        response: host_ipc::protocol::JsonRpcResponse,
    ) {
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
        forwards_once_with_its_seq("image", "image.open", Some("gui-namespace-forward-once"));
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
