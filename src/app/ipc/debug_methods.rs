//! step 3 (debug 빌드 only): debug.event_bus.* / debug.extension.invoke_hook / debug.popup.* /
//! debug.fullscreen.*.

use crate::adapters::ipc::handler::params;
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
        // 임의 Lua 주입 (debug 전용, ADR-0031) — App 소유 lua_engine 워커로 실행.
        // release 에는 이 경로가 없다(identity 원칙 1: release 는 사용자 키 입력에서만 실행).
        // 임의 Lua 주입 (debug 전용, ADR-0031) — App 소유 lua_engine 워커로 실행.
        // release 에는 이 경로가 없다(identity 원칙 1: release 는 사용자 키 입력에서만 실행).
        // 본체는 헤드리스 pump 와 **같은 함수**를 쓴다 — 두 벌로 두면 갈라진다.
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
                // close 만 App-level glue 를 거친다 — 매니저를 직접 치면 렌더가
                // 수집하는 close 큐를 건너뛰어 `cancel_child_file_picker` 연쇄
                // 정리가 안 돈다. debug 강제 close 가 release 경로(plugin 의
                // `popup.close`)와 다른 코드를 타면 이 표면으로 하는 검증 자체가
                // 실제 동작을 못 비춘다.
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
        // plugin egui-mesh banner(A3) 강제 open/close — 사용자 조작 재현이라 debug 전용.
        // `{ plugin_id?, banner_id, surface_id }` / `{ instance_id }`. host manager +
        // 소유 view 의 BannerManager 를 함께 다뤄야 해 App-level glue 를 호출한다.
        #[cfg(feature = "gui")]
        if cmd.request.method.starts_with("debug.plugin_banner.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let params = &cmd.request.params;
            let response = match cmd.request.method.as_str() {
                "debug.plugin_banner.open" => {
                    // surface id 는 관문에서 **폭에 맞게** 읽는다 — `as u32` 로 자르면
                    // 범위 밖 값이 실재하는 다른 surface 를 가리킨다. 잘못 온 값은
                    // 안 온 것과 갈라서 답한다.
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
                            // debug 트리거는 소유권 검증 우회(caller=None) — 실 소유 plugin 으로 연다.
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
        // 전체화면 무대 강제 진입/종료/조회 — 사용자 조작(popup 타이틀바의 전체화면
        // 버튼) 재현이라 debug 전용.
        //
        // `route_debug_handler` 가 아니라 여기 있는 이유: 그 라우터는 `AppState`
        // (= `MainView` 하나)만 받아 **다른 창을 볼 수 없다.** 무대는 창 단위라
        // (`docs/design/systems/fullscreen-stage.md`) `window_id` 로 창을 지목하지
        // 못하면 창 2 개에 각각 무대를 띄우는 시나리오 자체를 구동할 수 없다.
        // `self.view.views` 순회는 App 레벨에서만 가능하다.
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

/// `debug.fullscreen.*` — 전체화면 무대의 에이전트 진입/조회 표면.
///
/// 무대는 창 하나에 하나(`docs/design/systems/fullscreen-stage.md`)라 모든 메서드가
/// 창을 대상으로 한다. `window_id` 는 지정하면 그 창, 미지정이고 main 창이 하나면 그
/// 창, 여럿이면 에러다. 포커스된 창으로 조용히 폴백하지 않는다
/// (`docs/design/policies/focus.md`). 대상은 항상 main 창이다 — 상세는
/// [`App::pick_debug_window`].
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
        // 창을 고르기 **전에** 무대 id 를 검증한다. 모르는 id 를 조용한 no-op 으로
        // 흘리면 오타가 "열렸는데 안 보인다" 로 보인다.
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
        // App 레벨 경로는 라우팅이 세워주는 dirty 가 없다. 무대 진입은 OS 창 전환
        // (`sync_window_fullscreen`)까지 프레임 안에서 일어나므로 직접 유도한다.
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

    /// `window_id` 파라미터를 실제 창으로 해석한다.
    ///
    /// 실패 시 `(JSON-RPC code, message)`. 미지정 + 창 여럿은 에러이지 폴백이 아니다 —
    /// "지금 보고 있는 창" 이라는 개념에 기대면 에이전트 명령이 사용자 포커스에
    /// 의존하게 된다.
    ///
    /// `ui.screenshot` 과 달리 **명시한 id 도 main 창만** 받는다. 무대는 main 창에만
    /// 존재하므로(`docs/design/systems/fullscreen-stage.md`) 모달 id 를 받을 자리가
    /// 없다 — 캡처(읽기)와 달리 이건 무대를 여닫는 행동이라, 대상 집합을 넓히면
    /// 사용자 조작 영역인 모달까지 에이전트 행동 대상이 된다
    /// (`docs/adr/0118-screenshot-reads-any-window-explicit-id-only.md`).
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
