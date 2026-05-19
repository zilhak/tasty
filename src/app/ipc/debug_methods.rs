//! step 3 (debug 빌드 only): debug.event_bus.* / debug.extension.invoke_hook / debug.popup.*.

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};
use crate::{handle_debug_event_bus, handle_debug_extension_invoke_hook};

impl App {
    pub(crate) fn ipc_step_debug(&mut self, cmd: &IpcCommand) -> IpcStep {
        if cmd.request.method.starts_with("debug.event_bus.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = handle_debug_event_bus(
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
            handle_debug_extension_invoke_hook(
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
