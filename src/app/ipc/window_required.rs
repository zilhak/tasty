//! step 4: focused window 가 필요한 메서드 (GPU/IME/debug 도구).
//!
//! - `surface.ime_*`
//! - `debug.info` (debug only)
//! - `ui.screenshot` (debug only)

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};
#[cfg(debug_assertions)]
use crate::view::ui::View as _;

impl App {
    pub(crate) fn ipc_step_window_required(&mut self, cmd: &IpcCommand) -> IpcStep {
        #[allow(unused_mut)]
        let mut is_window_required = cmd.request.method.starts_with("surface.ime_");
        #[cfg(debug_assertions)]
        {
            is_window_required = is_window_required
                || cmd.request.method == "debug.info"
                || cmd.request.method == "ui.screenshot"
                || cmd.request.method == "debug.inject_window_mouse"
                || cmd.request.method == "debug.inject_egui_mouse"
                || cmd.request.method == "debug.inject_egui_key"
                || cmd.request.method == "debug.selection"
                || cmd.request.method == "debug.pending_menu"
                || cmd.request.method == "debug.focused_surface";
        }
        if !is_window_required {
            return IpcStep::NotHandled;
        }
        let focused_id = match self.view.focused_view_id {
            Some(id) => id,
            None => {
                let response = host_ipc::protocol::JsonRpcResponse::error(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    -32000,
                    "No window available for this command",
                );
                send_response(&cmd.response_tx, response);
                return IpcStep::Handled;
            }
        };
        let w = match self
            .view
            .views
            .get_mut(&focused_id)
            .and_then(|w| w.as_main_mut())
        {
            Some(w) => w,
            // focused id 가 있는데 MainView 가 아니면 (모달 등) 본 step 으로 처리 불가 —
            // 이 케이스를 옛 코드는 `continue` (드롭) 으로 처리했다. 동일 의미를
            // Handled 로 표현 (응답 전송 없음 → client 가 timeout).
            None => return IpcStep::Handled,
        };

        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.info" {
            let debug_data = crate::debug_info::collect(
                &w.state,
                &w.core_state,
                Some(&w.base.gpu),
                w.ime_active,
            );
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                debug_data,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
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
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({"path": path, "scheduled": true}),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_window_mouse" {
            use crate::view::main::debug_input::InjectPointer;
            let p = &cmd.request.params;
            let surface_id = p.get("surface_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            // fx, fy ∈ [0,1] surface-local 정규화 좌표 (기본 중앙).
            let fx = p.get("fx").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let fy = p.get("fy").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let event_type = p
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("move");
            let button = match p.get("button").and_then(|v| v.as_u64()).unwrap_or(0) {
                1 => winit::event::MouseButton::Middle,
                2 => winit::event::MouseButton::Right,
                _ => winit::event::MouseButton::Left,
            };
            let action = match event_type {
                "press" => InjectPointer::Button {
                    button,
                    pressed: true,
                },
                "release" => InjectPointer::Button {
                    button,
                    pressed: false,
                },
                "scroll" => InjectPointer::Scroll {
                    dx: p.get("scroll_dx").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    dy: p.get("scroll_dy").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                },
                _ => InjectPointer::Move,
            };
            let ok = w.debug_inject_mesh_pointer(surface_id, fx, fy, action);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "injected": ok }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // egui-mesh popup(A2) 입력 forward 검증용 — winit 핸들러가 아니라 egui 입력 큐에
        // 직접 주입한다(popup 은 egui input 을 통해 plugin 으로 forward 되기 때문). 좌표는
        // window 정규화 (fx,fy ∈ [0,1] 논리). release 미노출, debug 격리(원칙 1·3).
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_egui_mouse" {
            use crate::view::main::debug_input::InjectPointer;
            let p = &cmd.request.params;
            let fx = p.get("fx").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let fy = p.get("fy").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let event_type = p
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("move");
            let button = match p.get("button").and_then(|v| v.as_u64()).unwrap_or(0) {
                1 => winit::event::MouseButton::Middle,
                2 => winit::event::MouseButton::Right,
                _ => winit::event::MouseButton::Left,
            };
            let action = match event_type {
                "press" => InjectPointer::Button {
                    button,
                    pressed: true,
                },
                "release" => InjectPointer::Button {
                    button,
                    pressed: false,
                },
                "scroll" => InjectPointer::Scroll {
                    dx: p.get("scroll_dx").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    dy: p.get("scroll_dy").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                },
                _ => InjectPointer::Move,
            };
            let surface_id = p
                .get("surface_id")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let ok = w.debug_inject_egui_pointer(fx, fy, surface_id, action);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "injected": ok }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_egui_key" {
            let p = &cmd.request.params;
            let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("Escape");
            let pressed = p.get("pressed").and_then(|v| v.as_bool()).unwrap_or(true);
            let ok = w.debug_inject_egui_key(key, pressed);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "injected": ok }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // read-only debug dump: 마우스 라우팅이 만든 로컬 텍스트 선택 상태를 그대로 노출한다
        // (input 안전망 — press→move→release 주입 후 selection 회귀를 단언). 부수효과 0,
        // 사용자 상태를 변경하지 않으므로 관찰 전용. debug 격리(원칙 1·3), release 미노출.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.selection" {
            let sel = w.text_selection.as_ref();
            let body = match sel {
                Some(s) => {
                    let n = s.normalized();
                    serde_json::json!({
                        "present": true,
                        "surface_id": s.surface_id,
                        "mode": format!("{:?}", s.mode),
                        "dragging": s.dragging,
                        "empty": s.is_empty(),
                        "anchor": { "col": s.anchor.col, "row": s.anchor.absolute_row },
                        "cursor": { "col": s.cursor.col, "row": s.cursor.absolute_row },
                        "start": { "col": n.start.col, "row": n.start.absolute_row },
                        "end": { "col": n.end.col, "row": n.end.absolute_row },
                    })
                }
                None => serde_json::json!({ "present": false }),
            };
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                body,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // read-only debug dump: 우클릭 라우팅이 세운 대기 중 컨텍스트 메뉴(종류/대상 surface).
        // 우클릭 주입 후 메뉴 라우팅 회귀를 단언한다. 관찰 전용, debug 격리, release 미노출.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.pending_menu" {
            // 주입 경로가 세운 메뉴는 `debug_captured_menu` 로 가로채져 있다(블로킹 회피).
            // live pending 이 있으면(비-주입 경로) 그걸, 아니면 포획본을 관찰한다.
            let menu = w
                .state
                .dialogs
                .pending_native_menu
                .as_ref()
                .or(w.debug_captured_menu.as_ref());
            let body = match menu {
                Some(menu) => {
                    let (kind, surface_id) = pending_menu_kind(menu);
                    let mut obj = serde_json::json!({ "present": true, "kind": kind });
                    if let Some(sid) = surface_id {
                        obj["surface_id"] = serde_json::json!(sid);
                    }
                    obj
                }
                None => serde_json::json!({ "present": false }),
            };
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                body,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // read-only debug dump: 현재 포커스된 surface id (없으면 null). click-to-activate
        // 라우팅(비활성 surface 좌클릭 → 포커스 전환) 회귀를 단언한다. `surface.list` 는
        // engine 단위라 view-layer 포커스를 노출하지 않으므로 별도 관찰 IPC 가 필요하다.
        // 관찰 전용, debug 격리, release 미노출.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.focused_surface" {
            let focused = w.state.focused_surface_id(&w.core_state);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "surface_id": focused }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method.starts_with("surface.ime_") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = host_ipc::handler::ime::handle_ime_method(
                w,
                &cmd.request.method,
                &cmd.request.params,
                id,
            );
            send_response(&cmd.response_tx, response);
            w.base.dirty = true;
        }
        IpcStep::Handled
    }
}

/// `PendingNativeMenu` variant → (kind 문자열, 대상 surface_id). 관찰용 debug dump 전용.
#[cfg(debug_assertions)]
fn pending_menu_kind(menu: &crate::state::PendingNativeMenu) -> (&'static str, Option<u32>) {
    use crate::state::PendingNativeMenu as M;
    match menu {
        M::Tab { .. } => ("Tab", None),
        M::Pane { .. } => ("Pane", None),
        M::Workspace { .. } => ("Workspace", None),
        M::TerminalSurface { surface_id, .. } => ("TerminalSurface", Some(*surface_id)),
        M::Surface { surface_id, .. } => ("Surface", Some(*surface_id)),
        M::Explorer { surface_id, .. } => ("Explorer", Some(*surface_id)),
        M::ExplorerFavorite { surface_id, .. } => ("ExplorerFavorite", Some(*surface_id)),
        M::NewWorkspaceButton { .. } => ("NewWorkspaceButton", None),
        M::WorkspaceCategoryHeader { .. } => ("WorkspaceCategoryHeader", None),
        M::SidebarBackground { .. } => ("SidebarBackground", None),
        M::NewTabButton { .. } => ("NewTabButton", None),
    }
}
