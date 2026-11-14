//! `debug.inject_egui_clipboard` — 메인 창의 egui 입력에 붙여넣기·복사·잘라내기 하나를 넣는다.
//! 실제 키가 거치는 편집 키 판정(바인딩 판정, 플러그인 회신 보류)과 egui-winit 기본 키 변환을 건너뛰고
//! 그 뒤의 이벤트를 바로 넣어 플러그인 팝업 전달과 복사 결과 처리를 키 없이 재현한다. 사용자 입력 재현이므로
//! release에는 두지 않는다.

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::{IpcCommand, send_response};

/// 요청의 편집 하나를 egui 이벤트로 만든다. 붙여넣기에는 비어 있지 않은 `text`가 있어야 한다.
fn clipboard_event(op: &str, text: Option<&str>) -> Result<egui::Event, String> {
    match op {
        "paste" => match text {
            Some(text) if !text.is_empty() => Ok(egui::Event::Paste(text.to_string())),
            _ => Err("op 'paste' needs a non-empty string 'text'".to_string()),
        },
        "copy" => Ok(egui::Event::Copy),
        "cut" => Ok(egui::Event::Cut),
        other => Err(format!("unknown op '{other}': expected paste, copy or cut")),
    }
}

impl App {
    pub(super) fn ipc_handle_debug_egui_clipboard(&mut self, cmd: &IpcCommand) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = match self.debug_egui_clipboard(&cmd.request.params) {
            Ok(body) => JsonRpcResponse::success(id, body),
            Err((code, msg)) => JsonRpcResponse::error(id, code, msg),
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    fn debug_egui_clipboard(
        &mut self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let op = params
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| (-32602, "missing or non-string 'op'".to_string()))?;
        let event = clipboard_event(op, params.get("text").and_then(|v| v.as_str()))
            .map_err(|msg| (-32602, msg))?;
        let (wid, view) = if params.get("window_id").is_some() {
            self.debug_egui_target(params)?
        } else {
            let focused = self
                .view
                .focused_view_id
                .ok_or_else(|| (-32603, "no focused main window".to_string()))?;
            let view = self
                .view
                .views
                .get_mut(&focused)
                .ok_or_else(|| (-32603, "focused main window is gone".to_string()))?;
            (u64::from(focused), view.as_mut())
        };
        if view.as_main().is_none() {
            return Err((-32602, format!("Window id {wid} is not a main window")));
        }
        view.base_mut().gpu.debug_push_egui_events(vec![event]);
        view.mark_dirty();
        Ok(serde_json::json!({ "injected": true, "window_id": wid }))
    }
}

#[cfg(test)]
mod tests {
    use super::clipboard_event;

    /// 붙여넣기는 글자를 싣고 복사·잘라내기는 글자 없이 만들며, 빈 붙여넣기와 모르는 편집은 거절한다.
    #[test]
    fn each_op_becomes_one_egui_edit() {
        assert_eq!(
            clipboard_event("paste", Some("a")),
            Ok(egui::Event::Paste("a".into()))
        );
        assert_eq!(clipboard_event("copy", None), Ok(egui::Event::Copy));
        assert_eq!(
            clipboard_event("cut", Some("ignored")),
            Ok(egui::Event::Cut)
        );
        assert!(clipboard_event("paste", None).is_err());
        assert!(clipboard_event("paste", Some("")).is_err());
        assert!(clipboard_event("select_all", None).is_err());
    }
}
