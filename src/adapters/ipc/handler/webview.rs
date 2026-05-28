//! `webview.*` IPC 핸들러 — plugin 이 webview-enabled surface 의 URL/navigation 제어.
//!
//! host 는 webview 토대 (native overlay) 만 제공하고, plugin 은 IPC 로 URL 설정.
//! sync_webviews 가 매 프레임 RemoteSurface 의 webview_url 캐시를 읽어 native
//! WebView 에 load_url 자동 호출.

use serde_json::Value;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

/// `webview.set_url(surface_id, url)` — webview-enabled kind 의 RemoteSurface 에 URL 설정.
pub fn handle_set_url(
    _state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match params.get("surface_id").and_then(|v| v.as_u64()) {
        Some(s) => s as u32,
        None => return JsonRpcResponse::invalid_params(id, "missing 'surface_id'"),
    };
    let url = match params.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "missing 'url'"),
    };

    // surface_id 와 일치하는 RemoteSurface 찾기 + set_webview_url 호출.
    for ws in &engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    let surface = tab.surface();
                    if surface.surface_id() != Some(sid) {
                        continue;
                    }
                    if let Some(rs) = surface
                        .as_any()
                        .downcast_ref::<crate::plugin::remote_surface::RemoteSurface>()
                    {
                        rs.set_webview_url(Some(url));
                        return JsonRpcResponse::success(id, serde_json::json!({ "ok": true }));
                    }
                    return JsonRpcResponse::error(
                        id,
                        -32000,
                        "surface is not a webview-enabled RemoteSurface",
                    );
                }
            }
        }
    }
    JsonRpcResponse::error(id, -32000, "surface_id not found")
}
