//! X11 raw 키 누름의 입력 소유자(`raw_key_owner`). raw 키는 창의 키 사건 경로를 거치지 않으므로 소유자를 고르기 전에
//! 포커스를 정규화한다.

use super::{InputOwner, raw_key_owner};
use crate::adapters::ui::popup::focus::PopupNode;
use crate::adapters::ui::popup::{PopupInstanceId, PopupScope, defs};
use crate::state::RequestContext;

/// 고른 항목이 없는 탐색기가 포커스이고 알림 목록이 포커스를 가진 채 열린 장면.
fn scene() -> (
    RequestContext,
    crate::runtime::engine_session::EngineSession,
    u32,
    PopupInstanceId,
) {
    crate::i18n::init("en");
    let (mut state, mut session, sid, root) = crate::state::tests::explorer_fixture(false);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let notifications = state
        .popups
        .open_request(
            &crate::intent::OpenRequest::new("notifications", PopupScope::Window).focused(),
            true,
        )
        .expect("알림 목록이 열린다");
    {
        let engine = session.borrow_mut();
        state.normalize_focus(&engine.read());
    }
    state
        .explorer_views
        .get_or_init(&crate::model::ExplorerPanel::new(sid, root), None);
    (state, session, sid, notifications)
}

/// Given 포커스 칸의 host 팝업이 다음 그리기 전에 부모 버튼이 대표하는 자식(`Collapsed(None)`)이 됐을 때, When X11 raw
/// 키 누름의 소유자를 고르면, Then 정규화가 그 포커스를 비워 포커스된 탐색기 목록이 소유자다.
#[test]
fn a_raw_key_under_a_hidden_child_focus_goes_to_the_explorer_list() {
    let (mut state, mut session, sid, notifications) = scene();
    state.popups.hide_behind_parent_button(notifications);
    state.popups.set_focus(Some(PopupNode::Host(notifications)));
    assert!(
        state.popups.has_focused(),
        "정규화 전 관리자 칸은 그 팝업이다"
    );
    let engine = session.borrow_mut();
    assert_eq!(
        raw_key_owner(&mut state, &engine.read()),
        Some(InputOwner::List(sid))
    );
}

/// 대조: Given 같은 장면에서 알림 목록이 키를 받을 수 있는 포커스 노드일 때, When raw 키 소유자를 고르면, Then 창 키가
/// 팝업으로 가므로 소유자가 없다.
#[test]
fn a_raw_key_under_a_live_popup_focus_has_no_explorer_owner() {
    let (mut state, mut session, _, _) = scene();
    let engine = session.borrow_mut();
    assert_eq!(raw_key_owner(&mut state, &engine.read()), None);
}

/// 대조: Given 팝업 포커스가 없는 같은 장면, When raw 키 소유자를 고르면, Then 포커스된 탐색기 목록이 소유자다.
#[test]
fn a_raw_key_without_popup_focus_goes_to_the_explorer_list() {
    let (mut state, mut session, sid, _) = scene();
    state.popups.set_focus(None);
    let engine = session.borrow_mut();
    assert_eq!(
        raw_key_owner(&mut state, &engine.read()),
        Some(InputOwner::List(sid))
    );
}

/// 순서 6 앞 식: 창 키가 오버레이로 가지 않을 때 포커스된 explorer.
fn overlay_rule(
    state: &crate::state::MainViewState,
    read: &crate::runtime::engine_read::EngineRead<'_>,
) -> Option<u32> {
    (!state.window_keys_to_overlay())
        .then(|| crate::adapters::ui::input::shortcuts::focused_explorer_surface_id(state, read))
        .flatten()
}

/// C3 항별 표: 정규화한 장면마다 IME explorer surface(`ime_explorer_surface`)가 앞 식(`window_keys_to_overlay`)과 같다.
/// 항은 팝업 없음, 포커스 host 팝업, 몫 결속, plugin 키 주인, 무대, 설정 열기 요청, 튜토리얼 말풍선이다.
#[test]
fn the_ime_explorer_surface_matches_the_overlay_rule_for_every_gate_term() {
    type Setup = fn(&mut crate::state::MainViewState, PopupInstanceId);
    let cases: [(&str, Setup); 7] = [
        ("no popup focus", |s, _| s.popups.set_focus(None)),
        ("host popup focus", |_, _| {}),
        ("bound owner", |s, iid| {
            s.popups.set_focus(None);
            s.popups.bind_input_key_owner(PopupNode::Host(iid));
        }),
        ("plugin key owner", |s, _| {
            s.plugin_popup_key_candidates
                .push((42, tasty_host_plugin::next_popup_z_seq()));
            s.plugin_popup_bound_scopes
                .insert(42, ("p".to_string(), Some(PopupScope::Window)));
            s.popups.set_focus(Some(PopupNode::Plugin(42)));
        }),
        ("stage", |s, _| {
            s.popups.set_focus(None);
            assert!(s.open_fullscreen_stage("blank"));
        }),
        ("settings request", |s, _| {
            s.popups.set_focus(None);
            s.settings_open_requested = true;
        }),
        ("tutorial", |s, _| {
            s.popups.set_focus(None);
            s.tutorial.request_start(0);
            s.tutorial.start_pending();
        }),
    ];
    let mut allowed = 0;
    for (name, setup) in cases {
        let (mut state, mut session, sid, notifications) = scene();
        setup(&mut state, notifications);
        let engine = session.borrow_mut();
        let read = engine.read();
        state.normalize_focus(&read);
        let now = super::ime_explorer_surface(&state, &read);
        assert_eq!(now, overlay_rule(&state, &read), "{name}");
        if now == Some(sid) {
            allowed += 1;
        }
    }
    assert_eq!(
        allowed, 1,
        "팝업 포커스가 없는 장면만 explorer 가 IME 를 받는다"
    );
}

/// C2 표: rename 소유자는 포커스 노드가 rename host 인스턴스일 때만이다. 포커스 노드가 rename 이 아닌 host 팝업(알림
/// 목록)이거나 팝업 포커스가 없으면 소유자가 없다.
#[test]
fn the_rename_owner_needs_the_rename_focus_node() {
    let (mut state, mut session, _, _) = scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    assert!(matches!(
        state.focus_node(&read),
        crate::state::focus_node::FocusNode::Popup(PopupNode::Host(_))
    ));
    assert_eq!(super::rename_input_owner(&state, &read), None);
    state.popups.set_focus(None);
    state.normalize_focus(&read);
    assert_eq!(super::rename_input_owner(&state, &read), None);
}
