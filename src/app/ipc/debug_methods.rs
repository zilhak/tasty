//! debug 빌드의 App 단위 조작·조회 요청을 처리한다.

use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::handler::debug_plugin;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(crate) fn ipc_step_debug(&mut self, cmd: &IpcCommand) -> IpcStep {
        // 설정 창 열기는 App 이벤트로 처리하며 예약 접수만 즉시 응답한다.
        #[cfg(feature = "gui")]
        if cmd.request.method == "debug.settings.open" {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let tab = cmd
                .request
                .params
                .get("tab")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let subtab = cmd
                .request
                .params
                .get("subtab")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            self.pending_settings_tab = tab.clone();
            self.pending_settings_subtab = subtab.clone();
            crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                id,
                serde_json::json!({ "scheduled": true, "tab": tab, "subtab": subtab }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // 설정에 한정하지 않고 현재 활성 모달을 닫는다.
        #[cfg(feature = "gui")]
        if cmd.request.method == "debug.modal.close_request" {
            return self.ipc_handle_debug_modal_close_request(cmd);
        }
        // 임의 Lua 실행은 debug 전용이며 헤드리스와 같은 함수를 사용한다.
        if cmd.request.method == "debug.lua.eval" {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = crate::core::app_surface_debug::lua_eval(
                self.lua_engine.as_ref(),
                id,
                &cmd.request.params,
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
                // 공통 닫기 큐를 거쳐 자식 파일 피커도 정리한다.
                "debug.popup.close" => {
                    match params::read_int::<u64>(&cmd.request.params, "instance_id") {
                        Err(msg) => host_ipc::protocol::JsonRpcResponse::invalid_params(id, &msg),
                        Ok(None) => host_ipc::protocol::JsonRpcResponse::invalid_params(
                            id,
                            "Missing required 'instance_id' parameter",
                        ),
                        Ok(Some(instance_id)) if self.plugin_manager.is_none() => {
                            host_ipc::protocol::JsonRpcResponse::error(
                                id,
                                -32002,
                                format!("plugin manager not initialized (instance {instance_id})"),
                            )
                        }
                        Ok(Some(instance_id)) => {
                            self.enqueue_plugin_popup_close(
                                instance_id,
                                tasty_plugin_protocol::PopupCloseReason::PluginRequest,
                            );
                            host_ipc::protocol::JsonRpcResponse::success(
                                id,
                                serde_json::json!({ "closed": instance_id }),
                            )
                        }
                    }
                }
                other => host_ipc::protocol::JsonRpcResponse::method_not_found(id, other),
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // 배너는 매니저와 소유 창 상태를 함께 갱신한다.
        #[cfg(feature = "gui")]
        if cmd.request.method.starts_with("debug.plugin_banner.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let params = &cmd.request.params;
            let response = match cmd.request.method.as_str() {
                "debug.plugin_banner.open" => {
                    // 범위 밖 값을 잘라 다른 surface ID로 바꾸지 않는다.
                    let sid = match params::read_u32(params, "surface_id") {
                        Ok(v) => v,
                        Err(msg) => {
                            send_response(
                                &cmd.response_tx,
                                host_ipc::protocol::JsonRpcResponse::invalid_params(id, &msg),
                            );
                            return IpcStep::Handled;
                        }
                    };
                    match (params.get("banner_id").and_then(|v| v.as_str()), sid) {
                        (Some(bid), Some(sid)) => {
                            let bid = bid.to_string();
                            // debug 요청은 소유자 검사를 생략하되 실제 소유 플러그인으로 연다.
                            match self.open_plugin_banner(None, &bid, sid) {
                                Ok(iid) => host_ipc::protocol::JsonRpcResponse::success(
                                    id,
                                    serde_json::json!({ "instance_id": iid }),
                                ),
                                Err(e) => host_ipc::protocol::JsonRpcResponse::error(id, -32602, e),
                            }
                        }
                        _ => host_ipc::protocol::JsonRpcResponse::invalid_params(
                            id,
                            "Missing 'banner_id' or 'surface_id'",
                        ),
                    }
                }
                "debug.plugin_banner.close" => match params::read_int::<u64>(params, "instance_id")
                {
                    Err(msg) => host_ipc::protocol::JsonRpcResponse::invalid_params(id, &msg),
                    Ok(Some(iid)) => {
                        let closed = self.close_plugin_banner(
                            iid,
                            tasty_plugin_protocol::BannerCloseReason::PluginRequest,
                        );
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({ "closed": closed }),
                        )
                    }
                    Ok(None) => host_ipc::protocol::JsonRpcResponse::invalid_params(
                        id,
                        "Missing 'instance_id'",
                    ),
                },
                other => host_ipc::protocol::JsonRpcResponse::method_not_found(id, other),
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // 전체화면 무대는 창별 상태라 모든 창을 볼 수 있는 App에서 처리한다.
        #[cfg(feature = "gui")]
        if cmd.request.method.starts_with("debug.fullscreen.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = self.ipc_debug_fullscreen(&cmd.request.method, &cmd.request.params, id);
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }
}

/// 전체화면 무대는 MainView만 대상으로 한다. ID가 없으면 MainView 하나일 때만 선택한다.
#[cfg(feature = "gui")]
impl App {
    fn ipc_debug_fullscreen(
        &mut self,
        method: &str,
        params: &serde_json::Value,
        id: serde_json::Value,
    ) -> host_ipc::protocol::JsonRpcResponse {
        match method {
            "debug.fullscreen.list" => crate::core::app_surface_debug::fullscreen_list(id),
            "debug.fullscreen.open" => self.debug_fullscreen_open(params, id),
            "debug.fullscreen.close" => self.debug_fullscreen_close(params, id),
            "debug.fullscreen.state" => self.debug_fullscreen_state(params, id),
            other => host_ipc::protocol::JsonRpcResponse::method_not_found(id, other),
        }
    }

    fn debug_fullscreen_open(
        &mut self,
        params: &serde_json::Value,
        id: serde_json::Value,
    ) -> host_ipc::protocol::JsonRpcResponse {
        let Some(stage_id) = params.get("stage_id").and_then(|v| v.as_str()) else {
            return host_ipc::protocol::JsonRpcResponse::invalid_params(id, "Missing 'stage_id'");
        };
        // 잘못된 무대 ID를 열기 성공처럼 처리하지 않는다.
        if crate::fullscreen_stages::find(stage_id).is_none() {
            let known: Vec<&str> = crate::fullscreen_stages::all_metas()
                .iter()
                .map(|m| m.id)
                .collect();
            return host_ipc::protocol::JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Unknown stage id '{stage_id}'. Known: {}. Use 'debug.fullscreen.list'.",
                    known.join(", ")
                ),
            );
        }
        let target = match self.pick_debug_window(params) {
            Ok(t) => t,
            Err((code, msg)) => {
                return host_ipc::protocol::JsonRpcResponse::error(id, code, msg);
            }
        };
        let Some(main) = self
            .view
            .views
            .get_mut(&target)
            .and_then(|w| w.as_main_mut())
        else {
            return host_ipc::protocol::JsonRpcResponse::error(
                id,
                -32602,
                "Target window is not a main view",
            );
        };
        let previous = main.state.fullscreen_stage_id();
        if !main.state.open_fullscreen_stage(stage_id) {
            return host_ipc::protocol::JsonRpcResponse::error(
                id,
                -32603,
                format!("Failed to open stage '{stage_id}'"),
            );
        }
        let opened = main.state.fullscreen_stage_id();
        // 일반 라우터를 거치지 않아 무대와 OS 전체화면 동기화에 필요한 repaint를 직접 요청한다.
        main.base.dirty = true;
        main.base.winit.request_redraw();
        host_ipc::protocol::JsonRpcResponse::success(
            id,
            serde_json::json!({
                "window_id": u64::from(target),
                "stage_id": opened,
                "previous_stage_id": previous,
                "replaced": previous.is_some_and(|p| Some(p) != opened),
            }),
        )
    }

    /// OS 창 관리자 없이도 활성 모달 닫기를 재현하도록 close_active_modal을 호출한다.
    /// SettingsView의 CloseRequested 이벤트 자체를 주입하는 경로는 아니다.
    #[cfg(feature = "gui")]
    fn ipc_handle_debug_modal_close_request(&mut self, cmd: &IpcCommand) -> IpcStep {
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let was_open = self.view.active_modal_id.is_some();
        if was_open {
            self.close_active_modal();
        }
        send_response(
            &cmd.response_tx,
            host_ipc::protocol::JsonRpcResponse::success(
                response_id,
                serde_json::json!({"closed": was_open}),
            ),
        );
        IpcStep::Handled
    }

    fn debug_fullscreen_close(
        &mut self,
        params: &serde_json::Value,
        id: serde_json::Value,
    ) -> host_ipc::protocol::JsonRpcResponse {
        let target = match self.pick_debug_window(params) {
            Ok(t) => t,
            Err((code, msg)) => {
                return host_ipc::protocol::JsonRpcResponse::error(id, code, msg);
            }
        };
        let Some(main) = self
            .view
            .views
            .get_mut(&target)
            .and_then(|w| w.as_main_mut())
        else {
            return host_ipc::protocol::JsonRpcResponse::error(
                id,
                -32602,
                "Target window is not a main view",
            );
        };
        let previous = main.state.fullscreen_stage_id();
        let closed = main.state.close_fullscreen_stage();
        main.base.dirty = true;
        main.base.winit.request_redraw();
        host_ipc::protocol::JsonRpcResponse::success(
            id,
            serde_json::json!({
                "window_id": u64::from(target),
                "closed": closed,
                "stage_id": previous,
            }),
        )
    }

    fn debug_fullscreen_state(
        &mut self,
        params: &serde_json::Value,
        id: serde_json::Value,
    ) -> host_ipc::protocol::JsonRpcResponse {
        let target = match self.pick_debug_window(params) {
            Ok(t) => t,
            Err((code, msg)) => {
                return host_ipc::protocol::JsonRpcResponse::error(id, code, msg);
            }
        };
        let Some(main) = self.view.views.get(&target).and_then(|w| w.as_main()) else {
            return host_ipc::protocol::JsonRpcResponse::error(
                id,
                -32602,
                "Target window is not a main view",
            );
        };
        let report = main.fullscreen_window_report();
        host_ipc::protocol::JsonRpcResponse::success(
            id,
            serde_json::json!({
                "window_id": u64::from(target),
                "stage_id": main.state.fullscreen_stage_id(),
                "stage_active": report.stage_active,
                "os_fullscreen": report.os_fullscreen,
                "maximized": report.maximized,
                "inner_size": { "width": report.inner_size.0, "height": report.inner_size.1 },
                "monitor": report.monitor.map(|m| serde_json::json!({
                    "name": m.name,
                    "position": { "x": m.position.0, "y": m.position.1 },
                    "size": { "width": m.size.0, "height": m.size.1 },
                    "scale_factor": m.scale_factor,
                })),
            }),
        )
    }

    /// MainView ID를 해석한다. 캡처와 달리 모달·preset은 대상이 아니다.
    /// ID가 없을 때 포커스를 쓰지 않고 MainView가 하나인 경우만 선택한다.
    fn pick_debug_window(
        &self,
        params: &serde_json::Value,
    ) -> Result<winit::window::WindowId, (i32, String)> {
        let requested =
            params::read_int::<u64>(params, "window_id").map_err(|msg| (-32602, msg))?;
        let mains: Vec<_> = self
            .view
            .views
            .iter()
            .filter(|(_, w)| w.as_main().is_some())
            .map(|(id, _)| *id)
            .collect();
        match requested {
            Some(wid) => mains
                .iter()
                .copied()
                .find(|w| u64::from(*w) == wid)
                .ok_or_else(|| (-32602, format!("Window id {wid} not found"))),
            None if mains.len() == 1 => Ok(mains[0]),
            None if mains.is_empty() => Err((-32000, "No main window open".to_string())),
            None => Err((
                -32000,
                "Multiple windows open; specify 'window_id' (focus-independent). Use 'window.list' to enumerate.".to_string(),
            )),
        }
    }
}
