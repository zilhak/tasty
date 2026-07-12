//! step 2: 호스트 자체 메서드.
//!
//! - `system.shutdown` (debug)
//! - `system.gpu_stats` (read-only GPU 리소스 카운트 — 메모리 누수 soak 검증)
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
        if cmd.request.method == "system.gpu_stats" {
            return self.ipc_handle_system_gpu_stats(cmd);
        }
        if cmd.request.method == "window.create" || cmd.request.method == "view.create" {
            crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::CreateWindow);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"scheduled": true}),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method == "window.close" || cmd.request.method == "view.close" {
            return self.ipc_handle_window_close(cmd);
        }
        // window.focus 는 사용자 입력 재현 (단축키/마우스 클릭 영역) 으로 분류.
        // CLAUDE.md: "CLI/IPC로 포커스·활성 탭·활성 워크스페이스를 전환하는 명령은
        // 존재하지 않는다." → release 빌드에 노출 안 함. debug 빌드만 유지.
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
        if cmd.request.method.starts_with("plugin.") {
            return self.ipc_dispatch_plugin_method(cmd, caller);
        }
        if cmd.request.method == "approval.await" {
            self.ipc_dispatch_approval_await(cmd);
            return IpcStep::Handled;
        }
        if cmd.request.method == "agent.task_await" {
            self.ipc_dispatch_task_await(cmd);
            return IpcStep::Handled;
        }
        // remote.workspaces / remote.attach — 원격 워크스페이스 브라우징·attach 능력의
        // 로컬 IPC 노출(원칙 2: 에이전트가 CLI 없이 소켓만으로도 수행 가능). 블로킹 SSH
        // I/O 는 워커 스레드로 돌려 이벤트루프를 막지 않는다. CLI(`remote workspaces`)와
        // 동일한 `tasty_cli::remote_browse` 코어를 공유한다.
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

    /// `system.gpu_stats` — GPU 리소스 카운트 read-only 조회 (메모리 누수 soak 검증용).
    ///
    /// 반환: wgpu 전역 리포트(`Instance::generate_report()` — 모든 창 합산 buffers/
    /// textures/texture_views/bind_groups … 의 live 카운트) + 창별 `GpuState` 카운트
    /// (egui-mesh target 맵 3종 len, atlas, draw calls). soak 하네스가 "surface 를
    /// 닫았는데 카운트가 기준선으로 복귀하지 않음" 유형의 누수를 판정하는 1차 소스.
    ///
    /// 순수 조회 — 사용자 상태(focus/선택/스크롤)에 닿지 않는다(원칙 1). IPC+CLI
    /// 양면 노출(원칙 2: `tasty list gpu-stats`), 대상 지정 불필요한 전역 스냅샷이라
    /// 포커스 독립(원칙 3). local_only — plugin 미노출.
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
                })
            })
            .collect();
        // RegistryReport 를 JSON 으로 — 타입 경로(wgpu-core 재수출)에 의존하지 않도록
        // 필드 접근만 하는 macro 로 변환한다. `num_allocated` 가 live 카운트.
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

    /// `window.close` / `view.close`: main view 만 대상, 마지막 main 은 거부.
    /// CLAUDE.md "포커스 독립": id 로 직접 지정, focused 의존 금지.
    fn ipc_handle_window_close(&mut self, cmd: &IpcCommand) -> IpcStep {
        let target_id = cmd.request.params.get("id").and_then(|v| v.as_u64());
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = match target_id {
            None => host_ipc::protocol::JsonRpcResponse::error(
                response_id,
                -32602,
                "Missing 'id' parameter (u64). focused 의존은 금지.",
            ),
            Some(id_u64) => {
                // main view 만 대상 — `window.list` 가 노출하는 범위와 동일.
                // (modal/preset 은 사용자 조작 영역이라 IPC close 대상이 아님.)
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
                        // GUI request_close_window 와 공통 helper — window.closed
                        // plugin event + window.delete.post Lua fire 포함.
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

    /// `window.focus` / `view.focus` (debug 전용): 사용자 입력 재현이라 release 미노출.
    #[cfg(debug_assertions)]
    fn ipc_handle_window_focus(&mut self, cmd: &IpcCommand) -> IpcStep {
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

    /// `window.list` / `view.list`: 전 view 순회, main view 만 노출.
    fn ipc_handle_window_list(&self, cmd: &IpcCommand) -> IpcStep {
        let focused_id = self.view.focused_view_id;
        let list: Vec<_> = self
            .view
            .views
            .iter()
            .filter_map(|(id, w)| {
                let main = w.as_main()?;
                Some(serde_json::json!({
                    "id": u64::from(*id),
                    "focused": focused_id == Some(*id),
                    "title": main.state.active_workspace(&main.core_state).name,
                }))
            })
            .collect();
        let response = host_ipc::protocol::JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            serde_json::json!(list),
        );
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// `ui.screenshot` — 정식 release 기능. focus 독립(원칙 3): 대상 window/surface 를
    /// ID 로 직접 지정하고 focused_view_id 에 의존하지 않는다. 에이전트가 자기 작업을
    /// 관찰하는 캡처라 사용자 상태(focus/가시 탭)를 건드리지 않는다(원칙 1·2). local_only
    /// (파일 쓰기 표면) — plugin 미노출.
    ///
    /// params: { path (필수), surface_id? (u32), window_id? (u64) }
    /// - surface_id → 해당 터미널 surface 를 오프스크린 렌더로 캡처(가시성/포커스 무관).
    /// - 아니면 window 프레임 캡처: window_id 지정, 없으면 유일 window, 다중이면 에러.
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
        let surface_id = cmd
            .request
            .params
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let window_id = cmd.request.params.get("window_id").and_then(|v| v.as_u64());

        // ── surface 지정 오프스크린 캡처 (focus 독립) ──
        if let Some(sid) = surface_id {
            return self.ipc_screenshot_surface(cmd, response_id, &path, sid);
        }

        // ── window(tasty 자체 화면) 캡처 (focus 독립) ──
        let mains: Vec<_> = self
            .view
            .views
            .iter()
            .filter(|(_, w)| w.as_main().is_some())
            .map(|(id, _)| *id)
            .collect();
        let target = match window_id {
            Some(wid) => mains.iter().copied().find(|w| u64::from(*w) == wid),
            None if mains.len() == 1 => mains.first().copied(),
            None => None,
        };
        let response = match target {
            Some(tid) => match self.view.views.get_mut(&tid).and_then(|w| w.as_main_mut()) {
                Some(m) => {
                    m.base.gpu.pending_screenshot = Some(std::path::PathBuf::from(&path));
                    m.base.dirty = true;
                    m.base.winit.request_redraw();
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
                    -32602,
                    "Target window is not a main view",
                ),
            },
            None => match window_id {
                Some(wid) => host_ipc::protocol::JsonRpcResponse::error(
                    response_id,
                    -32602,
                    format!("Window id {wid} not found"),
                ),
                None => host_ipc::protocol::JsonRpcResponse::error(
                    response_id,
                    -32000,
                    "Multiple windows open; specify 'window_id' (focus-independent). Use 'window.list' to enumerate.",
                ),
            },
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// `ui.screenshot` 의 surface 지정 오프스크린 캡처 경로. 소유 window 를 ID 로 순회
    /// 해소(focus 무관)하고 terminal surface 만 캡처 스케줄한다.
    fn ipc_screenshot_surface(
        &mut self,
        cmd: &IpcCommand,
        response_id: serde_json::Value,
        path: &str,
        sid: u32,
    ) -> IpcStep {
        // 소유 window(창별 CoreState)를 ID 로 순회 해소 — focus 무관.
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
                        format!("enable failed: {e}"),
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
                        format!("disable failed: {e}"),
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
            "plugin.list_agent_permissions" => {
                host_ipc::handler::session::handle_list_agent_permissions(
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

    /// `agent.task_await`: blocking. Arc<TaskWakerHub> + memory port arc + agent_seq
    /// 를 worker thread 로 클론. await_task_blocking 이 현 state snapshot → hub
    /// recv_timeout. J.A.S5.
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
            (Some(hub), Some(seq)) => {
                let params = cmd.request.params.clone();
                let response_tx = cmd.response_tx.clone();
                std::thread::spawn(move || {
                    let resp = crate::adapters::ipc::handler::agent::task::await_task_blocking(
                        &hub, &memory, seq, rpc_id, &params,
                    );
                    send_response(&response_tx, resp);
                });
            }
            _ => {
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

    /// `approval.await`: blocking. Arc<ApprovalStore> + memory port arc 를
    /// worker thread 로 클론해 cascade 없이 자기 수명에서 영속한다.
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

    /// `remote.workspaces` { profile? , ssh? , remote_tasty? , remote_port_mode? } →
    /// 원격 tasty 의 워크스페이스 목록(browse). 블로킹 SSH I/O 라 **워커 스레드**에서
    /// 조회하고 완료 시 `response_tx` 로 지연 회신한다(이벤트루프 무블록).
    ///
    /// 순수 조회 — 로컬 사용자 상태(focus/닫은항목 히스토리/선택)에 닿지 않는다(원칙 1).
    /// CLI `tasty remote workspaces` 와 `tasty_cli::remote_browse::browse` 를 공유한다.
    fn ipc_dispatch_remote_workspaces(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let params = &cmd.request.params;
        let profile = params
            .get("profile")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ssh = params
            .get("ssh")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let remote_tasty = params
            .get("remote_tasty")
            .and_then(|v| v.as_str())
            .unwrap_or("tasty")
            .to_string();
        let remote_port_mode = params
            .get("remote_port_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();
        if profile.is_some() && ssh.is_some() {
            send_response(
                &cmd.response_tx,
                host_ipc::protocol::JsonRpcResponse::invalid_params(
                    rpc_id,
                    "'profile' and 'ssh' are mutually exclusive",
                ),
            );
            return;
        }
        if profile.is_none() && ssh.is_none() {
            send_response(
                &cmd.response_tx,
                host_ipc::protocol::JsonRpcResponse::invalid_params(
                    rpc_id,
                    "one of 'profile' or 'ssh' is required",
                ),
            );
            return;
        }
        let response_tx = cmd.response_tx.clone();
        std::thread::spawn(move || {
            let resp = match tasty_cli::remote_browse::resolve_connection_spec(
                profile.as_deref(),
                ssh.as_deref(),
                &remote_tasty,
                &remote_port_mode,
            ) {
                Ok((target, rt, pm, pf)) => {
                    match tasty_cli::remote_browse::browse(&target, &rt, &pm, pf.as_deref()) {
                        Ok(list) => host_ipc::protocol::JsonRpcResponse::success(
                            rpc_id,
                            serde_json::to_value(list).unwrap_or(serde_json::Value::Null),
                        ),
                        Err(e) => host_ipc::protocol::JsonRpcResponse::error(
                            rpc_id,
                            -32050,
                            format!("remote browse failed: {e}"),
                        ),
                    }
                }
                Err(e) => {
                    host_ipc::protocol::JsonRpcResponse::invalid_params(rpc_id, e.to_string())
                }
            };
            send_response(&response_tx, resp);
        });
    }

    /// `remote.attach` { remote_workspace , profile? , ssh? , remote_tasty? ,
    /// remote_port_mode? } → 선택한 원격 워크스페이스를 **로컬 mirror 로 attach**.
    ///
    /// **focus 중립(원칙 1 핵심)**: 이 IPC/에이전트 경로는 mirror workspace 를 *생성만*
    /// 하고 focus 를 그 ws 로 옮기지 않는다. mirror 생성 실체(`start_gui_attach`)는
    /// `engine.workspaces.push` 만 하고 `active_workspace` 를 건드리지 않는다(조용한 생성).
    /// 새 mirror 로의 focus 이동은 **사용자 입력 경로 전용 별도 단계**다(RA02 팝업에서
    /// 사용자가 확정할 때) — release IPC 에는 focus 변경 API 가 없다(원칙 3).
    ///
    /// 블로킹 SSH 터널 수립은 워커 스레드에서 하고(auto-attach 와 동일한 결과 채널 재사용,
    /// anchor=None), 회신은 즉시 `{attaching:true}`(fire-and-forget) — mirror 는 비동기로
    /// 나타난다. 호출자는 `list workspaces`(mirror 플래그)로 결과를 확인한다.
    fn ipc_dispatch_remote_attach(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let params = &cmd.request.params;
        let remote_ws = params.get("remote_workspace").and_then(|v| v.as_u64());
        let profile = params
            .get("profile")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ssh = params
            .get("ssh")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let remote_tasty = params
            .get("remote_tasty")
            .and_then(|v| v.as_str())
            .unwrap_or("tasty")
            .to_string();
        let remote_port_mode = params
            .get("remote_port_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();

        let Some(remote_ws) = remote_ws else {
            send_response(
                &cmd.response_tx,
                host_ipc::protocol::JsonRpcResponse::invalid_params(
                    rpc_id,
                    "Missing required 'remote_workspace' (u32)",
                ),
            );
            return;
        };
        let remote_ws = remote_ws as u32;
        if profile.is_some() && ssh.is_some() {
            send_response(
                &cmd.response_tx,
                host_ipc::protocol::JsonRpcResponse::invalid_params(
                    rpc_id,
                    "'profile' and 'ssh' are mutually exclusive",
                ),
            );
            return;
        }
        if profile.is_none() && ssh.is_none() {
            send_response(
                &cmd.response_tx,
                host_ipc::protocol::JsonRpcResponse::invalid_params(
                    rpc_id,
                    "one of 'profile' or 'ssh' is required",
                ),
            );
            return;
        }

        // 워커: 접속 스펙 resolve + 엔드포인트(SSH 터널/loopback) 해석(블로킹). 완료 시
        // auto-attach 결과 채널로 push → 메인 루프가 drain 해 focus 중립 start_gui_attach.
        let tx = self.auto_attach_tx.clone();
        let proxy = self.view.proxy.clone();
        std::thread::spawn(move || {
            let result = tasty_cli::remote_browse::resolve_connection_spec(
                profile.as_deref(),
                ssh.as_deref(),
                &remote_tasty,
                &remote_port_mode,
            )
            .and_then(|(target, rt, pm, pf)| {
                tasty_cli::remote_browse::resolve_endpoint(&target, &rt, &pm, pf.as_deref())
            });
            let outcome = crate::app::auto_attach::AutoAttachOutcome {
                anchor_ws_id: None,
                remote_ws,
                result,
            };
            let _ = tx.send(outcome); // 수신자(메인 루프) drop 시에만 실패 — 무시.
            let _ = proxy.send_event(AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
        });

        // 즉시 회신(fire-and-forget). mirror 는 비동기 생성, focus 는 이동하지 않음.
        send_response(
            &cmd.response_tx,
            host_ipc::protocol::JsonRpcResponse::success(
                rpc_id,
                serde_json::json!({ "attaching": true, "remote_workspace": remote_ws }),
            ),
        );
    }
}
