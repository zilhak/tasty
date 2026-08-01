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
        if cmd.request.method == "debug.lua.eval" {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let source = cmd.request.params.get("source").and_then(|v| v.as_str());
            let response = match (source, self.lua_engine.as_ref()) {
                (Some(src), Some(engine)) => {
                    // fire-and-forget: 워커/deadline(07) 격리 하에서 실행. 부수효과는 로그로 관측.
                    engine.run_script(src, Some("debug.lua.eval"));
                    host_ipc::protocol::JsonRpcResponse::success(
                        id,
                        serde_json::json!({ "scheduled": true }),
                    )
                }
                (None, _) => {
                    host_ipc::protocol::JsonRpcResponse::invalid_params(id, "Missing 'source'")
                }
                (_, None) => host_ipc::protocol::JsonRpcResponse::error(
                    id,
                    -32603,
                    "lua engine not initialized",
                ),
            };
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
        // plugin egui-mesh banner(A3) 강제 open/close — 사용자 조작 재현이라 debug 전용.
        // `{ plugin_id?, banner_id, surface_id }` / `{ instance_id }`. host manager +
        // 소유 view 의 BannerManager 를 함께 다뤄야 해 App-level glue 를 호출한다.
        #[cfg(feature = "gui")]
        if cmd.request.method.starts_with("debug.plugin_banner.") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let p = &cmd.request.params;
            let response = match cmd.request.method.as_str() {
                "debug.plugin_banner.open" => {
                    match (
                        p.get("banner_id").and_then(|v| v.as_str()),
                        p.get("surface_id").and_then(|v| v.as_u64()),
                    ) {
                        (Some(bid), Some(sid)) => {
                            let bid = bid.to_string();
                            // debug 트리거는 소유권 검증 우회(caller=None) — 실 소유 plugin 으로 연다.
                            match self.open_plugin_banner(None, &bid, sid as u32) {
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
                "debug.plugin_banner.close" => {
                    match p.get("instance_id").and_then(|v| v.as_u64()) {
                        Some(iid) => {
                            let closed = self.close_plugin_banner(
                                iid,
                                tasty_plugin_protocol::BannerCloseReason::PluginRequest,
                            );
                            host_ipc::protocol::JsonRpcResponse::success(
                                id,
                                serde_json::json!({ "closed": closed }),
                            )
                        }
                        None => host_ipc::protocol::JsonRpcResponse::invalid_params(
                            id,
                            "Missing 'instance_id'",
                        ),
                    }
                }
                other => host_ipc::protocol::JsonRpcResponse::method_not_found(id, other),
            };
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }
}
