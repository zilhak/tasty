//! step 2: 호스트 자체 메서드.
//!
//! - `system.shutdown` (debug)
//! - `script.reload`
//! - `window.create` / `window.close` / `window.focus` / `window.list`
//! - `plugin.*` (15개 메서드)
//! - `approval.await` (blocking — worker thread 위임)

use crate::AppEvent;
use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(crate) fn ipc_step_app_methods(
        &mut self,
        cmd: &IpcCommand,
        caller: &host_ipc::caller::CallerContext,
    ) -> IpcStep {
        #[cfg(debug_assertions)]
        if cmd.request.method == "system.shutdown" {
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"shutdown": true}),
            );
            send_response(&cmd.response_tx, response);
            crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::Shutdown);
            return IpcStep::Shutdown;
        }
        if cmd.request.method == "script.reload" {
            let resp_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = match self.lua_engine.as_mut() {
                None => host_ipc::protocol::JsonRpcResponse::error(
                    resp_id,
                    -32603,
                    "lua engine not initialized",
                ),
                Some(engine) => match engine.reload() {
                    Ok(loaded) => host_ipc::protocol::JsonRpcResponse::success(
                        resp_id,
                        serde_json::json!({ "loaded": loaded }),
                    ),
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(
                        resp_id,
                        -32603,
                        &format!("lua reload failed: {e}"),
                    ),
                },
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.create" {
            crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::CreateWindow);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"scheduled": true}),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.close" {
            if let Some(focused_id) = self.engine.focused_window_id {
                self.windows.remove(&focused_id);
                self.engine.focused_window_id = self.windows.keys().next().copied();
            }
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"closed": true}),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.focus" {
            let target = cmd
                .request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut found = false;
            for (id, w) in &self.windows {
                if w.as_main().is_none() {
                    continue; // 모달은 focus 대상이 아님
                }
                if format!("{:?}", id) == target {
                    w.base().winit.focus_window();
                    self.engine.focused_window_id = Some(*id);
                    found = true;
                    break;
                }
            }
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"focused": found}),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.list" {
            let focused_id = self.engine.focused_window_id;
            let list: Vec<_> = self
                .windows
                .iter()
                .filter_map(|(id, w)| {
                    let main = w.as_main()?;
                    Some(serde_json::json!({
                        "id": format!("{:?}", id),
                        "focused": focused_id == Some(*id),
                        "title": main.state.active_workspace(&main.engine_state).name,
                    }))
                })
                .collect();
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!(list),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method.starts_with("plugin.") {
            return self.ipc_dispatch_plugin_method(cmd, caller);
        }
        if cmd.request.method == "approval.await" {
            self.ipc_dispatch_approval_await(cmd);
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }

    /// `plugin.*` 메서드 한 묶음. 라이프사이클 변경 메서드는 HandledDirty 반환.
    fn ipc_dispatch_plugin_method(
        &mut self,
        cmd: &IpcCommand,
        caller: &host_ipc::caller::CallerContext,
    ) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = match cmd.request.method.as_str() {
            "plugin.list" => {
                host_ipc::handler::plugin::handle_list(self.plugin_manager.as_ref(), id)
            }
            "plugin.show" => host_ipc::handler::plugin::handle_show(
                self.plugin_manager.as_ref(),
                id,
                &cmd.request.params,
            ),
            "plugin.extension.list" => {
                host_ipc::handler::plugin::handle_extension_list(self.plugin_manager.as_ref(), id)
            }
            "plugin.install" => host_ipc::handler::plugin::handle_install(
                self.plugin_manager.as_mut(),
                id,
                &cmd.request.params,
            ),
            "plugin.remove" => host_ipc::handler::plugin::handle_remove(
                self.plugin_manager.as_mut(),
                id,
                &cmd.request.params,
            ),
            "plugin.enable" => host_ipc::handler::plugin::handle_enable(
                self.plugin_manager.as_mut(),
                id,
                &cmd.request.params,
            ),
            "plugin.disable" => host_ipc::handler::plugin::handle_disable(
                self.plugin_manager.as_mut(),
                id,
                &cmd.request.params,
            ),
            "plugin.permissions" => host_ipc::handler::plugin::handle_permissions(
                self.plugin_manager.as_ref(),
                id,
                &cmd.request.params,
            ),
            "plugin.grant" => host_ipc::handler::plugin::handle_grant(
                self.plugin_manager.as_mut(),
                id,
                &cmd.request.params,
            ),
            "plugin.revoke" => host_ipc::handler::plugin::handle_revoke(
                self.plugin_manager.as_mut(),
                id,
                &cmd.request.params,
            ),
            "plugin.grant_agent_permission" => {
                host_ipc::handler::session::handle_grant_agent_permission(id, &cmd.request.params)
            }
            "plugin.revoke_agent_permission" => {
                host_ipc::handler::session::handle_revoke_agent_permission(id, &cmd.request.params)
            }
            "plugin.list_agent_permissions" => {
                host_ipc::handler::session::handle_list_agent_permissions(id, &cmd.request.params)
            }
            "plugin.audit_query" => host_ipc::handler::audit::handle_query(id, &cmd.request.params),
            "plugin.audit_summary" => {
                host_ipc::handler::audit::handle_summary(id, &cmd.request.params)
            }
            "plugin.audit_follow" => {
                host_ipc::handler::audit::handle_follow(id, &cmd.request.params)
            }
            "plugin.audit_clear" => host_ipc::handler::audit::handle_clear(id, &cmd.request.params),
            "plugin.request_permission" => {
                // 첫 main window 의 state 를 빌려 사용 (모든 window 가 같은 approval_store
                // Arc 공유). main 이 하나도 없으면 elevation popup 표시 자체가 의미 없으므로
                // internal_error.
                let main = self
                    .windows
                    .values_mut()
                    .find_map(|w| w.as_main_mut());
                match main {
                    Some(m) => host_ipc::handler::session::handle_request_permission(
                        &mut m.state,
                        &mut m.engine_state,
                        caller,
                        id,
                        &cmd.request.params,
                    ),
                    None => host_ipc::protocol::JsonRpcResponse::error(
                        id,
                        -32603,
                        "no main window available for elevation popup",
                    ),
                }
            }
            other => host_ipc::protocol::JsonRpcResponse::method_not_found(id, other),
        };
        let dirty = matches!(
            cmd.request.method.as_str(),
            "plugin.install"
                | "plugin.remove"
                | "plugin.enable"
                | "plugin.disable"
                | "plugin.grant"
                | "plugin.revoke"
        );
        send_response(&cmd.response_tx, response);
        if dirty {
            IpcStep::HandledDirty
        } else {
            IpcStep::Handled
        }
    }

    /// `approval.await`: blocking. Arc<ApprovalStore> 만 worker thread 로 클론.
    fn ipc_dispatch_approval_await(&mut self, cmd: &IpcCommand) {
        let store_opt = self
            .windows
            .values()
            .find_map(|w| w.as_main().map(|w| w.engine_state.approval_store.clone()))
            .or_else(|| {
                self.engine_state
                    .as_ref()
                    .map(|e| e.approval_store.clone())
            });
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        match store_opt {
            Some(store) => {
                let params = cmd.request.params.clone();
                let response_tx = cmd.response_tx.clone();
                std::thread::spawn(move || {
                    let resp = host_ipc::handler::approval::await_blocking(&store, rpc_id, &params);
                    send_response(&response_tx, resp);
                });
            }
            None => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::error(
                        rpc_id,
                        -32000,
                        "no application state available",
                    ),
                );
            }
        }
    }
}
