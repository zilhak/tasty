//! 플러그인이 RemoteSurface에 URL을 설정하면 sync_webviews가 native WebView에 반영한다.

use super::params::require_u32;
use serde_json::Value;

use crate::plugin::PluginManager;
use tasty_ipc::protocol::JsonRpcResponse;

/// 문서 변경을 attach 클라이언트에 알린다. 대상과 허용 종류는 attach_runtime이 검사한다.
fn notify_content_changed(
    engine: &crate::core::CoreState,
    surface: &crate::plugin_bridge::remote_surface::RemoteSurface,
    sid: u32,
) {
    use crate::model::Surface;
    if let Some((kind, plugin_id, _)) = surface.attach_content_info() {
        crate::core::attach_runtime::notify_markdown_changed(&engine.attach, kind, plugin_id, sid);
    }
}

/// RemoteSurface에 URL과 페이지 작성자 정보를 기록한다.
/// 외부 호출도 URL을 설정할 수 있지만 소유 플러그인이 쓴 페이지의 클릭만
/// 파일 열기의 사용자 행동으로 인정한다(ADR-0031).
pub fn handle_set_url(
    engine: &crate::core::CoreState,
    caller: &tasty_ipc::caller::CallerContext,
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

    // 분할 탭의 비포커스 surface도 찾도록 레이아웃 전체를 순회한다.
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
                        rs.set_webview_url(Some(url), is_owner(caller, rs));
                        notify_content_changed(engine, rs, sid);
                        return JsonRpcResponse::success(id, serde_json::json!({ "ok": true }));
                    }
                    tracing::warn!(
                        "webview.set_url: surface {sid} is not a webview-enabled RemoteSurface"
                    );
                    return JsonRpcResponse::error(
                        id,
                        -32000,
                        "surface is not a webview-enabled RemoteSurface",
                    );
                }
            }
        }
    }
    tracing::warn!("webview.set_url: surface {sid} not found in any workspace layout");
    JsonRpcResponse::error(id, -32000, "surface_id not found")
}

fn is_owner(
    caller: &tasty_ipc::caller::CallerContext,
    rs: &crate::plugin_bridge::remote_surface::RemoteSurface,
) -> bool {
    matches!(
        caller,
        tasty_ipc::caller::CallerContext::Plugin { plugin_id, .. } if *plugin_id == rs.plugin_id
    )
}

