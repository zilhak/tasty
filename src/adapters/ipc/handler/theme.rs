//! 창 없이 현재 Theme과 설정을 조회한다. GUI와 헤드리스가 같은 함수를 쓴다.

use serde_json::Value;

use tasty_ipc::protocol::JsonRpcResponse;

/// webview는 egui-mesh의 set_context를 받지 않으므로 이 조회로 색·light 여부·UI zoom을 얻는다.
pub fn handle_query(engine: &crate::core::CoreState, id: Value) -> JsonRpcResponse {
    let theme = crate::theme::theme();
    let wire = tasty_plugin_protocol::ThemeWire {
        colors: theme.to_colors(),
        is_light: theme.is_light,
        ui_zoom: engine.settings.appearance.ui_scale_factor(),
    };
    match serde_json::to_value(&wire) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32000, format!("theme serialize failed: {e}")),
    }
}
