//! 요청 입구에서 판정한 출처가 파일 식별과 새 탭 생성 요청까지 유지되는지 확인한다.
//! origin을 시험에서 직접 지정하면 입구의 잘못된 사용자 판정을 잡을 수 없다.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::json;
use tasty_ipc::caller::CallerContext;

use super::handle_dispatch;
use crate::app::command::DomainIntent;
use crate::file::dispatch::FileDispatchOrigin;
use crate::file::format::DetectorId;
use crate::intent::Intent;

const PLUGIN: &str = "com.example.popup";
const POPUP: u64 = 7;

fn plugin_caller(plugin_id: &str) -> CallerContext {
    CallerContext::Plugin {
        plugin_id: plugin_id.to_string(),
        permissions: Arc::new(HashSet::new()),
    }
}

/// dispatch 요청을 파일 식별 완료와 새 탭 생성 요청까지 적용한다.
/// activated는 확정 입력을 받은 팝업, with_origin_surface는 명시적 대상 pane, mirror는 원격 전달을 준비한다.
fn dispatch_through(
    caller: &CallerContext,
    activated: Option<(&str, u64)>,
    params: serde_json::Value,
    with_origin_surface: bool,
    mirror: bool,
) -> DispatchOutcome {
    dispatch_through_with(caller, activated, &[], params, with_origin_surface, mirror)
}

type DispatchOutcome = (
    FileDispatchOrigin,
    bool,
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
    u32,
);

/// webview의 navigation 기록도 준비한다. 제스처 여부와 페이지 소유자는 Attempt로 지정한다.
fn dispatch_through_with(
    caller: &CallerContext,
    activated: Option<(&str, u64)>,
    navigated: &[Attempt],
    mut params: serde_json::Value,
    with_origin_surface: bool,
    mirror: bool,
) -> DispatchOutcome {
    use tasty_plugin_protocol::host_port::FileHandlerRegistryPort;
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine_session) = if mirror {
        crate::state::tests::test_mirror_state()
    } else {
        crate::state::tests::test_state()
    };
    let mut engine = engine_session.borrow_mut();
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.runtime.file_handler.as_ref(),
        PLUGIN,
        &[json!({"id":"open", "detector":"origin-test", "priority":0,
            "action":{"kind":"open_surface", "surface_kind":"empty", "param_key":"file"}})],
    );
    if let Some((plugin, instance)) = activated {
        state
            .plugin_popup_user_activated
            .insert(instance, plugin.to_string());
    }
    let pane_id = state.focused_pane_id(&engine);
    assert_eq!(
        state
            .navigation
            .tab_index(engine.find_pane_by_id(pane_id).expect("pane")),
        0
    );
    let view = Arc::new(());
    state.webview_identity = Arc::downgrade(&view);
    let proofs = Arc::new(crate::app::html_runtime::NavigationProofs::default());
    let sid = engine.workspace_at(0).unwrap().all_surface_ids()[0];
    install_source(&mut engine, sid);
    for attempt in navigated {
        attempt.record(&proofs, &state, &engine, sid);
    }
    if with_origin_surface {
        let sid = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        assert_eq!(engine.find_pane_for_surface(sid), Some(pane_id));
        params["origin_surface_id"] = json!(sid);
    }

    let mut out = crate::ipc::window_port::IntentOutbox::default();
    let mut scope = crate::ipc::request_scope::RequestScope::capture(
        &mut state,
        engine.core,
        Some(proofs.clone()),
    );
    let resp = handle_dispatch(
        &mut out,
        &mut scope,
        &engine.as_ref(),
        caller,
        json!(1),
        params,
    );
    drop(scope);
    assert!(resp.error.is_none(), "dispatch must be accepted: {resp:?}");
    let mut emitted = out.into_vec();
    assert_eq!(emitted.len(), 1);
    let dispatched = emitted.remove(0);
    let intent_is_user = dispatched.origin.is_user();
    let Intent::Domain(DomainIntent::DispatchFile {
        target,
        origin_surface_id,
        dispatch_origin,
        ignore_size_limit,
        ..
    }) = dispatched.body
    else {
        panic!("file_handler.dispatch 가 DispatchFile 외 intent 를 냈다");
    };

    crate::file::dispatch::apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        target,
        Some(DetectorId::new("origin-test")),
        origin_surface_id,
        dispatch_origin,
        ignore_size_limit,
    );
    // App receives creation requests; the View does not synchronously create/select a tab.
    assert_eq!(engine.find_pane_by_id(pane_id).unwrap().tabs.len(), 1);
    assert_eq!(
        state
            .navigation
            .tab_index(engine.find_pane_by_id(pane_id).unwrap()),
        0
    );
    (
        dispatch_origin,
        intent_is_user,
        state,
        engine_session,
        pane_id,
    )
}

