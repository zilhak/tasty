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
