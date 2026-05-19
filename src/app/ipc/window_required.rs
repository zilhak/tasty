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
use crate::window::Window as _;

impl App {
    pub(crate) fn ipc_step_window_required(&mut self, cmd: &IpcCommand) -> IpcStep {
        #[allow(unused_mut)]
        let mut is_window_required = cmd.request.method.starts_with("surface.ime_");
        #[cfg(debug_assertions)]
        {
            is_window_required = is_window_required
                || cmd.request.method == "debug.info"
                || cmd.request.method == "ui.screenshot";
        }
        if !is_window_required {
            return IpcStep::NotHandled;
        }
        let focused_id = match self.engine.focused_window_id {
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
            .windows
            .get_mut(&focused_id)
            .and_then(|w| w.as_main_mut())
        {
            Some(w) => w,
            // focused id 가 있는데 MainWindow 가 아니면 (모달 등) 본 step 으로 처리 불가 —
            // 이 케이스를 옛 코드는 `continue` (드롭) 으로 처리했다. 동일 의미를
            // Handled 로 표현 (응답 전송 없음 → client 가 timeout).
            None => return IpcStep::Handled,
        };

        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.info" {
            let debug_data =
                crate::debug_info::collect(&w.state, Some(&w.base.gpu), w.ime_active);
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
