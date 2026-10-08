//! `debug.inject_egui_*`의 `window_id` 지정 — 지목한 창의 egui 입력 큐에 이벤트를 넣는다.
//! 설정처럼 별도 winit 창으로 뜨는 화면의 입력은 포커스된 메인 창 경로로 닿지 않는다.
//! 사용자 입력 재현이므로 release에는 두지 않는다.

use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::{IpcCommand, send_response};
use crate::view::main::debug_input::{push_egui_key, push_egui_text};
use crate::view::ui::View;

/// 창을 지정해 넣을 egui 입력 종류. 메서드 이름 분기는 라우터(`debug_methods.rs`)가 맡는다.
pub(super) enum EguiInjection {
    Text,
    Key,
}

impl App {
    pub(super) fn ipc_handle_debug_egui_to_window(
        &mut self,
        cmd: &IpcCommand,
        kind: EguiInjection,
    ) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let params = &cmd.request.params;
        let result = match kind {
            EguiInjection::Text => self.debug_egui_text_to_window(params),
            EguiInjection::Key => self.debug_egui_key_to_window(params),
        };
        let response = match result {
            Ok(body) => JsonRpcResponse::success(id, body),
            Err((code, msg)) => JsonRpcResponse::error(id, code, msg),
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    fn debug_egui_text_to_window(
        &mut self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
            return Err((-32602, "missing or non-string 'text'".to_string()));
        };
        let (wid, view) = self.debug_egui_target(params)?;
        let injected = push_egui_text(&mut view.base_mut().gpu, text);
        if injected {
            view.mark_dirty();
        }
        Ok(serde_json::json!({ "injected": injected, "window_id": wid }))
    }

    /// 기본값은 focused window 경로와 같다(`Escape`, 누름).
    fn debug_egui_key_to_window(
        &mut self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("Escape");
        let pressed = params
            .get("pressed")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let (wid, view) = self.debug_egui_target(params)?;
        let injected = push_egui_key(&mut view.base_mut().gpu, key, pressed);
        if injected {
            view.mark_dirty();
        }
        Ok(serde_json::json!({ "injected": injected, "window_id": wid }))
    }

    /// 창은 ID로만 고른다. 포커스된 창으로 대신하지 않는다.
    fn debug_egui_target(
        &mut self,
        params: &serde_json::Value,
    ) -> Result<(u64, &mut dyn View), (i32, String)> {
        let Some(wid) =
            params::read_int::<u64>(params, "window_id").map_err(|msg| (-32602, msg))?
        else {
            return Err((-32602, "'window_id' must be a window id".to_string()));
        };
        self.view
            .views
            .iter_mut()
            .find(|(w, _)| u64::from(**w) == wid)
            .map(|(_, view)| (wid, view.as_mut()))
            .ok_or_else(|| (-32602, format!("Window id {wid} not found")))
    }
}