fn dispatch_then_selection(
    caller: &CallerContext,
    activated: Option<(&str, u64)>,
    params: serde_json::Value,
    with_origin_surface: bool,
) -> (FileDispatchOrigin, bool, bool) {
    let (origin, intent_is_user, mut state, _engine_session, pane_id) =
        dispatch_through(caller, activated, params, with_origin_surface, false);
    (
        origin,
        intent_is_user,
        creation_activation(&mut state, pane_id),
    )
}

fn popup_params() -> serde_json::Value {
    json!({ "path": "/tmp/a.md", "owner_popup_instance": POPUP })
}

#[test]
fn a_dispatch_from_the_users_popup_requests_activation_of_the_new_tab() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::User, true, true));
}

#[test]
fn a_dispatch_without_a_popup_requests_background_creation() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        json!({ "path": "/tmp/a.md" }),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

#[test]
fn an_external_caller_cannot_claim_a_popup() {
    let got = dispatch_then_selection(
        &CallerContext::Local,
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, false));
}

#[test]
fn a_plugin_cannot_claim_another_plugins_popup() {
    let got = dispatch_then_selection(
        &plugin_caller("com.example.other"),
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

#[test]
fn an_untouched_popup_is_not_a_user_action() {
    let got = dispatch_then_selection(&plugin_caller(PLUGIN), None, popup_params(), false);
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

// 대상 pane을 명시해도 사용자 여부는 팝업 입력으로 판정한다.
#[test]
fn a_users_popup_that_names_an_origin_surface_still_requests_activation_of_the_new_tab() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        popup_params(),
        true,
    );
    assert_eq!(got, (FileDispatchOrigin::User, true, true));
}

#[test]
fn an_origin_surface_without_a_popup_requests_background_creation() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        json!({ "path": "/tmp/a.md" }),
        true,
    );
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

const NAV_URL: &str = "about:blank#tasty-nav:link:%2Ftmp%2Fa.md";

/// host 가 plugin [`PLUGIN`] 에 통지한 navigation 시도 하나.
struct Attempt {
    /// 엔진이 그 시도를 사용자 제스처로 봤는가.
    user_gesture: bool,
    /// 그 surface 의 지금 페이지를 쓴 `webview.set_url` 호출자가 [`PLUGIN`] 이었는가.
    owner_wrote_page: bool,
}

fn install_source(engine: &mut crate::runtime::engine_access::EngineMut<'_>, sid: u32) {
    engine.runtime.surfaces.insert(
        sid,
        Box::new(crate::plugin_bridge::remote_surface::RemoteSurface::new(
            sid,
            "markdown",
            PLUGIN.into(),
            "source".into(),
        )),
    );
}

fn creation_activation(state: &mut crate::state::RequestContext, pane_id: u32) -> bool {
    let intents = state.take_pending_intents();
    assert_eq!(intents.len(), 1);
    match &intents[0].body {
        Intent::Domain(DomainIntent::CreateTab {
            pane_id: target,
            activate,
            ..
        }) => {
            assert_eq!(*target, pane_id);
            assert_eq!(*activate, intents[0].origin.is_user());
            *activate
        }
        Intent::NewTab { .. } => intents[0].origin.is_user(),
        other => panic!("expected creation request: {other:?}"),
    }
}

impl Attempt {
    fn record(
        &self,
        proofs: &crate::app::html_runtime::NavigationProofs,
        state: &crate::state::RequestContext,
        engine: &crate::runtime::engine_access::EngineMut<'_>,
        sid: u32,
    ) {
        let remote = engine
            .runtime
            .surfaces
            .get(&sid)
            .unwrap()
            .as_any()
            .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
            .unwrap();
        proofs.record(
            &state.webview_identity,
            sid,
            Some(&crate::plugin_bridge::user_navigation::NavigationOwner {
                plugin_id: PLUGIN.into(),
                wrote_page: self.owner_wrote_page,
            }),
            Some(&remote.webview_url),
            &crate::webview::PendingNavigation {
                url: NAV_URL.into(),
                user_gesture: self.user_gesture,
            },
        );
    }
}

