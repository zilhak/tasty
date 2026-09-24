//! 창·플러그인 등 App 자원이 필요한 IPC 메서드를 처리한다.

mod remote;

use crate::AppEvent;
use crate::adapters::ipc::handler::params;
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
        // engine 라우터를 거치지 않는 변경도 멱등 키를 확인한다. 맡지 않은 메서드는 다음 단계가 다시 확인한다.
        if let Some(step) = host_ipc::handler::idempotency::run_app_layer(
            caller,
            cmd,
            IpcStep::Handled,
            |step| !matches!(step, IpcStep::NotHandled),
            |relayed| self.ipc_step_app_methods(relayed, caller),
        ) {
            return step;
        }
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
        if cmd.request.method == "system.gpu_stats" {
            return self.ipc_handle_system_gpu_stats(cmd);
        }
        if cmd.request.method == "timer.list" {
            return self.ipc_handle_timer_list(cmd);
        }
        if cmd.request.method == "window.create" || cmd.request.method == "view.create" {
            // 이벤트 처리 뒤 창 생성 결과를 회신해 예약 접수와 실제 성공을 구별한다.
            let completion = crate::app::event::IpcCompletion::new(
                cmd.response_tx.clone(),
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            );
            crate::shortcuts::send_app_event(
                &self.view.proxy,
                AppEvent::CreateWindow(
                    crate::app::event::WindowRequestOrigin::Agent,
                    Some(completion),
                ),
            );
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.close" || cmd.request.method == "view.close" {
            return self.ipc_handle_window_close(cmd);
        }
        // 포커스 변경은 사용자 입력 재현이므로 debug에만 제공한다.
        #[cfg(debug_assertions)]
        if cmd.request.method == "window.focus" || cmd.request.method == "view.focus" {
            return self.ipc_handle_window_focus(cmd);
        }
        if cmd.request.method == "window.list" || cmd.request.method == "view.list" {
            return self.ipc_handle_window_list(cmd);
        }
        if cmd.request.method == "ui.screenshot" {
            return self.ipc_handle_ui_screenshot(cmd);
        }
        if cmd.request.method == "clipboard.set_text" {
            return self.ipc_handle_clipboard_set_text(cmd);
        }
        if cmd.request.method.starts_with("plugin.") {
            return self.ipc_dispatch_plugin_method(cmd, caller);
        }
        if cmd.request.method == "approval.await" {
            self.ipc_dispatch_approval_await(cmd);
            return IpcStep::Handled;
        }
        if cmd.request.method == "events.fetch" {
            self.ipc_dispatch_events_fetch(cmd);
            return IpcStep::Handled;
        }
        if cmd.request.method == "agent.task_await" {
            self.ipc_dispatch_task_await(cmd);
            return IpcStep::Handled;
        }
        if cmd.request.method == "remote.workspaces" {
            self.ipc_dispatch_remote_workspaces(cmd);
            return IpcStep::Handled;
        }
        if cmd.request.method == "remote.attach" {
            self.ipc_dispatch_remote_attach(cmd);
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }

    /// App과 플러그인 매니저가 가진 타이머의 등록 상태를 함께 조회한다.
    fn ipc_handle_timer_list(&self, cmd: &IpcCommand) -> IpcStep {
        let response = host_ipc::protocol::JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            self.timer_list_json(std::time::Instant::now()),
        );
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// 전체 wgpu 자원 수와 창별 렌더 자원·explorer view 수를 조회한다.
    fn ipc_handle_system_gpu_stats(&self, cmd: &IpcCommand) -> IpcStep {
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let windows: Vec<_> = self
            .view
            .views
            .iter()
            .map(|(id, w)| {
                serde_json::json!({
                    "window_id": u64::from(*id),
                    "main": w.as_main().is_some(),
                    "stats": w.base().gpu.resource_stats(),
                    // explorer store는 MainView에만 있어 다른 창은 null이다.
                    "explorer_views": w.as_main().map(|m| m.state.explorer_views.view_count()),
                })
            })
            .collect();
        // wgpu-core의 재수출 타입에 의존하지 않도록 필드 접근 macro로 JSON을 만든다.
        macro_rules! reg {
            ($r:expr) => {
                serde_json::json!({
                    "allocated": $r.num_allocated,
                    "kept_from_user": $r.num_kept_from_user,
                    "released_from_user": $r.num_released_from_user,
                })
            };
        }
        let wgpu_report = self.gpu_instance.generate_report().map(|r| {
            serde_json::json!({
                "surfaces": reg!(r.surfaces),
                "hub": {
                    "devices": reg!(r.hub.devices),
                    "queues": reg!(r.hub.queues),
                    "buffers": reg!(r.hub.buffers),
                    "textures": reg!(r.hub.textures),
                    "texture_views": reg!(r.hub.texture_views),
                    "samplers": reg!(r.hub.samplers),
                    "bind_groups": reg!(r.hub.bind_groups),
                    "bind_group_layouts": reg!(r.hub.bind_group_layouts),
                    "pipeline_layouts": reg!(r.hub.pipeline_layouts),
                    "shader_modules": reg!(r.hub.shader_modules),
                    "render_pipelines": reg!(r.hub.render_pipelines),
                    "compute_pipelines": reg!(r.hub.compute_pipelines),
                    "command_buffers": reg!(r.hub.command_buffers),
                    "render_bundles": reg!(r.hub.render_bundles),
                    "query_sets": reg!(r.hub.query_sets),
                },
            })
        });
        let response = host_ipc::protocol::JsonRpcResponse::success(
            response_id,
            serde_json::json!({
                "windows": windows,
                "wgpu": wgpu_report,
            }),
        );
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// ID로 지정한 MainView만 닫는다. 마지막 MainView는 종료 동작이라 거절한다.
    fn ipc_handle_window_close(&mut self, cmd: &IpcCommand) -> IpcStep {
        let target_id = params::read_int::<u64>(&cmd.request.params, "id");
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = match target_id {
            Err(msg) => host_ipc::protocol::JsonRpcResponse::error(response_id, -32602, &msg),
            Ok(None) => host_ipc::protocol::JsonRpcResponse::error(
                response_id,
                -32602,
                "Missing 'id' parameter (u64); specify the target window.",
            ),
            Ok(Some(id_u64)) => {
                // 모달·preset은 window.list와 IPC 닫기 대상에 포함하지 않는다.
                let mains: Vec<_> = self
                    .view
                    .views
                    .iter()
                    .filter(|(_, w)| w.as_main().is_some())
                    .map(|(id, _)| *id)
                    .collect();
                let target = mains.iter().copied().find(|w| u64::from(*w) == id_u64);
                match target {
                    Some(_) if mains.len() <= 1 => host_ipc::protocol::JsonRpcResponse::error(
                        response_id,
                        -32000,
                        "Cannot close the last main window via IPC — quitting the app is a user action",
                    ),
                    Some(tid) => {
                        self.close_main_window(tid, tasty_plugin_protocol::LifecycleReason::Ipc);
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
        IpcStep::Handled
    }

    /// 사용자 포커스 변경을 재현하는 debug 메서드다.
    #[cfg(debug_assertions)]
    fn ipc_handle_window_focus(&mut self, cmd: &IpcCommand) -> IpcStep {
        let target_id = params::read_int::<u64>(&cmd.request.params, "id");
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = match target_id {
            Err(msg) => host_ipc::protocol::JsonRpcResponse::error(response_id, -32602, &msg),
            Ok(None) => host_ipc::protocol::JsonRpcResponse::error(
                response_id,
                -32602,
                "Missing 'id' parameter (u64)",
            ),
            Ok(Some(id_u64)) => {
                let mut found = false;
                for (id, w) in &self.view.views {
                    if w.as_main().is_none() {
                        continue;
                    }
                    if u64::from(*id) == id_u64 {
                        w.base().winit.focus_window();
                        self.view.focused_view_id = Some(*id);
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
        IpcStep::Handled
    }

    /// MainView만 열거한다. parked engine은 슬롯을 점유해도 창 ID가 없어 이 목록에 넣지 않는다.
    fn ipc_handle_window_list(&self, cmd: &IpcCommand) -> IpcStep {
        let focused_id = self.view.focused_view_id;
        let list: Vec<_> = self
            .view
            .views
            .iter()
            .filter_map(|(id, w)| {
                let main = w.as_main()?;
                let mut info = host_ipc::handler::system_info_fields(&main.state, &main.core_state);
                info["id"] = serde_json::json!(u64::from(*id));
                info["focused"] = serde_json::json!(focused_id == Some(*id));
                info["title"] =
                    serde_json::json!(main.state.active_workspace(&main.core_state).name);
                Some(info)
            })
            .collect();
        let response = host_ipc::protocol::JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            serde_json::json!(list),
        );
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// 터미널 surface는 별도 렌더로, 창은 프레임으로 캡처한다.
    /// 포커스를 바꾸지 않으며 창 ID가 없으면 MainView가 정확히 하나일 때만 선택한다.
    fn ipc_handle_ui_screenshot(&mut self, cmd: &IpcCommand) -> IpcStep {
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let path = match cmd.request.params.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::error(
                        response_id,
                        -32602,
                        "Missing 'path' parameter (string)",
                    ),
                );
                return IpcStep::Handled;
            }
        };
        let surface_id = match crate::adapters::ipc::handler::params::read_u32(
            &cmd.request.params,
            "surface_id",
        ) {
            Ok(v) => v,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(response_id, msg),
                );
                return IpcStep::Handled;
            }
        };
        let window_id = match params::read_int::<u64>(&cmd.request.params, "window_id") {
            Ok(v) => v,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::error(response_id, -32602, &msg),
                );
                return IpcStep::Handled;
            }
        };

        if let Some(sid) = surface_id {
            return self.ipc_screenshot_surface(cmd, response_id, &path, sid);
        }

        let ids: Vec<_> = self.view.views.keys().copied().collect();
        let kinds: Vec<(u64, bool)> = ids
            .iter()
            .map(|id| {
                let is_main = self
                    .view
                    .views
                    .get(id)
                    .is_some_and(|w| w.as_main().is_some());
                (u64::from(*id), is_main)
            })
            .collect();
        let response = match resolve_screenshot_window(&kinds, window_id) {
            Ok(pos) => {
                let tid = ids[pos];
                match self.view.views.get_mut(&tid) {
                    Some(w) => {
                        let base = w.base_mut();
                        base.gpu.pending_screenshot = Some(std::path::PathBuf::from(&path));
                        base.dirty = true;
                        base.winit.request_redraw();
                        host_ipc::protocol::JsonRpcResponse::success(
                            response_id,
                            serde_json::json!({
                                "path": path,
                                "window_id": u64::from(tid),
                                "scheduled": true,
                            }),
                        )
                    }
                    None => host_ipc::protocol::JsonRpcResponse::error(
                        response_id,
                        -32603,
                        "Target window disappeared while resolving",
                    ),
                }
            }
            Err((code, msg)) => host_ipc::protocol::JsonRpcResponse::error(response_id, code, msg),
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }
    fn ipc_handle_clipboard_set_text(&mut self, cmd: &IpcCommand) -> IpcStep {
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let resp = crate::core::app_surface::clipboard_set_text(
            &self.core,
            response_id,
            &cmd.request.params,
        );
        send_response(&cmd.response_tx, resp);
        IpcStep::Handled
    }

    /// surface ID의 소유 창을 찾아 터미널 캡처를 예약한다. 비터미널 surface는 거절한다.
    fn ipc_screenshot_surface(
        &mut self,
        cmd: &IpcCommand,
        response_id: serde_json::Value,
        path: &str,
        sid: u32,
    ) -> IpcStep {
        let owner = self.view.views.values_mut().find_map(|w| {
            let m = w.as_main_mut()?;
            if m.core_state.has_surface(sid) {
                Some(m)
            } else {
                None
            }
        });
        let response = match owner {
            None => host_ipc::protocol::JsonRpcResponse::error(
                response_id,
                -32602,
                format!("Surface {sid} not found"),
            ),
            Some(m) => {
                let kind = m.core_state.find_surface_by_id(sid).map(|s| s.kind());
                if kind == Some("terminal") {
                    m.base.gpu.pending_surface_screenshot =
                        Some((sid, std::path::PathBuf::from(path)));
                    m.base.dirty = true;
                    m.base.winit.request_redraw();
                    host_ipc::protocol::JsonRpcResponse::success(
                        response_id,
                        serde_json::json!({
                            "path": path,
                            "surface_id": sid,
                            "scheduled": true,
                        }),
                    )
                } else {
                    host_ipc::protocol::JsonRpcResponse::error(
                        response_id,
                        -32000,
                        format!(
                            "Surface {sid} is kind '{}' — only terminal surfaces can be captured (egui panels / plugin / webview are out of scope)",
                            kind.unwrap_or("unknown")
                        ),
                    )
                }
            }
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    fn ipc_dispatch_plugin_method(
        &mut self,
        cmd: &IpcCommand,
        caller: &host_ipc::caller::CallerContext,
    ) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        // 읽기 전용 메서드 표는 헤드리스와 공유한다.
        let surface_registry = self.core_state().surface_registry.clone();
        if let Some(response) = host_ipc::handler::plugin::dispatch_readonly(
            &self.core,
            self.plugin_manager.as_ref(),
            &surface_registry,
            cmd.request.method.as_str(),
            id.clone(),
            &cmd.request.params,
        ) {
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        let response = match cmd.request.method.as_str() {
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
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(id, -32000, e.to_string()),
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
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(id, -32000, e.to_string()),
                }
            }
            // 상태 변경은 헤드리스와 공유하고 결과 이벤트의 전달만 빌드별로 처리한다.
            "plugin.enable" | "plugin.disable" => {
                let surface_registry = self.core_state().surface_registry.clone();
                let Some((response, events)) = host_ipc::handler::plugin::dispatch_lifecycle_toggle(
                    self.plugin_manager.as_mut(),
                    &surface_registry,
                    cmd.request.method.as_str(),
                    id,
                    &cmd.request.params,
                ) else {
                    unreachable!("plugin.enable/disable is missing from the toggle table");
                };
                self.cascade_plugin_events(events);
                response
            }
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
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(id, -32000, e.to_string()),
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
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(id, -32000, e.to_string()),
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
            "plugin.upgrade_builtins" => {
                let force = cmd
                    .request
                    .params
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let restore_removed: Vec<String> = cmd
                    .request
                    .params
                    .get("restore_removed")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let restore_all = cmd
                    .request
                    .params
                    .get("restore_all")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let restart_running = cmd
                    .request
                    .params
                    .get("restart_running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                match self.plugin_upgrade_builtins(
                    force,
                    restore_removed,
                    restore_all,
                    restart_running,
                ) {
                    Ok((report, events)) => {
                        self.cascade_plugin_events(events);
                        match serde_json::to_value(&report) {
                            Ok(v) => host_ipc::protocol::JsonRpcResponse::success(id, v),
                            Err(e) => host_ipc::protocol::JsonRpcResponse::error(
                                id,
                                -32603,
                                format!("serialize report failed: {e}"),
                            ),
                        }
                    }
                    Err(e) => host_ipc::protocol::JsonRpcResponse::error(id, -32000, e.to_string()),
                }
            }
            "plugin.audit_follow" => {
                host_ipc::handler::audit::handle_follow(&self.core, id, &cmd.request.params)
            }
            "plugin.audit_clear" => {
                host_ipc::handler::audit::handle_clear(&self.core, id, &cmd.request.params)
            }
            "plugin.request_permission" => {
                // 창 생성 경로가 approval_store Arc를 공유하므로 첫 MainView를 사용한다.
                // 창이 없으면 승인 popup을 표시할 수 없어 거절한다.
                let core = &mut self.core;
                let main = self.view.views.values_mut().find_map(|w| w.as_main_mut());
                match main {
                    Some(m) => host_ipc::handler::session::handle_request_permission(
                        core,
                        &mut m.state,
                        &mut m.core_state,
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
                | "plugin.upgrade_builtins"
        );
        send_response(&cmd.response_tx, response);
        if dirty {
            IpcStep::HandledDirty
        } else {
            IpcStep::Handled
        }
    }
    /// wait_ms가 있으면 워커에서 기다려 이벤트 루프를 막지 않는다. 버스 사본은 같은 링을 공유한다.
    fn ipc_dispatch_events_fetch(&mut self, cmd: &IpcCommand) {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let Some(mgr) = self.plugin_manager.as_ref() else {
            send_response(&cmd.response_tx, crate::ipc::handler::events::no_bus(id));
            return;
        };
        let bus = mgr.event_bus.clone();
        let args = match crate::ipc::handler::events::FetchParams::parse(&cmd.request.params, &id) {
            Ok(a) => a,
            Err(resp) => {
                send_response(&cmd.response_tx, resp);
                return;
            }
        };
        if args.wait.is_zero() {
            send_response(
                &cmd.response_tx,
                crate::ipc::handler::events::fetch(&bus, &args, id),
            );
            return;
        }
        let response_tx = cmd.response_tx.clone();
        std::thread::spawn(move || {
            let resp = crate::ipc::handler::events::fetch(&bus, &args, id);
            send_response(&response_tx, resp);
        });
    }

    /// 요청한 작업을 가진 engine의 허브를 고르고 대기는 공용 워커 함수에 맡긴다.
    fn ipc_dispatch_task_await(&mut self, cmd: &IpcCommand) {
        let hub_opt = self
            .view
            .views
            .values()
            .find_map(|w| w.as_main().map(|w| w.core_state.task_waker_hub.clone()))
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|(_, e)| e.task_waker_hub.clone())
            })
            .or_else(|| self.core_state.as_ref().map(|e| e.task_waker_hub.clone()));
        let seq_opt = self
            .view
            .views
            .values()
            .find_map(|w| w.as_main().map(|w| w.core_state.agent_seq.clone()))
            .or_else(|| self.parked_states.first().map(|(_, e)| e.agent_seq.clone()))
            .or_else(|| self.core_state.as_ref().map(|e| e.agent_seq.clone()));
        let memory = self.core.memory_arc();
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        match (hub_opt, seq_opt) {
            (Some(hub), Some(seq)) => crate::ipc::handler::agent::task::spawn_task_await(
                hub,
                memory,
                seq,
                rpc_id,
                cmd.request.params.clone(),
                &cmd.response_tx,
            ),
            _ => send_response(
                &cmd.response_tx,
                crate::core::app_surface::no_application_state(rpc_id),
            ),
        }
    }
    /// 창 생성 때 공유한 approval_store를 고르고 대기는 공용 함수에 맡긴다.
    fn ipc_dispatch_approval_await(&mut self, cmd: &IpcCommand) {
        let store_opt = self
            .view
            .views
            .values()
            .find_map(|w| w.as_main().map(|w| w.core_state.approval_store.clone()))
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|(_, e)| e.approval_store.clone())
            })
            .or_else(|| self.core_state.as_ref().map(|e| e.approval_store.clone()));
        let memory = self.core.memory_arc();
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        match store_opt {
            Some(store) => crate::ipc::handler::approval::spawn_approval_await(
                store,
                memory,
                rpc_id,
                cmd.request.params.clone(),
                &cmd.response_tx,
            ),
            None => send_response(
                &cmd.response_tx,
                crate::core::app_surface::no_application_state(rpc_id),
            ),
        }
    }
}

