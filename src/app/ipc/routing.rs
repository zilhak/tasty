//! 플러그인 namespace로 전달하거나 요청 대상을 가진 창·parked engine으로 라우팅한다.

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(crate) fn ipc_step_routing(
        &mut self,
        cmd: &IpcCommand,
        checked: &host_ipc::handler::CheckedRequest<'_>,
    ) -> IpcStep {
        // 공용 메서드 표의 변경 요청은 멱등 키를 확인한다. 플러그인 고유 메서드는 개입하지 않는다.
        if let Some(mgr) = self.plugin_manager.as_mut()
            && mgr.owns_namespace(&cmd.request.method)
        {
            host_ipc::handler::idempotency::forward_keeping_the_key(checked.caller(), cmd, |c| {
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
            let core = &mut self.core;
            let resp_opt = self
                .view
                .views
                .get_mut(&id)
                .and_then(|w| w.as_main_mut())
                .map(|w| {
                    let r = host_ipc::handler::handle_checked_request(
                        core,
                        &mut w.state,
                        &mut w.core_state,
                        checked,
                    );
                    w.base.dirty = true;
                    r
                });
            if let Some(response) = resp_opt {
                send_response(&cmd.response_tx, response);
                self.dispatch_pending_intents();
                return IpcStep::Handled;
            }
        }
        // 창과 parked 상태가 같은 종류의 자원을 찾도록 공용 판정을 사용한다.
        let owner_in_parked = named.and_then(|rid| {
            self.parked_states
                .iter_mut()
                .find(|(_, e)| crate::core::request_target::engine_has_resource(e, rid))
        });
        if let Some((state, engine)) = owner_in_parked {
            let response =
                host_ipc::handler::handle_checked_request(&mut self.core, state, engine, checked);
            send_response(&cmd.response_tx, response);
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
        if let Some((state, engine)) = self.parked_states.first_mut() {
            let response =
                host_ipc::handler::handle_checked_request(&mut self.core, state, engine, checked);
            send_response(&cmd.response_tx, response);
            self.dispatch_pending_intents();
        }
        IpcStep::Handled
    }
}