/// 소유 plugin 이 쓴 페이지 위에서 엔진이 `user_gesture` 로 본 시도.
fn gesture(user_gesture: bool) -> Attempt {
    Attempt {
        user_gesture,
        owner_wrote_page: true,
    }
}

fn link_params() -> serde_json::Value {
    json!({ "path": "/tmp/a.md", "user_navigation_url": NAV_URL })
}

fn link_then_selection(
    caller: &CallerContext,
    navigated: &[Attempt],
    params: serde_json::Value,
) -> (FileDispatchOrigin, bool, bool) {
    let (origin, intent_is_user, mut state, _owner, pane_id) =
        dispatch_through_with(caller, None, navigated, params, true, false);
    (
        origin,
        intent_is_user,
        creation_activation(&mut state, pane_id),
    )
}

#[test]
fn a_link_the_user_clicked_in_the_plugins_webview_requests_activation_of_the_new_tab() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::User, true, true));
}

#[test]
fn a_navigation_the_engine_did_not_see_as_a_gesture_is_not_a_user_action() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[gesture(false)], link_params());
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

/// 뒤따른 비사용자 navigation이 이전 클릭 기록을 지워야 한다.
#[test]
fn a_non_gesture_attempt_after_an_unused_click_leaves_the_dispatch_unverified() {
    let agents_script = Attempt {
        user_gesture: false,
        owner_wrote_page: false,
    };
    for later in [gesture(false), agents_script] {
        let got = link_then_selection(
            &plugin_caller(PLUGIN),
            &[gesture(true), later],
            link_params(),
        );
        assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
    }
}

/// 다른 호출자가 쓴 페이지의 클릭은 소유 플러그인의 사용자 행동으로 인정하지 않는다.
#[test]
fn a_click_on_a_page_the_owner_did_not_write_is_not_a_user_action() {
    let on_agents_page = Attempt {
        user_gesture: true,
        owner_wrote_page: false,
    };
    let got = link_then_selection(&plugin_caller(PLUGIN), &[on_agents_page], link_params());
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

#[test]
fn a_plugin_cannot_claim_a_navigation_that_never_happened() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[], link_params());
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

#[test]
fn an_external_caller_cannot_claim_a_webview_navigation() {
    let got = link_then_selection(&CallerContext::Local, &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, false));
}

/// 입구 판정부터 식별 결과 적용까지 이어서 본다. 매칭 핸들러가 없을 때 fallback picker는
/// 증명하지 못한 plugin 중계 요청에만 열리고 외부 IPC 요청에는 열리지 않는다.
#[test]
fn only_an_unverified_plugin_request_opens_the_fallback_picker() {
    for (caller, opens) in [(plugin_caller(PLUGIN), true), (CallerContext::Local, false)] {
        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let mut scope =
            crate::ipc::request_scope::RequestScope::capture(&mut state, engine.core, None);
        let resp = handle_dispatch(
            &mut out,
            &mut scope,
            &engine.as_ref(),
            &caller,
            json!(1),
            json!({ "path": "/tmp/unmatched.unknown" }),
        );
        drop(scope);
        assert!(resp.error.is_none(), "dispatch must be accepted: {resp:?}");
        let Intent::Domain(DomainIntent::DispatchFile {
            target,
            origin_surface_id,
            dispatch_origin,
            ignore_size_limit,
            ..
        }) = out.into_vec().remove(0).body
        else {
            panic!("file_handler.dispatch 가 DispatchFile 외 intent 를 냈다");
        };
        crate::file::dispatch::apply_identify_result(
            &mut core,
            &mut state,
            &mut engine,
            target,
            None,
            origin_surface_id,
            dispatch_origin,
            ignore_size_limit,
        );
        assert_eq!(
            state.dialogs.file_handler_picker.is_some(),
            opens,
            "{caller:?}"
        );
    }
}