/// 명시한 창 ID는 모달·preset도 허용한다. ID가 없으면 MainView 하나만 자동 선택한다.
/// 포커스나 열린 모달을 대체 대상으로 쓰지 않는다. 반환값은 입력 목록의 인덱스다.
/// 캡처 허용 범위가 window.list·window.close의 범위를 넓히는 것은 아니다.
/// docs/adr/0018-explicit-capture-and-fullscreen-stage.md 참조.
fn resolve_screenshot_window(
    windows: &[(u64, bool)],
    requested: Option<u64>,
) -> Result<usize, (i32, String)> {
    match requested {
        Some(wid) => windows
            .iter()
            .position(|(id, _)| *id == wid)
            .ok_or_else(|| (-32602, format!("Window id {wid} not found"))),
        None => {
            let mains: Vec<usize> = windows
                .iter()
                .enumerate()
                .filter(|(_, (_, is_main))| *is_main)
                .map(|(i, _)| i)
                .collect();
            match mains.len() {
                1 => Ok(mains[0]),
                0 => Err((-32000, "No main window open; specify 'window_id'".to_string())),
                _ => Err((
                    -32000,
                    "Multiple windows open; specify 'window_id' (focus-independent). Use 'window.list' to enumerate.".to_string(),
                )),
            }
        }
    }
}

