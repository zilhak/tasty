//! step 3 (debug 빌드 only): debug.event_bus.* / debug.extension.invoke_hook / debug.popup.*.

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::handler::debug_plugin;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(crate) fn ipc_step_debug(&mut self, cmd: &IpcCommand) -> IpcStep {
        // 설정 모달을 코드로 강제로 연다 — 사용자 조작(설정 열기) 재현이라 debug 전용.
        // 시각 검증 자동화(렌더 스크린샷 ↔ 디자인 픽셀 대조)의 진입점.
        // open 경로는 App-level `AppEvent::OpenSettings`(proxy) 라 여기서 처리한다.
        #[cfg(feature = "gui")]
        if cmd.request.method == "debug.settings.open" {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let tab = cmd
                .request
                .params
                .get("tab")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            self.pending_settings_tab = tab.clone();
            crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                id,
                serde_json::json!({ "scheduled": true, "tab": tab }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method.starts_with("debug.event_bus.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = debug_plugin::handle_event_bus(
                self.plugin_manager.as_mut(),
                &cmd.request.method,
                &cmd.request.params,
                id,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "debug.extension.invoke_hook" {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            debug_plugin::handle_extension_invoke_hook(
                self.plugin_manager.as_mut(),
                &cmd.request.params,
                id,
                cmd.response_tx.clone(),
            );
            return IpcStep::Handled;
        }
        if cmd.request.method.starts_with("debug.popup.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = match cmd.request.method.as_str() {
                "debug.popup.list" => {
                    host_ipc::handler::popup::handle_list(self.plugin_manager.as_ref(), id)
                }
                "debug.popup.open" => host_ipc::handler::popup::handle_open(
                    self.plugin_manager.as_mut(),
                    id,
                    &cmd.request.params,
                ),
                "debug.popup.close" => host_ipc::handler::popup::handle_close(
                    self.plugin_manager.as_mut(),
                    id,
                    &cmd.request.params,
                ),
                other => host_ipc::protocol::JsonRpcResponse::method_not_found(id, other),
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }
}
