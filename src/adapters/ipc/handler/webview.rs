//! `webview.*` IPC 핸들러 — plugin 이 webview-enabled surface 의 URL/navigation 제어.
//!
//! host 는 webview 토대 (native overlay) 만 제공하고, plugin 은 IPC 로 URL 설정.
//! sync_webviews 가 매 프레임 RemoteSurface 의 webview_url 캐시를 읽어 native
//! WebView 에 load_url 자동 호출.

use super::params::require_u32;
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
    let sid = match require_u32(params, "surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let url = match params.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "missing 'url'"),
    };

    // surface_id 와 일치하는 RemoteSurface 찾기 + set_webview_url 호출.
    // 탭당 1 개(`Tab::surface()` = 포커스 leaf)가 아니라 레이아웃 트리 전체를
    // 조회한다 — surface 레벨 split(SurfaceGroup)의 비포커스 leaf 도 대상이다.
    for ws in &engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    let Some(layout) = tab.layout_if_initialized() else {
                        continue;
                    };
                    let Some(surface) = layout.find_surface(sid) else {
                        continue;
                    };
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

/// `theme.query()` — 현재 resolved 전역 Theme 을 wire 스냅샷(색 집합+is_light+ui_zoom)으로
/// 반환한다. egui-mesh kind 는 매 `surface.set_context` 에 Theme 이 실려오지만, webview-kind
/// surface(예: markdown)는 host 가 mesh 프레임을 합성하지 않으므로 `set_context` 자체를
/// 받지 않는다 — plugin 이 문서를 (재)생성할 때 이 read-only 조회로 대신한다. 이후 색이
/// 바뀌면 host 가 발행하는 `theme.changed` Event Bus 이벤트(`event_subscribe`)를 구독해
/// 재호출하는 것은 plugin 책임.
pub fn handle_theme_query(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: Value,
) -> JsonRpcResponse {
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
    // `handle_set_url` 과 동일하게 레이아웃 트리 전체를 조회한다(split 비포커스 leaf 포함).
    for ws in &engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    let Some(layout) = tab.layout_if_initialized() else {
                        continue;
                    };
                    let Some(surface) = layout.find_surface(surface_id) else {
                        continue;
                    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SplitDirection;
    use serde_json::json;

    /// 탭의 focused leaf 를 유지한 채 `kind` surface 를 형제 leaf 로 추가하고
    /// 새 surface id 를 돌려준다. `Core::apply_split_surface`(`DomainIntent::SplitSurface`)
    /// 의 모델 효과만 재현한다 — 새 leaf 는 비포커스다.
    fn split_in_kind_surface(
        state: &crate::state::AppState,
        engine: &mut crate::core::CoreState,
        target_surface_id: u32,
        kind: &str,
        params: &Value,
    ) -> u32 {
        let new_sid = engine.next_ids.next_surface();
        let surface = engine
            .create_surface_via_registry(kind, new_sid, None, params)
            .expect("create surface via registry");
        let ws = &mut engine.workspaces[state.active_workspace];
        let pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("focused pane")
            .split_surface_by_id_with_surface(target_surface_id, SplitDirection::Vertical, surface)
            .expect("split surface");
        new_sid
    }

    fn focused_surface_id(state: &crate::state::AppState, engine: &crate::core::CoreState) -> u32 {
        let ws = &engine.workspaces[state.active_workspace];
        let pane = ws
            .pane_layout()
            .find_pane(ws.focused_pane)
            .expect("focused pane");
        pane.tabs[pane.active_tab].focused_surface
    }

    fn set_url(
        state: &crate::state::AppState,
        engine: &crate::core::CoreState,
        sid: u32,
    ) -> JsonRpcResponse {
        handle_set_url(
            state,
            engine,
            json!(1),
            &json!({ "surface_id": sid, "url": "file:///tmp/doc.html" }),
        )
    }

    /// split 탭의 **비포커스** leaf 도 `webview.set_url` 로 도달해야 한다.
    /// 수정 전에는 `Tab::surface()`(포커스 leaf 1 개)만 봐서 `surface_id not found` 였다.
    #[test]
    fn set_url_reaches_non_focused_split_leaf() {
        let (state, mut engine) = crate::state::tests::test_state();
        let terminal_sid = focused_surface_id(&state, &engine);
        let md_sid = split_in_kind_surface(
            &state,
            &mut engine,
            terminal_sid,
            "markdown",
            &json!({ "file": "/workspace/proj/readme.md" }),
        );

        // 포커스는 원래 터미널 leaf 에 남아 있어야 한다(= md_sid 는 비포커스 leaf).
        assert_eq!(focused_surface_id(&state, &engine), terminal_sid);

        let resp = set_url(&state, &engine, md_sid);
        assert!(
            resp.error.is_none(),
            "non-focused split leaf should be reachable: {:?}",
            resp.error
        );
        assert_eq!(resp.result, Some(json!({ "ok": true })));
    }

    /// 3 leaf 중첩 split(`Split { first: Split{..}, second: Leaf }`)의 모든 leaf 가 조회된다.
    #[test]
    fn set_url_reaches_all_leaves_of_nested_split() {
        let (state, mut engine) = crate::state::tests::test_state();
        let terminal_sid = focused_surface_id(&state, &engine);
        // 1차: terminal | markdown_a → 2차: (terminal | markdown_b) | markdown_a
        let md_a = split_in_kind_surface(
            &state,
            &mut engine,
            terminal_sid,
            "markdown",
            &json!({ "file": "/workspace/proj/a.md" }),
        );
        let md_b = split_in_kind_surface(
            &state,
            &mut engine,
            terminal_sid,
            "markdown",
            &json!({ "file": "/workspace/proj/b.md" }),
        );

        for sid in [md_a, md_b] {
            let resp = set_url(&state, &engine, sid);
            assert!(
                resp.error.is_none(),
                "leaf {sid} should be reachable: {:?}",
                resp.error
            );
        }
    }

    /// 단독 leaf 탭(회귀 없음)과 존재하지 않는 id(기존 에러 보존).
    #[test]
    fn set_url_sole_leaf_ok_and_unknown_id_errors() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        state
            .test_add_markdown_tab(&mut engine, "/workspace/proj/readme.md".to_string())
            .unwrap();
        let md_sid = focused_surface_id(&state, &engine);
        let resp = set_url(&state, &engine, md_sid);
        assert!(
            resp.error.is_none(),
            "sole leaf regression: {:?}",
            resp.error
        );

        let resp = set_url(&state, &engine, 999_999);
        assert_eq!(
            resp.error.map(|e| e.message),
            Some("surface_id not found".to_string())
        );
    }

    /// webview 가 아닌 surface 는 기존대로 명시적 에러를 낸다(비포커스 leaf 라도).
    #[test]
    fn set_url_on_terminal_leaf_reports_not_webview() {
        let (state, mut engine) = crate::state::tests::test_state();
        let terminal_sid = focused_surface_id(&state, &engine);
        // 탭을 split 로 만들기 위해 markdown leaf 를 붙인다. 이 테스트가 검사하는
        // 대상은 터미널 leaf 쪽 응답이라 새 surface id 는 쓰지 않는다(반환값은
        // `u32` — 삼켜지는 `Result` 가 아니다. 실패는 헬퍼 안에서 panic 한다).
        split_in_kind_surface(
            &state,
            &mut engine,
            terminal_sid,
            "markdown",
            &json!({ "file": "/workspace/proj/readme.md" }),
        );
        let resp = set_url(&state, &engine, terminal_sid);
        assert_eq!(
            resp.error.map(|e| e.message),
            Some("surface is not a webview-enabled RemoteSurface".to_string())
        );
    }
}
