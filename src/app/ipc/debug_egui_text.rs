//! `debug.inject_egui_text`의 `window_id` 지정 — 지목한 창의 egui 입력 큐에 문자를 넣는다.
//! 설정처럼 별도 winit 창으로 뜨는 화면의 입력칸은 포커스된 메인 창 경로로 닿지 않는다.
//! 사용자 입력 재현이므로 release에는 두지 않는다.

use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::{IpcCommand, send_response};
use crate::view::main::debug_input::push_egui_text;

impl App {
    pub(super) fn ipc_handle_debug_egui_text_to_window(&mut self, cmd: &IpcCommand) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = match self.debug_egui_text_to_window(&cmd.request.params) {
            Ok(body) => JsonRpcResponse::success(id, body),
            Err((code, msg)) => JsonRpcResponse::error(id, code, msg),
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// 창은 ID로만 고른다. 포커스된 창으로 대신하지 않는다.
    fn debug_egui_text_to_window(
        &mut self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
            return Err((-32602, "missing or non-string 'text'".to_string()));
        };
        let Some(wid) =
            params::read_int::<u64>(params, "window_id").map_err(|msg| (-32602, msg))?
        else {
            return Err((-32602, "'window_id' must be a window id".to_string()));
        };
        let Some(view) = self
            .view
            .views
            .iter_mut()
            .find(|(w, _)| u64::from(**w) == wid)
            .map(|(_, view)| view)
        else {
            return Err((-32602, format!("Window id {wid} not found")));
        };
        let injected = push_egui_text(&mut view.base_mut().gpu, text);
        if injected {
            view.mark_dirty();
        }
        Ok(serde_json::json!({ "injected": injected, "window_id": wid }))
    }
}
