//! `file_handler.dispatch` **입구**가 발화 주체를 새 탭의 선택까지 그대로 나르는지 고정한다.
//!
//! 새 탭 핸들러의 판정(사용자면 선택, 아니면 그대로)은 `intent::headless` 시험이 origin 을 직접
//! 주입해 잰다. 그 시험은 입구가 무엇을 싣는지 안 본다 — 입구가 사용자 popup 을 못 알아보거나
//! 외부 호출자의 주장을 믿으면 그 시험은 초록인 채로 사용자 상태가 움직인다. 그래서 여기서는
//! 요청을 `handle_dispatch` 에 넣고, 그 intent 를 identify 완료 적용과 새 탭 핸들러까지 실제로
//! 흘린 뒤 사용자가 보던 탭을 본다(ADR-0526).

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

/// `params` 로 `file_handler.dispatch` 를 부르고, 그 결과를 identify 완료 적용 → 새 탭 핸들러까지
/// 흘린다. 돌려주는 값은 입구가 정한 발화 주체 · 발화 intent 가 사용자인가 · 그 뒤의 상태다.
///
/// `activated` 가 `Some((plugin, instance))` 면 그 popup 이 사용자의 확정형 입력을 받은 상태로
/// 시작한다. `with_origin_surface` 면 focused pane 의 surface 를 `origin_surface_id` 로 더 싣는다 —
/// 그때 새 탭은 `Intent::NewTab` 이 아니라 명시 origin 갈래(`open_surface_tab` 의 `Some(pane)`)로 선다.
/// `mirror` 면 그 워크스페이스를 mirror 로 두고 시작한다 — 새 탭은 원격으로 forward 된다.
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

/// [`dispatch_through`] 에 webview 근거를 더한 것. `navigated` 의 시도마다 host 가 focused pane 의
/// 첫 surface 에서 난 그 시도를 plugin [`PLUGIN`] 에 통지한 상태로 시작한다 — 통지 자리가 부르는
/// 기록 함수를 순서대로 그대로 부른다. 엔진이 그 시도를 사용자 제스처로 봤는지와 그 surface 의
/// 페이지를 소유 plugin 이 썼는지는 [`Attempt`] 가 정한다.
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

    // `App::handle_identify_done` 이 워커 결과로 부르는 적용 — detector 는 위 핸들러의 것이다.
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

/// [`dispatch_through`] 를 로컬 워크스페이스에서 돌리고 focused pane 의 `(탭 수, 활성 탭)` 을 본다.
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

/// 외부 IPC 호출자는 같은 키를 실어도 사용자가 될 수 없다 — 요청 값만으로는 모자란다.
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

/// 다른 plugin 의 popup 을 대도 사용자가 아니다.
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

/// 사용자가 만지지 않은(확정형 입력을 안 받은 · 닫혀 걷힌) popup 은 근거가 못 된다.
#[test]
fn an_untouched_popup_is_not_a_user_action() {
    let got = dispatch_then_selection(&plugin_caller(PLUGIN), None, popup_params(), false);
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 사용자 popup 이 열 pane 을 `origin_surface_id` 로 함께 대도 사용자다 — 발화 주체는 그 키가
/// 아니라 popup 판정이 정하므로, 명시 origin 갈래에서도 새 탭이 선택된다.
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

/// `origin_surface_id` 만으로는 사용자가 아니다 — popup 근거 없이 pane 을 대면 에이전트다.
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

/// mirror 워크스페이스에서 사용자 popup 으로 연 새 탭은 원격으로 forward 되고, 그 op 는 원격 거절이
/// **toast 로 가도록**(표시 없음) 남는다. 같은 요청이 popup 근거 없이 오면 로그로 가도록 표시된다
/// (ADR-0503 의 `silent_failure`). 입구가 사용자 popup 을 못 알아보면 앞쪽 단언이 깨진다.
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
        "사용자 발화 op 의 원격 거절은 toast 로 간다"
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
        "에이전트 발화 op 의 원격 거절은 로그로 간다"
    );
}

// ── webview 근거 (ADR-0568) ────────────────────────────────────────────────────────────────

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

/// [`dispatch_through_with`] 를 origin surface 와 함께 돌리고 focused pane 의 `(탭 수, 활성 탭)` 을 본다.
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

/// 사용자가 plugin webview 안의 링크를 눌러 연 파일은 사용자 행동이다 — 새 탭이 선택된다.
#[test]
fn a_link_the_user_clicked_in_the_plugins_webview_selects_the_new_tab() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::User, true, (2, 1)));
}

/// 사람의 입력 없이 스크립트만으로 낸 navigation(엔진이 사용자 제스처로 안 본 것)을 근거로 대도
/// 에이전트다.
#[test]
fn a_navigation_the_engine_did_not_see_as_a_gesture_is_not_a_user_action() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[gesture(false)], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 사용자의 클릭이 기록만 남기고 쓰이지 않은 뒤(예: 링크 대상 파일이 없었다), 같은 surface 에서
/// 제스처가 아닌 시도가 오면 그 기록은 사라진다. 에이전트가 `webview.set_url` 로 쓴 스크립트가 같은
/// URL 의 navigation 을 내고 plugin 이 그것을 되대도 사용자 행동이 되지 않는다.
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

/// 소유 plugin 이 아닌 호출자가 `webview.set_url` 로 쓴 페이지 위에서 사람이 누른 클릭은 사용자
/// 행동이 아니다 — 무엇을 열지는 그 페이지를 쓴 쪽이 정했다.
#[test]
fn a_click_on_a_page_the_owner_did_not_write_is_not_a_user_action() {
    let on_agents_page = Attempt {
        user_gesture: true,
        owner_wrote_page: false,
    };
    let got = link_then_selection(&plugin_caller(PLUGIN), &[on_agents_page], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// host 가 통지한 적 없는 URL 을 대도 에이전트다 — 요청 값만으로는 모자란다.
#[test]
fn a_plugin_cannot_claim_a_navigation_that_never_happened() {
    let got = link_then_selection(&plugin_caller(PLUGIN), &[], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 외부 IPC 호출자는 실재하는 사용자 navigation 의 URL 을 실어도 사용자가 아니다.
#[test]
fn an_external_caller_cannot_claim_a_webview_navigation() {
    let got = link_then_selection(&CallerContext::Local, &[gesture(true)], link_params());
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 다른 plugin 의 webview 에서 난 사용자 navigation 은 근거가 못 된다.
#[test]
fn a_plugin_cannot_claim_another_plugins_webview_navigation() {
    let got = link_then_selection(
        &plugin_caller("com.example.other"),
        &[gesture(true)],
        link_params(),
    );
    assert_eq!(got, (FileDispatchOrigin::Agent, false, (2, 0)));
}

/// 근거는 한 번 쓰면 사라진다 — 같은 클릭을 두 번 대면 두 번째는 에이전트다.
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