#[cfg(test)]
mod screenshot_target_tests {
    use super::resolve_screenshot_window;

    /// MainView 두 개와 모달 두 개.
    const MIXED: &[(u64, bool)] = &[(10, true), (20, false), (11, true), (21, false)];
    /// MainView 한 개와 모달 한 개.
    const ONE_MAIN: &[(u64, bool)] = &[(10, true), (20, false)];

    #[test]
    fn an_explicit_id_can_name_a_modal_window() {
        assert_eq!(resolve_screenshot_window(MIXED, Some(20)), Ok(1));
        assert_eq!(resolve_screenshot_window(MIXED, Some(21)), Ok(3));
        assert_eq!(resolve_screenshot_window(ONE_MAIN, Some(20)), Ok(1));
    }

    #[test]
    fn an_explicit_id_still_names_a_main_window() {
        assert_eq!(resolve_screenshot_window(MIXED, Some(10)), Ok(0));
        assert_eq!(resolve_screenshot_window(MIXED, Some(11)), Ok(2));
    }

    #[test]
    fn an_unknown_id_is_a_parameter_error_naming_the_id() {
        let (code, msg) = resolve_screenshot_window(MIXED, Some(999)).unwrap_err();
        assert_eq!(code, -32602);
        assert!(
            msg.contains("999"),
            "메시지가 문제의 id 를 담아야 한다: {msg}"
        );
    }

    #[test]
    fn without_an_id_a_lone_main_window_is_picked_and_the_modal_is_not() {
        assert_eq!(resolve_screenshot_window(ONE_MAIN, None), Ok(0));
    }

    #[test]
    fn without_an_id_a_modal_is_never_the_automatic_target() {
        let only_modals: &[(u64, bool)] = &[(20, false), (21, false)];
        let (code, _) = resolve_screenshot_window(only_modals, None).unwrap_err();
        assert_eq!(code, -32000);
    }

    #[test]
    fn without_an_id_multiple_mains_is_an_error_not_a_focus_fallback() {
        let (code, msg) = resolve_screenshot_window(MIXED, None).unwrap_err();
        assert_eq!(code, -32000);
        assert!(
            msg.contains("window_id"),
            "무엇을 하라는 것인지 메시지가 말해야 한다: {msg}"
        );
    }

    #[test]
    fn no_window_at_all_is_an_error() {
        let (code, _) = resolve_screenshot_window(&[], None).unwrap_err();
        assert_eq!(code, -32000);
        let (code, _) = resolve_screenshot_window(&[], Some(10)).unwrap_err();
        assert_eq!(code, -32602);
    }
}