/// 차단 여부와 무관하게 navigation 시도를 소유 플러그인에 알린다.
/// 처리 전에 surface가 제거됐으면 무시한다. 통지한 플러그인과 페이지 소유 여부를 반환한다.
/// 호출자는 사용자 제스처 기록을 해당 플러그인에 연결하고 아닌 경우 이전 기록을 지운다(ADR-0031).
pub fn notify_navigation_attempt(
    mgr: &PluginManager,
    engine: &crate::core::CoreState,
    surface_id: u32,
    url: &str,
) -> Option<crate::plugin_bridge::user_navigation::NavigationOwner> {
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
                        return Some(crate::plugin_bridge::user_navigation::NavigationOwner {
                            plugin_id: rs.plugin_id.clone(),
                            wrote_page: rs.webview_page_by_owner(),
                        });
                    }
                    return None;
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SplitDirection;
    use serde_json::json;

    /// 포커스를 유지한 채 형제 surface를 추가한다. 실제 PTY를 만들지 않고 모델 변경만 재현한다.
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

    fn set_url(engine: &crate::core::CoreState, sid: u32) -> JsonRpcResponse {
        set_url_as(engine, &tasty_ipc::caller::CallerContext::Local, sid)
    }

    fn set_url_as(
        engine: &crate::core::CoreState,
        caller: &tasty_ipc::caller::CallerContext,
        sid: u32,
    ) -> JsonRpcResponse {
        handle_set_url(
            engine,
            caller,
            json!(1),
            &json!({ "surface_id": sid, "url": "file:///tmp/doc.html" }),
        )
    }

    fn plugin_caller(plugin_id: &str) -> tasty_ipc::caller::CallerContext {
        tasty_ipc::caller::CallerContext::Plugin {
            plugin_id: plugin_id.to_string(),
            permissions: std::sync::Arc::new(std::collections::HashSet::new()),
        }
    }

    fn remote_surface(
        engine: &crate::core::CoreState,
        sid: u32,
    ) -> &crate::plugin_bridge::remote_surface::RemoteSurface {
        for ws in &engine.workspaces {
            for pid in ws.pane_layout().all_pane_ids() {
                let Some(pane) = ws.pane_layout().find_pane(pid) else {
                    continue;
                };
                for tab in &pane.tabs {
                    if let Some(rs) = tab
                        .layout_if_initialized()
                        .and_then(|l| l.find_surface(sid))
                        .and_then(|s| {
                            s.as_any()
                                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
                        })
                    {
                        return rs;
                    }
                }
            }
        }
        panic!("remote surface {sid} not found");
    }

    #[test]
    fn set_url_remembers_whether_the_owning_plugin_wrote_the_page() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        state
            .test_add_markdown_tab(&mut engine, "/workspace/proj/readme.md".to_string())
            .unwrap();
        let md_sid = focused_surface_id(&state, &engine);
        let owner = remote_surface(&engine, md_sid).plugin_id.clone();
        assert!(!remote_surface(&engine, md_sid).webview_page_by_owner());

        let written_by = |caller: &tasty_ipc::caller::CallerContext| {
            assert!(set_url_as(&engine, caller, md_sid).error.is_none());
            remote_surface(&engine, md_sid).webview_page_by_owner()
        };
        assert!(written_by(&plugin_caller(&owner)));
        assert!(!written_by(&tasty_ipc::caller::CallerContext::Local));
        assert!(written_by(&plugin_caller(&owner)));
        assert!(!written_by(&plugin_caller("com.example.other")));
    }

    // 외부 작성자에서 소유 플러그인으로 바뀐 기록은 host가 읽을 때까지 유지한다.
    #[test]
    fn set_url_marks_when_the_owning_plugin_takes_the_page_back() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        state
            .test_add_markdown_tab(&mut engine, "/workspace/proj/readme.md".to_string())
            .unwrap();
        let md_sid = focused_surface_id(&state, &engine);
        let owner = plugin_caller(&remote_surface(&engine, md_sid).plugin_id.clone());
        let agent = tasty_ipc::caller::CallerContext::Local;

        let took_over_after = |caller: &tasty_ipc::caller::CallerContext| {
            assert!(set_url_as(&engine, caller, md_sid).error.is_none());
            remote_surface(&engine, md_sid).take_webview_owner_takeover()
        };
        assert!(took_over_after(&owner), "처음 쓴 페이지도 전이다");
        assert!(!took_over_after(&owner));
        assert!(!took_over_after(&agent));
        assert!(took_over_after(&owner));
        assert!(
            !remote_surface(&engine, md_sid).take_webview_owner_takeover(),
            "가져가면 내려간다"
        );
        assert!(!took_over_after(&agent));
        assert!(set_url_as(&engine, &owner, md_sid).error.is_none());
        assert!(set_url_as(&engine, &owner, md_sid).error.is_none());
        assert!(
            remote_surface(&engine, md_sid).take_webview_owner_takeover(),
            "가져가기 전의 전이는 뒤에 쓴 것이 지우지 않는다"
        );
    }

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

        assert_eq!(focused_surface_id(&state, &engine), terminal_sid);

        let resp = set_url(&engine, md_sid);
        assert!(
            resp.error.is_none(),
            "non-focused split leaf should be reachable: {:?}",
            resp.error
        );
        assert_eq!(resp.result, Some(json!({ "ok": true })));
    }

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
            let resp = set_url(&engine, sid);
            assert!(
                resp.error.is_none(),
                "leaf {sid} should be reachable: {:?}",
                resp.error
            );
        }
    }

    #[test]
    fn set_url_sole_leaf_ok_and_unknown_id_errors() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        state
            .test_add_markdown_tab(&mut engine, "/workspace/proj/readme.md".to_string())
            .unwrap();
        let md_sid = focused_surface_id(&state, &engine);
        let resp = set_url(&engine, md_sid);
        assert!(
            resp.error.is_none(),
            "sole leaf regression: {:?}",
            resp.error
        );

        let resp = set_url(&engine, 999_999);
        assert_eq!(
            resp.error.map(|e| e.message),
            Some("surface_id not found".to_string())
        );
    }

    // 수신자 판정 단독 시험과 별도로 set_url이 실제 변경 통지를 호출하는지 확인한다.
    #[test]
    fn set_url_on_markdown_surface_signals_attached_clients() {
        use tasty_ipc::stream_hub::StreamHub;

        let (state, mut engine) = crate::state::tests::test_state();
        let terminal_sid = focused_surface_id(&state, &engine);
        let md_sid = split_in_kind_surface(
            &state,
            &mut engine,
            terminal_sid,
            "markdown",
            &json!({ "file": "/workspace/proj/readme.md" }),
        );
        let hub = StreamHub::new();
        let client = hub.alloc_id();
        let rx = hub.register(client);
        engine.attach.set_notifier(hub);
        let ws_id = engine.workspaces[state.active_workspace].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[terminal_sid], &[terminal_sid, md_sid], client)
            .expect("acquire workspace");

        assert!(set_url(&engine, md_sid).error.is_none());
        let frame = rx.try_recv().expect("markdown_changed frame");
        let payload: Value = serde_json::from_slice(&frame.payload).expect("json payload");
        assert_eq!(payload["event"], "markdown_changed");
        assert_eq!(payload["surface_id"], md_sid);
        assert!(rx.try_recv().is_err(), "한 번의 set_url 에 신호는 한 번");

        assert!(set_url(&engine, terminal_sid).error.is_some());
        assert!(
            rx.try_recv().is_err(),
            "webview surface 가 아니면 신호가 없다"
        );
    }

    #[test]
    fn set_url_on_terminal_leaf_reports_not_webview() {
        let (state, mut engine) = crate::state::tests::test_state();
        let terminal_sid = focused_surface_id(&state, &engine);
        // 분할만 준비한다. 검사 대상은 기존 터미널이며 새 surface ID는 사용하지 않는다.
        split_in_kind_surface(
            &state,
            &mut engine,
            terminal_sid,
            "markdown",
            &json!({ "file": "/workspace/proj/readme.md" }),
        );
        let resp = set_url(&engine, terminal_sid);
        assert_eq!(
            resp.error.map(|e| e.message),
            Some("surface is not a webview-enabled RemoteSurface".to_string())
        );
    }
}
