//! 요청 입구에서 판정한 출처가 파일 식별과 새 탭 선택까지 유지되는지 확인한다.
//! origin을 시험에서 직접 지정하면 입구의 잘못된 사용자 판정을 잡을 수 없다.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::json;
use tasty_ipc::caller::CallerContext;

use super::handle_dispatch;
use crate::core::intent::DomainIntent;
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

/// dispatch 요청을 파일 식별 완료와 새 탭 생성까지 적용한다.
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
    crate::state::AppState,
    crate::core::CoreState,
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
    let (mut state, mut engine) = crate::state::tests::test_state();
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.file_handler.as_ref(),
        PLUGIN,
        &[json!({"id":"open", "detector":"origin-test", "priority":0,
            "action":{"kind":"open_surface", "surface_kind":"empty", "param_key":"file"}})],
    );
    if let Some((plugin, instance)) = activated {
        state
            .plugin_popup_user_activated
            .insert(instance, plugin.to_string());
    }
    let pane_id = state.active_workspace(&engine).focused_pane;
    assert_eq!(engine.find_pane_by_id(pane_id).expect("pane").active_tab, 0);
    for attempt in navigated {
        let sid = engine.workspaces[0].all_surface_ids()[0];
        attempt.record(&mut state, sid);
    }
    if with_origin_surface {
        let sid = engine.workspaces[0].all_surface_ids()[0];
        assert_eq!(engine.find_pane_for_surface(sid), Some(pane_id));
        params["origin_surface_id"] = json!(sid);
    }
    engine.workspaces[0].mirror = mirror;

    let mut out = crate::ipc::window_port::IntentOutbox::default();
    let resp = handle_dispatch(&mut out, &mut state, &engine, caller, json!(1), params);
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
    for pending in state.take_pending_intents() {
        assert!(matches!(pending.body, Intent::NewTab { .. }));
        crate::intent::tab::handle(&mut core, &mut state, &mut engine, &pending);
    }
    (dispatch_origin, intent_is_user, state, engine, pane_id)
}

fn dispatch_then_selection(
    caller: &CallerContext,
    activated: Option<(&str, u64)>,
    params: serde_json::Value,
    with_origin_surface: bool,
) -> (FileDispatchOrigin, bool, (usize, usize)) {
    let (origin, intent_is_user, _state, engine, pane_id) =
        dispatch_through(caller, activated, params, with_origin_surface, false);
    let pane = engine.find_pane_by_id(pane_id).expect("pane");
    (origin, intent_is_user, (pane.tabs.len(), pane.active_tab))
}

fn popup_params() -> serde_json::Value {
    json!({ "path": "/tmp/a.md", "owner_popup_instance": POPUP })
}

#[test]
fn a_dispatch_from_the_users_popup_selects_the_new_tab() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::User, true, (2, 1)));
}

#[test]
fn a_dispatch_without_a_popup_keeps_the_users_tab() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        json!({ "path": "/tmp/a.md" }),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

