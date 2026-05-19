//! IPC 디스패치 본체 (`process_ipc` — 약 540라인).
//!
//! 호출자(`about_to_wait`)는 매 프레임 1번씩 큐를 drain 한다. caller 결정 → 권한 게이트 →
//! app-level (session/system/window/plugin/debug) 우선 처리 → window-required (IME 등) →
//! approval.await blocking → plugin namespace forward → focused window 또는 parked state
//! 라우터.
//!
//! 본 메서드의 내부 분할은 후속 Level 에서 진행. 본 L1.E 에서는 위치 이동만.

use crate::AppEvent;
use crate::app::App;
use crate::ipc;
use crate::resolve_caller_from_envelope;
use crate::window::Window as _;
#[cfg(debug_assertions)]
use crate::{debug_info, handle_debug_event_bus, handle_debug_extension_invoke_hook};

impl App {
    /// Process pending IPC commands. Returns true if any commands were processed.
    pub(crate) fn process_ipc(&mut self) -> bool {
        use crate::ipc::server::send_response;
        let ipc = match &self.engine.ipc_server {
            Some(ipc) => ipc,
            None => return false,
        };

        let mut processed = false;
        let mut tool_registry_dirty = false;
        while let Ok(cmd) = ipc.try_recv() {
            // Phase 6.2c — envelope 의 session_token 을 검증해 caller 결정.
            // 토큰이 없으면 Local. 있는데 invalid/expired/revoked 면 permission_denied
            // 로 즉시 거부 (Local 로 fallback 하지 않는다 — 위조 방어).
            // 검증을 통과한 caller 가 부적격 메서드(local_only 등)를 호출하면
            // ensure_allowed 가 한 단계 위에서 차단한다.
            let caller = match resolve_caller_from_envelope(&cmd.request) {
                Ok(c) => c,
                Err(resp) => {
                    send_response(&cmd.response_tx, resp);
                    processed = true;
                    continue;
                }
            };
            // Agent caller 는 모든 app-level/local-only 분기를 호출하면 안 된다.
            // method_meta 기반으로 한 번에 차단하고, 통과한 경우에만 분기를 본다.
            // Local caller 는 이전과 동일하게 통과.
            if !matches!(caller, ipc::caller::CallerContext::Local) {
                if let Err(e) = caller.ensure_allowed(&cmd.request.method) {
                    tracing::warn!("ipc agent caller denied: {e}");
                    let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    // Phase 6.5a audit: app-level dispatcher 의 deny 도 기록.
                    if let Some(st) = self
                        .windows
                        .values()
                        .find_map(|w| w.as_main().map(|m| &m.state))
                    {
                        let ws = st
                            .engine
                            .workspaces
                            .get(st.active_workspace)
                            .map(|w| w.id);
                        let seq = st.engine.telemetry_seq.next();
                        ipc::audit::record(
                            &caller,
                            &cmd.request.method,
                            ipc::audit::AuditDecision::Deny,
                            Some(&format!("{e}")),
                            ws,
                            seq,
                        );
                    }
                    // Phase 6.4a — Agent caller 의 MissingPermission 은 elevation
                    // 발행. NotPluginCallable/UnknownMethod 는 elevation 으로
                    // 회복되지 않으므로 단순 deny.
                    let mut data = serde_json::json!(null);
                    if let (
                        ipc::caller::CallerError::MissingPermission { permission, .. },
                        ipc::caller::CallerContext::Agent { agent_id, .. },
                    ) = (&e, &caller)
                    {
                        let agent_id = agent_id.clone();
                        let perm_token = permission.as_token();
                        let method = cmd.request.method.clone();
                        let main_state = self
                            .windows
                            .values_mut()
                            .find_map(|w| w.as_main_mut().map(|m| &mut m.state));
                        if let Some(st) = main_state {
                            if let Some(rec) = ipc::handler::approval::publish_capability_elevation(
                                st,
                                &agent_id,
                                &method,
                                &perm_token,
                                None,
                            ) {
                                data = serde_json::json!({
                                    "kind": "capability_elevation",
                                    "approval_id": rec.request.id,
                                    "permission": perm_token,
                                    "method": method,
                                });
                            }
                        }
                    }
                    let mut response = ipc::protocol::JsonRpcResponse::error(
                        rpc_id,
                        -32001,
                        &format!("permission_denied: {e}"),
                    );
                    if !data.is_null()
                        && let Some(err) = response.error.as_mut()
                    {
                        err.data = Some(data);
                    }
                    send_response(&cmd.response_tx, response);
                    processed = true;
                    continue;
                }
            }
            // App-level IPC methods (don't need focused window)
            #[cfg(debug_assertions)]
            if cmd.request.method == "system.shutdown" {
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"shutdown": true}),
                );
                send_response(&cmd.response_tx, response);
                crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::Shutdown);
                return true;
            }
            if cmd.request.method == "script.reload" {
                let resp_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = match self.lua_engine.as_mut() {
                    None => ipc::protocol::JsonRpcResponse::error(
                        resp_id,
                        -32603,
                        "lua engine not initialized",
                    ),
                    Some(engine) => match engine.reload() {
                        Ok(loaded) => ipc::protocol::JsonRpcResponse::success(
                            resp_id,
                            serde_json::json!({ "loaded": loaded }),
                        ),
                        Err(e) => ipc::protocol::JsonRpcResponse::error(
                            resp_id,
                            -32603,
                            &format!("lua reload failed: {e}"),
                        ),
                    },
                };
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.create" {
                crate::shortcuts::send_app_event(&self.engine.proxy, AppEvent::CreateWindow);
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"scheduled": true}),
                );
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.close" {
                // Close the focused window
                if let Some(focused_id) = self.engine.focused_window_id {
                    self.windows.remove(&focused_id);
                    self.engine.focused_window_id = self.windows.keys().next().copied();
                }
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"closed": true}),
                );
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            if cmd.request.method == "window.focus" {
                // Focus a specific MainWindow by searching for matching ID string
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
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!({"focused": found}),
                );
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            // ── Plugin management (App-level — App holds the PluginManager) ──
            if cmd.request.method.starts_with("plugin.") {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = match cmd.request.method.as_str() {
                    "plugin.list" => {
                        ipc::handler::plugin::handle_list(self.plugin_manager.as_ref(), id)
                    }
                    "plugin.show" => ipc::handler::plugin::handle_show(
                        self.plugin_manager.as_ref(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.extension.list" => {
                        ipc::handler::plugin::handle_extension_list(
                            self.plugin_manager.as_ref(),
                            id,
                        )
                    }
                    "plugin.install" => ipc::handler::plugin::handle_install(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.remove" => ipc::handler::plugin::handle_remove(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.enable" => ipc::handler::plugin::handle_enable(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.disable" => ipc::handler::plugin::handle_disable(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.permissions" => ipc::handler::plugin::handle_permissions(
                        self.plugin_manager.as_ref(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.grant" => ipc::handler::plugin::handle_grant(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.revoke" => ipc::handler::plugin::handle_revoke(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "plugin.grant_agent_permission" => {
                        ipc::handler::session::handle_grant_agent_permission(
                            id,
                            &cmd.request.params,
                        )
                    }
                    "plugin.revoke_agent_permission" => {
                        ipc::handler::session::handle_revoke_agent_permission(
                            id,
                            &cmd.request.params,
                        )
                    }
                    "plugin.list_agent_permissions" => {
                        ipc::handler::session::handle_list_agent_permissions(
                            id,
                            &cmd.request.params,
                        )
                    }
                    "plugin.audit_query" => {
                        ipc::handler::audit::handle_query(id, &cmd.request.params)
                    }
                    "plugin.audit_summary" => {
                        ipc::handler::audit::handle_summary(id, &cmd.request.params)
                    }
                    "plugin.audit_follow" => {
                        ipc::handler::audit::handle_follow(id, &cmd.request.params)
                    }
                    "plugin.audit_clear" => {
                        ipc::handler::audit::handle_clear(id, &cmd.request.params)
                    }
                    "plugin.request_permission" => {
                        // 첫 main window 의 state 를 빌려 사용 (모든 window 가
                        // 같은 approval_store Arc 공유). main 이 하나도 없으면
                        // elevation popup 표시 자체가 의미 없으므로 internal_error.
                        let main_state = self
                            .windows
                            .values_mut()
                            .find_map(|w| w.as_main_mut().map(|m| &mut m.state));
                        match main_state {
                            Some(st) => {
                                ipc::handler::session::handle_request_permission(
                                    st,
                                    &caller,
                                    id,
                                    &cmd.request.params,
                                )
                            }
                            None => ipc::protocol::JsonRpcResponse::error(
                                id,
                                -32603,
                                "no main window available for elevation popup",
                            ),
                        }
                    }
                    other => ipc::protocol::JsonRpcResponse::method_not_found(id, other),
                };
                // plugin 라이프사이클이 바뀌었을 수 있는 메서드만 도구 메뉴 재집계
                // 표시. list/show/permissions/extension.list는 read-only이므로 skip.
                // (실제 refresh는 IPC drain 루프 종료 후 — 루프 안에서는 ipc borrow가 살아있음)
                if matches!(
                    cmd.request.method.as_str(),
                    "plugin.install"
                        | "plugin.remove"
                        | "plugin.enable"
                        | "plugin.disable"
                        | "plugin.grant"
                        | "plugin.revoke"
                ) {
                    tool_registry_dirty = true;
                }
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            // ── Debug Event Bus (App-level — needs PluginManager) ──
            #[cfg(debug_assertions)]
            if cmd.request.method.starts_with("debug.event_bus.") {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = handle_debug_event_bus(
                    self.plugin_manager.as_mut(),
                    &cmd.request.method,
                    &cmd.request.params,
                    id,
                );
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }
            #[cfg(debug_assertions)]
            if cmd.request.method == "debug.extension.invoke_hook" {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                handle_debug_extension_invoke_hook(
                    self.plugin_manager.as_mut(),
                    &cmd.request.params,
                    id,
                    cmd.response_tx.clone(),
                );
                processed = true;
                continue;
            }
            #[cfg(debug_assertions)]
            if cmd.request.method.starts_with("debug.popup.") {
                let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                let response = match cmd.request.method.as_str() {
                    "debug.popup.list" => ipc::handler::popup::handle_list(
                        self.plugin_manager.as_ref(),
                        id,
                    ),
                    "debug.popup.open" => ipc::handler::popup::handle_open(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    "debug.popup.close" => ipc::handler::popup::handle_close(
                        self.plugin_manager.as_mut(),
                        id,
                        &cmd.request.params,
                    ),
                    other => ipc::protocol::JsonRpcResponse::method_not_found(id, other),
                };
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
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
                            "title": main.state.active_workspace().name,
                        }))
                    })
                    .collect();
                let response = ipc::protocol::JsonRpcResponse::success(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    serde_json::json!(list),
                );
                send_response(&cmd.response_tx, response);
                processed = true;
                continue;
            }

            // Window-required IPC methods (GPU, IME, debug)
            #[allow(unused_mut)]
            let mut is_window_required = cmd.request.method.starts_with("surface.ime_");
            #[cfg(debug_assertions)]
            {
                is_window_required = is_window_required
                    || cmd.request.method == "debug.info"
                    || cmd.request.method == "ui.screenshot";
            }
            if is_window_required {
                let focused_id = match self.engine.focused_window_id {
                    Some(id) => id,
                    None => {
                        let response = ipc::protocol::JsonRpcResponse::error(
                            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                            -32000,
                            "No window available for this command",
                        );
                        send_response(&cmd.response_tx, response);
                        processed = true;
                        continue;
                    }
                };
                let w = match self
                    .windows
                    .get_mut(&focused_id)
                    .and_then(|w| w.as_main_mut())
                {
                    Some(w) => w,
                    None => continue,
                };

                #[cfg(debug_assertions)]
                if cmd.request.method == "debug.info" {
                    let debug_data = debug_info::collect(&w.state, Some(&w.base.gpu), w.ime_active);
                    let response = ipc::protocol::JsonRpcResponse::success(
                        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                        debug_data,
                    );
                    send_response(&cmd.response_tx, response);
                    processed = true;
                    continue;
                }
                #[cfg(debug_assertions)]
                if cmd.request.method == "ui.screenshot" {
                    let path = cmd
                        .request
                        .params
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("screenshot.png")
                        .to_string();
                    w.base.gpu.pending_screenshot = Some(std::path::PathBuf::from(&path));
                    w.mark_dirty();
                    let response = ipc::protocol::JsonRpcResponse::success(
                        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                        serde_json::json!({"path": path, "scheduled": true}),
                    );
                    send_response(&cmd.response_tx, response);
                    processed = true;
                    continue;
                }
                if cmd.request.method.starts_with("surface.ime_") {
                    let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    let response = ipc::handler::ime::handle_ime_method(
                        w,
                        &cmd.request.method,
                        &cmd.request.params,
                        id,
                    );
                    send_response(&cmd.response_tx, response);
                    w.base.dirty = true;
                }
                processed = true;
                continue;
            }

            // approval.await: blocking + timeout. 메인 스레드가 막히지 않게 워커
            // 스레드에 위임. Arc<ApprovalStore> 만 클론하면 도메인 단독으로 동작한다.
            if cmd.request.method == "approval.await" {
                let store_opt = self
                    .windows
                    .values()
                    .find_map(|w| w.as_main().map(|w| w.state.engine.approval_store.clone()))
                    .or_else(|| {
                        self.parked_states
                            .first()
                            .map(|s| s.engine.approval_store.clone())
                    });
                let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                match store_opt {
                    Some(store) => {
                        let params = cmd.request.params.clone();
                        let response_tx = cmd.response_tx.clone();
                        std::thread::spawn(move || {
                            let resp =
                                ipc::handler::approval::await_blocking(&store, rpc_id, &params);
                            send_response(&response_tx, resp);
                        });
                    }
                    None => {
                        send_response(
                            &cmd.response_tx,
                            ipc::protocol::JsonRpcResponse::error(
                                rpc_id,
                                -32000,
                                "no application state available",
                            ),
                        );
                    }
                }
                processed = true;
                continue;
            }

            // Plugin namespace IPC: 메서드가 plugin이 contribute한 prefix에 매칭되면
            // owner plugin에 forward. 응답은 plugin이 줄 때까지 보류되며 main loop
            // 다음 tick에서 `plugin_manager.handle_plugin_response`가 client에 회신.
            // 정적/GUI 분기를 모두 통과하지 못한 메서드만 여기 도달하므로, plugin이
            // 호스트 명령을 가릴 수 없다.
            if let Some(mgr) = self.plugin_manager.as_mut() {
                if mgr.ipc_namespaces.resolve(&cmd.request.method).is_some() {
                    let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    mgr.forward_namespace_call(
                        &cmd.request.method,
                        cmd.request.params.clone(),
                        None, // CLI/사용자 호출. plugin → plugin 호출은 step 04에서.
                        id,
                        cmd.response_tx.clone(),
                    );
                    processed = true;
                    continue;
                }
            }

            // All other commands: route to focused MainWindow or parked state
            let focused_id = self.engine.focused_window_id;
            if let Some(id) = focused_id {
                if let Some(w) = self.windows.get_mut(&id).and_then(|w| w.as_main_mut()) {
                    let response =
                        ipc::handler::handle_with_caller(&mut w.state, &cmd.request, &caller);
                    send_response(&cmd.response_tx, response);
                    w.base.dirty = true;
                    processed = true;
                    continue;
                }
            }
            if let Some(state) = self.parked_states.first_mut() {
                let response = ipc::handler::handle_with_caller(state, &cmd.request, &caller);
                send_response(&cmd.response_tx, response);
                processed = true;
            }
        }
        if tool_registry_dirty {
            self.refresh_tool_registry();
        }
        processed
    }
}
