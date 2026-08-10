//! `webview.*` IPC 핸들러 — plugin 이 webview-enabled surface 의 URL/navigation 제어.
//!
//! host 는 webview 토대 (native overlay) 만 제공하고, plugin 은 IPC 로 URL 설정.
//! sync_webviews 가 매 프레임 RemoteSurface 의 webview_url 캐시를 읽어 native
//! WebView 에 load_url 자동 호출.

use serde_json::Value;

use crate::plugin::PluginManager;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// `webview.set_url(surface_id, url)` — webview-enabled kind 의 RemoteSurface 에 URL 설정.
///
/// 시그니처는 *read-only* (`&AppState + &CoreState`) — `_state` 는 미사용,
/// `engine` 도 `&engine.workspaces` 순회 + `RemoteSurface::set_webview_url`
/// (interior-mut `&self` 메서드) 만 호출. `handle_tree` 와 동일 패턴 (D.3.C.H.2).
pub fn handle_set_url(
    _state: &AppState,
    engine: &crate::core::CoreState,
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
                        .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
                    ) {
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

/// webview 가 네비게이션을 시도한 URL 을 소유 plugin 에 통지(`webview.navigation_attempt`,
/// host→plugin — `handle_set_url` 의 반대 방향). "원격 http(s) 차단" 판정과 독립적으로,
/// 차단 여부와 무관하게 시도마다 항상 호출된다(호출부가 native backend 의
/// decide-policy/NavigationStarting 콜백에서 캡처한 URL 을 매 프레임 poll 해 넘긴다).
///
/// surface_id 에 대응하는 webview-enabled `RemoteSurface` 가 없으면 조용히 no-op —
/// 네비게이션 캡처와 surface 제거 사이에 프레임 경계가 끼어드는 정상적인 레이스다.
pub fn notify_navigation_attempt(
    mgr: &PluginManager,
    engine: &crate::core::CoreState,
    surface_id: u32,
    url: &str,
) {
    for ws in &engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    let surface = tab.surface();
                    if surface.surface_id() != Some(surface_id) {
                        continue;
                    }
                    if let Some(rs) = surface
                        .as_any()
                        .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
                    ) {
                        mgr.send_webview_navigation_attempt(
                            &rs.plugin_id,
                            &tasty_plugin_protocol::WebviewNavigationAttemptParams {
                                surface_id,
                                url: url.to_string(),
                            },
                        );
                    }
                    return;
                }
            }
        }
    }
}
