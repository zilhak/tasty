use super::*;
use tasty_model::html_script::{Fingerprint, ScriptDetection, ScriptScan};

/// 플러그인 프로세스 없이 html kind를 등록하고 html 탭을 연다.
fn state_with_html_tab() -> (crate::state::AppState, crate::core::CoreState, u32) {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(json!({
        "kind": "html",
        "display_name_i18n_key": "surface.kind.html",
        "rendering": "webview",
    }))
    .expect("html SurfaceKindDecl");
    let (host_cmd_tx, _host_cmd_rx) = std::sync::mpsc::channel();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.surface_registry,
        "com.tasty.html",
        &decl,
        host_cmd_tx,
    );
    let (_, sid) = state
        .add_kind_tab(&mut engine, "html", &json!({ "file": "/docs/a.html" }))
        .expect("html tab");
    (state, engine, sid)
}

fn html_script_of(engine: &crate::core::CoreState, sid: u32) -> JsonRpcResponse {
    handle_html_script(engine, json!(1), &json!({ "surface_id": sid }))
}

fn html_surface(engine: &crate::core::CoreState, sid: u32) -> &RemoteSurface {
    engine
        .find_surface_by_id(sid)
        .and_then(|s| s.as_any().downcast_ref::<RemoteSurface>())
        .expect("html remote surface")
}

#[test]
fn reports_detection_and_a_pending_banner_without_changing_it() {
    let (_state, engine, sid) = state_with_html_tab();
    let rs = html_surface(&engine, sid);
    rs.with_html_script(|st| {
        st.on_host_load_requested();
        st.on_load_started();
        st.on_main_response(
            "file:///docs/a.html",
            Some(ScriptScan {
                fingerprint: Fingerprint([0xab; 32]),
                detection: ScriptDetection::Scripts,
            }),
        );
        st.on_committed(Some("file:///docs/a.html"));
        st.on_load_finished();
        st.update_banner();
    });
    let before = rs.with_html_script(|st| st.clone());

    let resp = html_script_of(&engine, sid);
    let r = resp.result.expect("result");
    assert_eq!(r["surface_id"], sid);
    assert_eq!(r["sandbox"], true);
    assert_eq!(r["document"]["url"], "file:///docs/a.html");
    assert_eq!(r["document"]["detection"], "scripts");
    assert_eq!(r["document"]["fingerprint"], "ab".repeat(32));
    assert_eq!(r["allowed"], false);
    assert_eq!(r["javascript"], false);
    assert!(r["allowance"].is_null());
    assert_eq!(r["banner"]["phase"], "hidden");
    assert_eq!(r["banner"]["pending_view"], true);
    assert_eq!(r["banner"]["viewed"], false);
    assert_eq!(
        rs.with_html_script(|st| st.clone()),
        before,
        "조회가 상태를 바꿨다"
    );
}

#[test]
fn a_surface_without_a_document_reports_null_document() {
    let (_state, engine, sid) = state_with_html_tab();
    let r = html_script_of(&engine, sid).result.expect("result");
    assert!(r["document"].is_null());
    assert_eq!(r["banner"]["phase"], "hidden");
}

#[test]
fn rejects_a_missing_surface_and_a_non_html_surface() {
    let (mut state, mut engine, _sid) = state_with_html_tab();
    let err = html_script_of(&engine, 999_999)
        .error
        .expect("missing surface");
    assert!(err.message.contains("not found"), "{}", err.message);

    state
        .test_add_markdown_tab(&mut engine, "/docs/readme.md".to_string())
        .expect("markdown tab");
    let md_sid = state.focused_surface_id(&engine).expect("focused");
    let err = html_script_of(&engine, md_sid)
        .error
        .expect("markdown surface");
    assert!(
        err.message.contains("not an html surface"),
        "{}",
        err.message
    );
}
