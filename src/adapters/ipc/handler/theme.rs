//! `theme.query` — 창 없이도 답이 정해지는 외형 조회.
//!
//! 이 모듈이 `webview.rs` 에서 갈라져 나온 이유: 그 파일은 `gui` feature 로 게이트돼
//! 있고(native webview overlay 가 gui 전용이다), 안에 있던 `theme.query` 는 **그 게이트를
//! 같이 받고 있었다.** 하지만 이 핸들러가 읽는 것은 전역 Theme 과 `CoreState.settings`
//! 둘뿐이다 — 창도, surface 도, 렌더러도 안 본다. 그래서 헤드리스에서 답하지 못할
//! 이유가 없었는데도 `-32601`(그런 메서드 없음)로 끝나고 있었다(실측 2026-09-05).
//!
//! 헤드리스는 CLI 전용 실행 형태라 이 부재는 편의 문제가 아니라
//! [identity](../../../../docs/identity.md) 원칙 2 의 구멍이다. 판정 표는
//! [headless-ipc-surface](../../../../docs/dev-guide/headless-ipc-surface.md).

use serde_json::Value;

use tasty_ipc::protocol::JsonRpcResponse;

/// `theme.query` — 현재 Theme 색상표 + light 여부 + UI zoom.
///
/// webview-kind surface(예: markdown)는 egui-mesh 와 달리 `surface.set_context` 를 받지
/// 않아 Theme 이 자동으로 밀리지 않는다 — 이 read-only 조회가 그 대체 경로다.
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
