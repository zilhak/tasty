//! step 5 (마지막 fallback): plugin namespace forward → focused window /
//! parked state 라우터.

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(crate) fn ipc_step_routing(
        &mut self,
        cmd: &IpcCommand,
        caller: &host_ipc::caller::CallerContext,
    ) -> IpcStep {
        // Plugin namespace forward: 메서드가 plugin contribute 한 prefix 에 매칭되면
        // owner plugin 으로 forward. 응답은 plugin 이 줄 때까지 보류되며 다음 tick 에서
        // `plugin_manager.handle_plugin_response` 가 client 에 회신.
        if let Some(mgr) = self.plugin_manager.as_mut()
            && mgr.ipc_namespaces.resolve(&cmd.request.method).is_some()
        {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            mgr.forward_namespace_call(
                &cmd.request.method,
                cmd.request.params.clone(),
                None, // CLI/사용자 호출. plugin → plugin 호출은 별도 경로.
                id,
                cmd.response_tx.clone(),
            );
            return IpcStep::Handled;
        }

        // list 류는 모든 engine 결과를 합쳐 반환 (포커스 독립 원칙).
        if let Some(resp) = self.dispatch_list_global(&cmd.request) {
            send_response(&cmd.response_tx, resp);
            return IpcStep::Handled;
        }

        // owner main → focused main → parked owner → parked[0] 순으로 라우팅
        // (CLAUDE.md "포커스 독립" 원칙).
        let target_id = match self.find_request_owner(&cmd.request.params) {
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
                    let r = host_ipc::handler::handle_with_caller(
                        core,
                        &mut w.state,
                        &mut w.core_state,
                        &cmd.request,
                        caller,
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
        // parked owner 검사
        let owner_in_parked = crate::app::request_owner::params_resource_id(&cmd.request.params)
            .and_then(|(_, rid)| {
                self.parked_states.iter_mut().find(|(_, e)| match rid.kind {
                    crate::app::request_owner::Kind::Surface => e.has_surface(rid.id),
                    crate::app::request_owner::Kind::Workspace => e.has_workspace(rid.id),
                    crate::app::request_owner::Kind::Pane => e.has_pane(rid.id),
                })
            });
        if let Some((state, engine)) = owner_in_parked {
            let response = host_ipc::handler::handle_with_caller(
                &mut self.core,
                state,
                engine,
                &cmd.request,
                caller,
            );
            send_response(&cmd.response_tx, response);
            self.dispatch_pending_intents();
            return IpcStep::Handled;
        }
        if let Some((state, engine)) = self.parked_states.first_mut() {
            let response = host_ipc::handler::handle_with_caller(
                &mut self.core,
                state,
                engine,
                &cmd.request,
                caller,
            );
            send_response(&cmd.response_tx, response);
            self.dispatch_pending_intents();
        }
        IpcStep::Handled
    }
}
