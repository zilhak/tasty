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
                || cmd.request.method == "debug.inject_window_mouse";
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
