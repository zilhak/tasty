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
            crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::Shutdown);
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
            crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::CreateWindow);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"scheduled": true}),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.close" {
            // CLAUDE.md "포커스 독립": id 로 직접 지정. focused 의존 금지.
            let target_id = cmd.request.params.get("id").and_then(|v| v.as_u64());
            let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = match target_id {
                None => host_ipc::protocol::JsonRpcResponse::error(
                    response_id,
                    -32602,
                    "Missing 'id' parameter (u64). focused 의존은 금지.",
                ),
                Some(id_u64) => {
                    let target = self
                        .view
                        .windows
                        .keys()
                        .copied()
                        .find(|w| u64::from(*w) == id_u64);
                    match target {
                        Some(tid) => {
                            self.view.windows.remove(&tid);
                            if self.view.focused_window_id == Some(tid) {
                                self.view.focused_window_id =
                                    self.view.windows.keys().next().copied();
                            }
                            host_ipc::protocol::JsonRpcResponse::success(
                                response_id,
                                serde_json::json!({"closed": true, "id": id_u64}),
                            )
                        }
                        None => host_ipc::protocol::JsonRpcResponse::error(
                            response_id,
                            -32602,
                            format!("Window id {id_u64} not found"),
                        ),
                    }
                }
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // window.focus 는 사용자 입력 재현 (단축키/마우스 클릭 영역) 으로 분류.
        // CLAUDE.md: "CLI/IPC로 포커스·활성 탭·활성 워크스페이스를 전환하는 명령은
        // 존재하지 않는다." → release 빌드에 노출 안 함. debug 빌드만 유지.
        #[cfg(debug_assertions)]
        if cmd.request.method == "window.focus" {
            let target_id = cmd.request.params.get("id").and_then(|v| v.as_u64());
            let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = match target_id {
                None => host_ipc::protocol::JsonRpcResponse::error(
                    response_id,
                    -32602,
                    "Missing 'id' parameter (u64)",
                ),
                Some(id_u64) => {
                    let mut found = false;
                    for (id, w) in &self.view.windows {
                        if w.as_main().is_none() {
                            continue;
                        }
                        if u64::from(*id) == id_u64 {
                            w.base().winit.focus_window();
                            self.view.focused_window_id = Some(*id);
                            found = true;
                            break;
                        }
                    }
                    host_ipc::protocol::JsonRpcResponse::success(
                        response_id,
                        serde_json::json!({"focused": found, "id": id_u64}),
                    )
                }
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.list" {
            let focused_id = self.view.focused_window_id;
            let list: Vec<_> = self
                .view
                .windows
                .iter()
                .filter_map(|(id, w)| {
                    let main = w.as_main()?;
                    Some(serde_json::json!({
                        "id": u64::from(*id),
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
            "plugin.install" => {
                let path = match cmd.request.params.get("path").and_then(|v| v.as_str()) {
                    Some(p) => std::path::PathBuf::from(p),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'path' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                match self.plugin_install(path) {
                    Ok(events) => {
                        // 첫 event 의 plugin_id 가 응답 페이로드용 (Installed CoreEvent).
                        let installed_id = events
                            .iter()
                            .find_map(|ev| match ev {
                                crate::core::intent::CoreEvent::PluginRegistryChanged {
                                    plugin_id,
                                    ..
                                } => Some(plugin_id.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        self.cascade_plugin_events(events);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({ "installed": installed_id }),
                        )
                    }
                    Err(e) => {
                        host_ipc::protocol::JsonRpcResponse::error(id, -32000, &e.to_string())
                    }
                }
            }
            "plugin.remove" => {
                let plugin_id = match cmd.request.params.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'id' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let pid_for_response = plugin_id.clone();
                match self.plugin_remove(plugin_id) {
                    Ok(events) => {
                        self.cascade_plugin_events(events);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({ "removed": pid_for_response }),
                        )
                    }
                    Err(e) => {
                        host_ipc::protocol::JsonRpcResponse::error(id, -32000, &e.to_string())
                    }
                }
            }
            "plugin.enable" => {
                let plugin_id = match cmd.request.params.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'id' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let pid_for_response = plugin_id.clone();
                match self.plugin_enable(plugin_id) {
                    Ok(events) => {
                        self.cascade_plugin_events(events);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({ "enabled": pid_for_response }),
                        )
                    }
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(
                        id,
                        -32000,
                        &format!("enable failed: {e}"),
                    ),
                }
            }
            "plugin.disable" => {
                let plugin_id = match cmd.request.params.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'id' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let pid_for_response = plugin_id.clone();
                match self.plugin_disable(plugin_id) {
                    Ok(events) => {
                        self.cascade_plugin_events(events);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({ "disabled": pid_for_response }),
                        )
                    }
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(
                        id,
                        -32000,
                        &format!("disable failed: {e}"),
                    ),
                }
            }
            "plugin.permissions" => host_ipc::handler::plugin::handle_permissions(
                self.plugin_manager.as_ref(),
                id,
                &cmd.request.params,
            ),
            "plugin.grant" => {
                let plugin_id = match cmd.request.params.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'id' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let token = match cmd
                    .request
                    .params
                    .get("permission")
                    .and_then(|v| v.as_str())
                {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'permission' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let pid_for_response = plugin_id.clone();
                let perm_for_response = token.clone();
                match self.plugin_grant(plugin_id, token) {
                    Ok(events) => {
                        self.cascade_plugin_events(events);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({
                                "id": pid_for_response,
                                "permission": perm_for_response,
                            }),
                        )
                    }
                    Err(e) => {
                        host_ipc::protocol::JsonRpcResponse::error(id, -32000, &e.to_string())
                    }
                }
            }
            "plugin.revoke" => {
                let plugin_id = match cmd.request.params.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'id' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let token = match cmd
                    .request
                    .params
                    .get("permission")
                    .and_then(|v| v.as_str())
                {
                    Some(s) => s.to_string(),
                    None => {
                        send_response(
                            &cmd.response_tx,
                            host_ipc::protocol::JsonRpcResponse::invalid_params(
                                id,
                                "Missing 'permission' parameter",
                            ),
                        );
                        return IpcStep::Handled;
                    }
                };
                let pid_for_response = plugin_id.clone();
                let perm_for_response = token.clone();
                match self.plugin_revoke(plugin_id, token) {
                    Ok(events) => {
                        self.cascade_plugin_events(events);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({
                                "id": pid_for_response,
                                "permission": perm_for_response,
                            }),
                        )
                    }
                    Err(e) => {
                        host_ipc::protocol::JsonRpcResponse::error(id, -32000, &e.to_string())
                    }
                }
            }
            "plugin.grant_agent_permission" => {
                host_ipc::handler::session::handle_grant_agent_permission(
                    &self.core,
                    id,
                    &cmd.request.params,
                )
            }
            "plugin.revoke_agent_permission" => {
                host_ipc::handler::session::handle_revoke_agent_permission(
                    &self.core,
                    id,
                    &cmd.request.params,
                )
            }
            "plugin.list_agent_permissions" => {
                host_ipc::handler::session::handle_list_agent_permissions(
                    &self.core,
                    id,
                    &cmd.request.params,
                )
            }
            "plugin.audit_query" => {
                host_ipc::handler::audit::handle_query(&self.core, id, &cmd.request.params)
            }
            "plugin.audit_summary" => {
                host_ipc::handler::audit::handle_summary(&self.core, id, &cmd.request.params)
            }
            "plugin.audit_follow" => {
                host_ipc::handler::audit::handle_follow(&self.core, id, &cmd.request.params)
            }
            "plugin.audit_clear" => {
                host_ipc::handler::audit::handle_clear(&self.core, id, &cmd.request.params)
            }
            "plugin.request_permission" => {
                // 첫 main window 의 state 를 빌려 사용 (모든 window 가 같은 approval_store
                // Arc 공유). main 이 하나도 없으면 elevation popup 표시 자체가 의미 없으므로
                // internal_error.
                let core = &mut self.core;
                let main = self.view.windows.values_mut().find_map(|w| w.as_main_mut());
                match main {
                    Some(m) => host_ipc::handler::session::handle_request_permission(
                        core,
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

    /// `approval.await`: blocking. Arc<ApprovalStore> + memory port arc 를
    /// worker thread 로 클론해 cascade 없이 자기 수명에서 영속한다.
    fn ipc_dispatch_approval_await(&mut self, cmd: &IpcCommand) {
        let store_opt = self
            .view
            .windows
            .values()
            .find_map(|w| w.as_main().map(|w| w.engine_state.approval_store.clone()))
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|(_, e)| e.approval_store.clone())
            })
            .or_else(|| self.engine_state.as_ref().map(|e| e.approval_store.clone()));
        let memory = self.core.memory_arc();
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        match store_opt {
            Some(store) => {
                let params = cmd.request.params.clone();
                let response_tx = cmd.response_tx.clone();
                std::thread::spawn(move || {
                    let resp = host_ipc::handler::approval::await_blocking(
                        &store, &memory, rpc_id, &params,
                    );
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