#[test]
fn an_external_caller_cannot_claim_a_popup() {
    let got = dispatch_then_selection(
        &CallerContext::Local,
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

#[test]
fn a_plugin_cannot_claim_another_plugins_popup() {
    let got = dispatch_then_selection(
        &plugin_caller("com.example.other"),
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

#[test]
fn an_untouched_popup_is_not_a_user_action() {
    let got = dispatch_then_selection(&plugin_caller(PLUGIN), None, popup_params(), false);
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

// 대상 pane을 명시해도 사용자 여부는 팝업 입력으로 판정한다.
#[test]
fn a_users_popup_that_names_an_origin_surface_still_selects_the_new_tab() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        popup_params(),
        true,
    );
    assert_eq!(got, (FileDispatchOrigin::User, true, (2, 1)));
}

#[test]
fn an_origin_surface_without_a_popup_keeps_the_users_tab() {
    let got = dispatch_then_selection(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        json!({ "path": "/tmp/a.md" }),
        true,
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 원격 실패는 사용자 요청이면 toast, 에이전트 요청이면 로그로 남겨야 한다.
#[test]
fn a_mirror_tab_from_the_users_popup_keeps_its_remote_failure_toast() {
    let (origin, _, state, engine, _) = dispatch_through(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        popup_params(),
        false,
        true,
    );
    assert_eq!(origin, FileDispatchOrigin::User);
    assert_eq!(
        engine.pending_structural_forward.len(),
        1,
        "새 탭은 forward 된다"
    );
    assert!(
        !engine.pending_structural_forward[0].silent_failure,
        "사용자가 요청한 원격 작업의 거절은 토스트로 표시한다"
    );
    assert_eq!(state.toasts.len(), 0);

    let (origin, _, _, engine, _) = dispatch_through(
        &plugin_caller(PLUGIN),
        Some((PLUGIN, POPUP)),
        json!({ "path": "/tmp/a.md" }),
        false,
        true,
    );
    assert_eq!(origin, FileDispatchOrigin::Agent);
    assert_eq!(engine.pending_structural_forward.len(), 1);
    assert!(
        engine.pending_structural_forward[0].silent_failure,
        "에이전트가 요청한 원격 작업의 거절은 로그에 기록한다"
    );
}

const NAV_URL: &str = "about:blank#tasty-nav:link:%2Ftmp%2Fa.md";

/// host 가 plugin [`PLUGIN`] 에 통지한 navigation 시도 하나.
struct Attempt {
    /// 엔진이 그 시도를 사용자 제스처로 봤는가.
    user_gesture: bool,
    /// 그 surface 의 지금 페이지를 쓴 `webview.set_url` 호출자가 [`PLUGIN`] 이었는가.
    owner_wrote_page: bool,
}

impl Attempt {
    fn record(&self, state: &mut crate::state::AppState, sid: u32) {
        crate::plugin_bridge::user_navigation::record(
            &mut state.webview_user_navigations,
            sid,
            Some(&crate::plugin_bridge::user_navigation::NavigationOwner {
                plugin_id: PLUGIN.to_string(),
                wrote_page: self.owner_wrote_page,
            }),
            &crate::webview::PendingNavigation {
                url: NAV_URL.to_string(),
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
) -> (FileDispatchOrigin, bool, (usize, usize)) {
    let (origin, intent_is_user, _state, engine, pane_id) =
        dispatch_through_with(caller, None, navigated, params, true, false);
    let pane = engine.find_pane_by_id(pane_id).expect("pane");
    (origin, intent_is_user, (pane.tabs.len(), pane.active_tab))
}

#[test]
fn a_link_the_user_clicked_in_the_plugins_webview_selects_the_new_tab() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::User, true, (2, 1)));
}

#[test]
fn a_navigation_the_engine_did_not_see_as_a_gesture_is_not_a_user_action() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[gesture(false)], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 뒤따른 비사용자 navigation이 이전 클릭 기록을 지워야 한다.
#[test]
fn a_non_gesture_attempt_after_an_unused_click_leaves_the_dispatch_an_agent_request() {
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
        assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
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
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

#[test]
fn a_plugin_cannot_claim_a_navigation_that_never_happened() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

#[test]
fn an_external_caller_cannot_claim_a_webview_navigation() {
    let got = link_then_selection(&CallerContext::Local, &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

#[test]
fn a_plugin_cannot_claim_another_plugins_webview_navigation() {
    let got = link_then_selection(
        &plugin_caller("com.example.other"),
        &[gesture(true)],
        link_params(),
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 같은 프레임에 페이지 작성자가 바뀌면 navigation 기록을 버린다. 다음 프레임의 클릭은 허용한다.
#[test]
fn a_click_drained_in_the_frame_the_owner_took_the_page_back_is_not_a_user_action() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    let mut params = link_params();
    params["origin_surface_id"] = json!(sid);
    let mut origins = Vec::new();
    for (id, owner_took_over) in [(0, true), (1, false)] {
        gesture(true).record(&mut state, sid);
        crate::plugin_bridge::user_navigation::settle_frame(
            &mut state.webview_user_navigations,
            sid,
            owner_took_over,
        );
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let resp = handle_dispatch(
            &mut out,
            &mut state,
            &engine,
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
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    gesture(true).record(&mut state, sid);
    let mut params = link_params();
    params["origin_surface_id"] = json!(sid);
    let mut origins = Vec::new();
    for id in 0..2 {
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let resp = handle_dispatch(
            &mut out,
            &mut state,
            &engine,
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
