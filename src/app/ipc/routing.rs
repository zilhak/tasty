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
        if let Some(mgr) = self.plugin_manager.as_mut() {
            if mgr.ipc_namespaces.resolve(&cmd.request.method).is_some() {
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
        }

        // focused MainWindow 또는 parked state 로 라우팅.
        let focused_id = self.engine.focused_window_id;
        if let Some(id) = focused_id {
            if let Some(w) = self.windows.get_mut(&id).and_then(|w| w.as_main_mut()) {
                let response = host_ipc::handler::handle_with_caller(
                    &mut w.state,
                    &mut w.engine_state,
                    &cmd.request,
                    caller,
                );
                send_response(&cmd.response_tx, response);
                w.base.dirty = true;
                return IpcStep::Handled;
            }
        }
        // parked AppState 는 engine 이 분리돼 있지 않으므로 App.engine_state 를 빌려 사용.
        if let (Some(state), Some(engine)) = (
            self.parked_states.first_mut(),
            self.engine_state.as_mut(),
        ) {
            let response =
                host_ipc::handler::handle_with_caller(state, engine, &cmd.request, caller);
            send_response(&cmd.response_tx, response);
        }
        IpcStep::Handled
    }
}
