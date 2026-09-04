//! step 2: 호스트 자체 메서드.
//!
//! - `system.shutdown` (debug)
//! - `system.gpu_stats` (read-only GPU 리소스 카운트 — 메모리 누수 soak 검증)
//! - `timer.list` (read-only 타이머 허브 스냅샷 — 무엇이 인스턴스를 깨우는가)
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
        if cmd.request.method == "timer.list" {
            return self.ipc_handle_timer_list(cmd);
        }
        if cmd.request.method == "window.create" || cmd.request.method == "view.create" {
            crate::shortcuts::send_app_event(
                &self.view.proxy,
                AppEvent::CreateWindow(crate::app::event::WindowRequestOrigin::Agent),
            );
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
        if cmd.request.method == "agent.task_await" {
            self.ipc_dispatch_task_await(cmd);
            return IpcStep::Handled;
        }
        // remote.workspaces / remote.attach — 원격 워크스페이스 브라우징·attach 능력의
        // 로컬 IPC 노출(원칙 2: 에이전트가 CLI 없이 소켓만으로도 수행 가능). 블로킹 SSH
        // I/O 는 워커 스레드로 돌려 이벤트루프를 막지 않는다. CLI(`remote workspaces`)와
        // 동일한 `tasty_remote::browse` 코어를 공유한다.
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

    /// `timer.list` — 중앙 타이머 허브의 read-only 스냅샷.
    ///
    /// 일반 IPC 핸들러(`src/adapters/ipc/handler/`)가 아니라 여기 있는 이유: 허브는
    /// `App` 필드고 plugin manager 는 자기 허브를 따로 소유한다. `CoreState` 만 받는
    /// 핸들러에서는 둘 중 어느 쪽에도 닿지 못해 "무엇이 깨우고 있는가" 에 답할 수 없다.
    /// headless 는 같은 이유로 dispatch pump 에서 같은 함수를 부른다.
    ///
    /// 순수 조회 — 사용자 상태에 닿지 않고(원칙 1), 대상 지정이 필요 없는 전역
    /// 스냅샷이라 포커스 독립(원칙 3). local_only — plugin 미노출.
    fn ipc_handle_timer_list(&self, cmd: &IpcCommand) -> IpcStep {
        let response = host_ipc::protocol::JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            self.timer_list_json(std::time::Instant::now()),
        );
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
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
    ///
    /// `layout_slot` 은 이 창이 점유한 레이아웃 슬롯 번호다 — "새 창을 열면 어떤
    /// 레이아웃이 뜰지" 를 결정하는 상태라, 관측 수단이 없으면 에이전트가 창 생성
    /// 결과를 예측·검증할 수 없다. headless engine 은 슬롯을 잡지 않으므로 `null`.
    ///
    /// **parked engine 은 여기 섞지 않는다.** 파킹된 engine 도 슬롯을 점유하지만
    /// 창이 아니라 창 id 가 없고, `{id, focused, title}` 계약이 깨진다. 점유 현황
    /// 전체(파킹 포함)를 봐야 할 일이 생기면 `layout.slots` 같은 별도 조회
    /// 메서드로 분리한다.
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
                    "layout_slot": main.core_state.layout_slot,
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
    /// - 아니면 window 프레임 캡처: window_id 를 명시하면 **모달·preset 창을 포함한 모든
    ///   창**, 미지정이면 main 창이 정확히 하나일 때만, 그 외는 에러. 두 갈래가 비대칭인
    ///   근거는 [`resolve_screenshot_window`] 문서.
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
                    // `ids` 는 바로 위에서 같은 맵에서 뽑았으므로 도달하지 않는다.
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

    /// `clipboard.set_text` — 로컬 클립보드에 텍스트를 쓴다. `Permission::ClipboardWrite`
    /// 로 plugin 노출(원칙 2). remote mirror 캡처 결과를 원격 clipboard 에 반영하는
    /// attach 전송 경로(`attach_client.rs`)도 원격 tasty 인스턴스에서 이 메서드로 도착한다.
    ///
    /// params: { text (필수, string) }
    fn ipc_handle_clipboard_set_text(&mut self, cmd: &IpcCommand) -> IpcStep {
        let response_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let text = match cmd.request.params.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::error(
                        response_id,
                        -32602,
                        "Missing 'text' parameter (string)",
                    ),
                );
                return IpcStep::Handled;
            }
        };
        let response = match self.core.clipboard_arc().write_text(&text) {
            Ok(()) => host_ipc::protocol::JsonRpcResponse::success(
                response_id,
                serde_json::json!({"ok": true}),
            ),
            Err(e) => host_ipc::protocol::JsonRpcResponse::error(
                response_id,
                -32000,
                format!("Failed to write clipboard: {e}"),
            ),
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
    /// CLI `tasty remote workspaces` 와 `tasty_remote::browse::browse` 를 공유한다.
    fn ipc_dispatch_remote_workspaces(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let conn = match RemoteConnParams::parse(&cmd.request.params) {
            Ok(c) => c,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(rpc_id, msg),
                );
                return;
            }
        };
        let RemoteConnParams {
            profile,
            ssh,
            remote_tasty,
            remote_port_mode,
        } = conn;
        let response_tx = cmd.response_tx.clone();
        std::thread::spawn(move || {
            let resp = match tasty_remote::browse::resolve_connection_spec(
                profile.as_deref(),
                ssh.as_deref(),
                &remote_tasty,
                &remote_port_mode,
            ) {
                Ok((target, rt, pm, pf)) => {
                    match tasty_remote::browse::browse(&target, &rt, &pm, pf.as_deref()) {
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

    /// `remote.attach` { remote_workspace | new_workspace , profile? , ssh? ,
    /// remote_tasty? , remote_port_mode? , name? , cwd? } → 원격 워크스페이스를
    /// **로컬 mirror 로 attach**. `new_workspace: true` 면 원격에 워크스페이스를 **먼저
    /// 만들고** 그것을 attach 한다(생성+attach 복합 능력의 IPC 면 — CLI 면은
    /// `tasty remote new-workspace` + `tasty remote attach --workspace <id>`, 양쪽이
    /// `tasty_remote::create` 코어를 공유한다. 원칙 2).
    ///
    /// **focus 중립(원칙 1 핵심)**: 이 IPC/에이전트 경로는 mirror workspace 를 *생성만*
    /// 하고 focus 를 그 ws 로 옮기지 않는다. mirror 생성 실체(`start_gui_attach`)는
    /// `engine.workspaces.push` 만 하고 `active_workspace` 를 건드리지 않는다(조용한 생성).
    /// 새 mirror 로의 focus 이동은 **사용자 입력 경로 전용 별도 단계**다(RA02 팝업에서
    /// 사용자가 확정할 때) — release IPC 에는 focus 변경 API 가 없다(원칙 3).
    /// 원격측 active 도 바뀌지 않는다(`workspace.create` 는 Agent origin).
    ///
    /// 블로킹 SSH 터널 수립은 워커 스레드에서 한다(auto-attach 와 동일한 결과 채널 재사용,
    /// anchor=None).
    ///
    /// **회신 계약이 두 갈래인 이유**:
    /// - 기존 ws attach: 즉시 `{attaching:true}`(fire-and-forget). 호출자가 이미 대상 id
    ///   를 알고 있으므로 회신에 새 정보가 없고, mirror 는 비동기로 나타난다.
    /// - `new_workspace`: **생성 완료까지 기다렸다 지연 회신**해 `remote_workspace`(새 id)
    ///   를 돌려준다. 즉시 회신으로는 ① 호출자가 만들어진 id 를 알 길이 없고 ② 생성 실패
    ///   (예: 없는 `cwd`)를 통보할 방법이 없기 때문이다. `remote.workspaces` 가 이미 워커
    ///   완료 후 지연 회신하는 선례다. 지연 구간은 SSH 터널 수립 + IPC 1회 뿐이고
    ///   attach 자체(mirror 구성)는 기다리지 않는다.
    fn ipc_dispatch_remote_attach(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let conn = match RemoteConnParams::parse(&cmd.request.params) {
            Ok(c) => c,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(rpc_id, msg),
                );
                return;
            }
        };
        let target = match RemoteAttachTarget::parse(&cmd.request.params) {
            Ok(t) => t,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(rpc_id, msg),
                );
                return;
            }
        };

        let tx = self.auto_attach_tx.clone();
        let proxy = self.view.proxy.clone();
        match target {
            RemoteAttachTarget::Existing(remote_ws) => {
                std::thread::spawn(move || {
                    let result = conn.resolve_endpoint();
                    send_attach_outcome(&tx, &proxy, remote_ws, result);
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
            RemoteAttachTarget::Create { name, cwd } => {
                let response_tx = cmd.response_tx.clone();
                std::thread::spawn(move || {
                    remote_attach_create_worker(conn, name, cwd, rpc_id, &response_tx, &tx, &proxy);
                });
            }
        }
    }
}

/// `remote.*` 공통 접속 파라미터(`profile` XOR `ssh` + 포트 발견 옵션).
///
/// 두 디스패처(`remote.workspaces` / `remote.attach`)가 같은 상호배타 가드를 각자
/// 재현하면 메시지가 어긋나므로 한 곳에 모은다. CLI 선처리(`run.rs`)의 가드와 같은 규약.
struct RemoteConnParams {
    profile: Option<String>,
    ssh: Option<String>,
    remote_tasty: String,
    remote_port_mode: String,
}

impl RemoteConnParams {
    fn parse(params: &serde_json::Value) -> Result<Self, &'static str> {
        let profile = params
            .get("profile")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ssh = params
            .get("ssh")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if profile.is_some() && ssh.is_some() {
            return Err("'profile' and 'ssh' are mutually exclusive");
        }
        if profile.is_none() && ssh.is_none() {
            return Err("one of 'profile' or 'ssh' is required");
        }
        Ok(Self {
            profile,
            ssh,
            remote_tasty: params
                .get("remote_tasty")
                .and_then(|v| v.as_str())
                .unwrap_or("tasty")
                .to_string(),
            remote_port_mode: params
                .get("remote_port_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string(),
        })
    }

    /// 접속 스펙 resolve → 엔드포인트(SSH 터널/loopback). **블로킹** — 워커 전용.
    fn resolve_endpoint(&self) -> anyhow::Result<(Option<tasty_ssh::SshTunnel>, u16)> {
        let (target, rt, pm, pf) = tasty_remote::browse::resolve_connection_spec(
            self.profile.as_deref(),
            self.ssh.as_deref(),
            &self.remote_tasty,
            &self.remote_port_mode,
        )?;
        tasty_remote::browse::resolve_endpoint(&target, &rt, &pm, pf.as_deref())
    }
}

/// `remote.attach` 의 attach 대상 — 기존 원격 ws 지정 vs 원격에 새로 생성.
enum RemoteAttachTarget {
    Existing(u32),
    Create {
        name: Option<String>,
        cwd: Option<String>,
    },
}

impl RemoteAttachTarget {
    fn parse(params: &serde_json::Value) -> Result<Self, String> {
        let remote_ws = params
            .get("remote_workspace")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let new_workspace = params
            .get("new_workspace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let cwd = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        match (remote_ws, new_workspace) {
            (Some(_), true) => {
                Err("'remote_workspace' and 'new_workspace' are mutually exclusive".to_string())
            }
            (None, false) => Err(
                "one of 'remote_workspace' (u32) or 'new_workspace' (bool) is required".to_string(),
            ),
            (Some(id), false) => {
                // 생성 전용 옵션을 조용히 무시하면 "이름을 줬는데 안 붙었다" 로 보인다.
                if name.is_some() || cwd.is_some() {
                    return Err(
                        "'name'/'cwd' require 'new_workspace': true (they describe the workspace to create)"
                            .to_string(),
                    );
                }
                Ok(Self::Existing(id))
            }
            (None, true) => Ok(Self::Create { name, cwd }),
        }
    }
}

/// 해석된 엔드포인트를 auto-attach 결과 채널로 넘기고 메인 루프를 깨운다.
/// 메인 루프가 drain 해 focus 중립 `start_gui_attach` 를 수행한다.
fn send_attach_outcome(
    tx: &std::sync::mpsc::Sender<crate::app::auto_attach::AutoAttachOutcome>,
    proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
    remote_ws: u32,
    result: anyhow::Result<(Option<tasty_ssh::SshTunnel>, u16)>,
) {
    let outcome = crate::app::auto_attach::AutoAttachOutcome {
        anchor_ws_id: None,
        remote_ws,
        result,
        is_reconnect: false,
    };
    let _ = tx.send(outcome); // 수신자(메인 루프) drop 시에만 실패 — 무시.
    let _ = proxy.send_event(AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
}

/// `new_workspace` 워커 — 엔드포인트 해석 → 원격 `workspace.create` → 지연 회신 →
/// 생성된 id 로 attach outcome push.
///
/// 실패하면 outcome 을 보내지 않고 에러로 회신한다(붙을 대상이 없으므로). 그 경우
/// 터널 핸들은 여기서 Drop 되어 자식 ssh 가 회수된다(고아 터널 방지).
fn remote_attach_create_worker(
    conn: RemoteConnParams,
    name: Option<String>,
    cwd: Option<String>,
    rpc_id: serde_json::Value,
    response_tx: &std::sync::mpsc::SyncSender<host_ipc::protocol::JsonRpcResponse>,
    tx: &std::sync::mpsc::Sender<crate::app::auto_attach::AutoAttachOutcome>,
    proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
) {
    let (tunnel, port) = match conn.resolve_endpoint() {
        Ok(v) => v,
        Err(e) => {
            send_response(
                response_tx,
                host_ipc::protocol::JsonRpcResponse::error(
                    rpc_id,
                    -32050,
                    // `{e:#}` — anyhow 컨텍스트 체인을 한 줄로 편다. 최상위만 실으면
                    // 정작 원인(포트 발견 실패/터널 거부)이 호출자에게 안 보인다.
                    format!("remote endpoint resolve failed: {e:#}"),
                ),
            );
            return;
        }
    };
    let created = match tasty_remote::create::create_via_port(port, name.as_deref(), cwd.as_deref())
    {
        Ok(c) => c,
        Err(e) => {
            // 원격이 `cwd does not exist: …` 같은 invalid_params 를 돌려준 경우도 여기로
            // 온다 — 원문 메시지를 그대로 실어 호출자가 원인을 알 수 있게 한다.
            send_response(
                response_tx,
                host_ipc::protocol::JsonRpcResponse::error(
                    rpc_id,
                    -32050,
                    format!("remote workspace.create failed: {e:#}"),
                ),
            );
            return;
        }
    };
    send_response(
        response_tx,
        host_ipc::protocol::JsonRpcResponse::success(
            rpc_id,
            serde_json::json!({
                "attaching": true,
                "created": true,
                "remote_workspace": created.id,
                "name": created.name,
                "index": created.index,
            }),
        ),
    );
    send_attach_outcome(tx, proxy, created.id, Ok((tunnel, port)));
}

/// `ui.screenshot` 의 대상 창 결정.
///
/// `windows` 는 `(window id, main view 인가)` 를 창 하나당 하나씩 담는다. 성공하면
/// 그 슬라이스의 **인덱스**를 돌려준다(호출자가 같은 순서의 `WindowId` 배열에서
/// 되찾을 수 있게 — u64 로 다시 찾는 왕복을 없앤다). 실패는 `(JSON-RPC code, message)`.
///
/// 두 갈래가 의도적으로 비대칭이다.
///
/// - **`window_id` 를 명시하면 모든 창이 대상이다** — 설정·플러그인·종료 확인 모달과
///   preset 창을 포함한다. 캡처는 이미 그려진 프레임의 readback 이라 사용자 상태
///   (포커스 / 선택 / 스크롤 / 커서)를 바꾸지 않으므로 원칙 1 의 금지 대상이 아니고,
///   `ui.screenshot` 은 `local_only`(plugin 미노출)라 호출자는 이미 사용자 권한으로
///   `config.toml` 과 PTY 를 읽을 수 있다. 근거 전체는
///   `docs/ai-verification/screenshot-methods.md` "무엇을 캡처할 수 있는가".
/// - **미지정 시의 자동 선택은 main 창이 정확히 하나일 때뿐이다** — 모달로는 절대
///   폴백하지 않고 포커스도 보지 않는다(원칙 3). "지금 보고 있는 창" 에 기대는 순간
///   같은 명령이 사용자 상태에 따라 다른 것을 캡처한다.
///
/// 캡처가 **행동** 대상 집합을 넓히는 것은 아니다 — `window.list` 와 `window.close` 는
/// 종전대로 main 창만 다룬다(모달·preset 은 사용자 조작 영역).
///
/// 결정의 근거·기각 대안·재검토 조건:
/// `docs/adr/0118-screenshot-reads-any-window-explicit-id-only.md`.
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

    /// main 2 개(10, 11) + 설정 모달(20) + 종료 확인 모달(21).
    const MIXED: &[(u64, bool)] = &[(10, true), (20, false), (11, true), (21, false)];
    /// main 1 개(10) + 설정 모달(20).
    const ONE_MAIN: &[(u64, bool)] = &[(10, true), (20, false)];

    #[test]
    fn an_explicit_id_can_name_a_modal_window() {
        // 이것이 이 함수의 존재 이유다 — 되돌리면(모달을 후보에서 빼면) 여기서 깨진다.
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
        // main 이 없고 모달만 떠 있어도 "그거라도 찍는다" 로 가지 않는다 — 자동 선택이
        // 사용자가 무엇을 열어뒀는지에 의존하기 시작하면 원칙 3 이 깨진다.
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