/// 세션 에이전트도 외부 IPC라 팝업이나 navigation 기록이 있어도 picker를 띄울 수 없는 Agent다.
#[test]
fn a_session_agent_caller_stays_an_agent_request() {
    let agent = CallerContext::Agent {
        agent_id: "child:1".to_string(),
        permissions: Arc::new(HashSet::new()),
    };
    let got = dispatch_then_selection(&agent, Some((PLUGIN, POPUP)), popup_params(), false);
    assert_eq!(got, (FileDispatchOrigin::Agent, false, false));
    let got = link_then_selection(&agent, &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, false));
}

#[test]
fn a_plugin_cannot_claim_another_plugins_webview_navigation() {
    let got = link_then_selection(
        &plugin_caller("com.example.other"),
        &[gesture(true)],
        link_params(),
    );
    assert_eq!(got, (FileDispatchOrigin::PluginUnverified, false, false));
}

/// 같은 프레임에 페이지 작성자가 바뀌면 navigation 기록을 버린다. 다음 프레임의 클릭은 허용한다.
#[test]
fn a_click_drained_in_the_frame_the_owner_took_the_page_back_is_not_a_user_action() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let view = Arc::new(());
    state.webview_identity = Arc::downgrade(&view);
    let proofs = Arc::new(crate::app::html_runtime::NavigationProofs::default());
    let sid = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    install_source(&mut engine, sid);
    let mut params = link_params();
    params["origin_surface_id"] = json!(sid);
    let mut origins = Vec::new();
    for (id, owner_took_over) in [(0, true), (1, false)] {
        gesture(true).record(&proofs, &state, &engine, sid);
        proofs.settle_frame(&state.webview_identity, &[sid], &[(sid, owner_took_over)]);
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let mut scope = crate::ipc::request_scope::RequestScope::capture(
            &mut state,
            engine.core,
            Some(proofs.clone()),
        );
        let resp = handle_dispatch(
            &mut out,
            &mut scope,
            &engine.as_ref(),
            &plugin_caller(PLUGIN),
            json!(id),
            params.clone(),
        );
        assert!(resp.error.is_none(), "dispatch must be accepted: {resp:?}");
        let emitted = out.into_vec();
        assert_eq!(emitted.len(), 1);
        origins.push(emitted[0].origin.is_user());
    }
    assert_eq!(origins, vec![false, true]);
}

#[test]
fn a_webview_navigation_backs_only_one_dispatch() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let view = Arc::new(());
    state.webview_identity = Arc::downgrade(&view);
    let proofs = Arc::new(crate::app::html_runtime::NavigationProofs::default());
    let sid = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    install_source(&mut engine, sid);
    gesture(true).record(&proofs, &state, &engine, sid);
    let mut params = link_params();
    params["origin_surface_id"] = json!(sid);
    let mut origins = Vec::new();
    for id in 0..2 {
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let mut scope = crate::ipc::request_scope::RequestScope::capture(
            &mut state,
            engine.core,
            Some(proofs.clone()),
        );
        let resp = handle_dispatch(
            &mut out,
            &mut scope,
            &engine.as_ref(),
            &plugin_caller(PLUGIN),
            json!(id),
            params.clone(),
        );
        assert!(resp.error.is_none(), "dispatch must be accepted: {resp:?}");
        let emitted = out.into_vec();
        assert_eq!(emitted.len(), 1);
        origins.push(emitted[0].origin.is_user());
    }
    assert_eq!(origins, vec![true, false]);
}

/// mirror origin은 이름으로만 식별하지만 응답 depth는 요청값을 그대로 돌려준다. 응답 필드의 의미는 바꾸지 않는다.
#[test]
fn a_mirror_origin_dispatch_echoes_the_requested_depth() {
    for (mirror, expected) in [(false, "deep"), (true, "deep")] {
        let (mut state, mut engine_session) = if mirror {
            crate::state::tests::test_mirror_state()
        } else {
            crate::state::tests::test_state()
        };
        let mut engine = engine_session.borrow_mut();
        let sid = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let mut scope =
            crate::ipc::request_scope::RequestScope::capture(&mut state, engine.core, None);
        let resp = handle_dispatch(
            &mut out,
            &mut scope,
            &engine.as_ref(),
            &CallerContext::Local,
            json!(1),
            json!({"path": "/remote/doc.md", "depth": "deep", "origin_surface_id": sid}),
        );
        let result = resp.result.expect("dispatch accepted");
        assert_eq!(result["depth"], expected, "mirror={mirror}");
    }
}
