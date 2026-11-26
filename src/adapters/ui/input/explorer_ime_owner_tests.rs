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

/// 안쪽 컨트롤이 키보드 포커스를 가진 복원 안내 하나를 연다.
fn focused_notice(state: &mut crate::state::MainViewState) {
    crate::adapters::ui::restore_notice::open_for_test(
        state,
        crate::adapters::ui::restore_notice::RestoreNoticeEntry {
            id: 1,
            title: "title".into(),
            cause: "cause".into(),
            label: "label".into(),
            path: "/a".into(),
            body: "body".into(),
        },
        true,
    );
    assert!(crate::adapters::ui::restore_notice::has_keyboard_focus(
        &state.popups
    ));
}

/// 탐색기 `sid` 의 항목 이름 바꾸기 입력 대화상자를 포커스를 주며 연다.
fn open_rename(
    state: &mut crate::state::MainViewState,
    read: &crate::runtime::engine_read::EngineRead<'_>,
    sid: u32,
    scope: PopupScope,
) {
    crate::adapters::ui::rename_popup::open_for_test(
        state,
        crate::adapters::ui::rename_popup::RenamePayload::new(
            crate::state::RenameTarget::ExplorerEntry {
                surface_id: sid,
                path: "owned-fixture.txt".into(),
                binding: crate::runtime::surface_binding::SurfaceBinding::capture(read, sid)
                    .expect("binding"),
            },
            String::new(),
        ),
        scope,
        None,
    )
    .expect("이름 바꾸기가 열린다");
}

/// plugin 팝업 42 를 창 범위 키 후보로 둔다.
fn plugin_candidate(state: &mut crate::state::MainViewState) {
    state
        .plugin_popup_key_candidates
        .push((42, tasty_host_plugin::next_popup_z_seq()));
    state
        .plugin_popup_bound_scopes
        .insert(42, ("p".to_string(), Some(PopupScope::Window)));
}

/// C3 항별 표: 정규화한 장면마다 IME explorer surface(`ime_explorer_surface`)가 앞 식(`window_keys_to_overlay`)과 같다.
/// 항은 팝업 없음, 포커스 host 팝업, host 몫 결속, plugin 몫 결속, plugin 키 주인, 복원 안내 안쪽 키 포커스, 이름 바꾸기
/// 입력 대화상자, 주소 띠, 무대, 설정 열기 요청, 튜토리얼 말풍선이다.
#[test]
fn the_ime_explorer_surface_matches_the_overlay_rule_for_every_gate_term() {
    type Setup = fn(
        &mut crate::state::MainViewState,
        &crate::runtime::engine_read::EngineRead<'_>,
        u32,
        PopupInstanceId,
    );
    let cases: [(&str, Setup); 11] = [
        ("no popup focus", |s, _, _, _| s.popups.set_focus(None)),
        ("host popup focus", |_, _, _, _| {}),
        ("host bound owner", |s, _, _, iid| {
            s.popups.set_focus(None);
            s.popups.bind_input_key_owner(PopupNode::Host(iid));
        }),
        ("plugin bound owner", |s, _, _, _| {
            s.popups.set_focus(None);
            plugin_candidate(s);
            s.popups.bind_input_key_owner(PopupNode::Plugin(42));
        }),
        ("plugin key owner", |s, _, _, _| {
            plugin_candidate(s);
            s.popups.set_focus(Some(PopupNode::Plugin(42)));
        }),
        ("restore notice inner focus", |s, _, _, _| {
            s.popups.set_focus(None);
            focused_notice(s);
        }),
        ("rename input dialog", |s, read, sid, _| {
            s.popups.set_focus(None);
            open_rename(s, read, sid, PopupScope::Window);
        }),
        ("address band", |s, _, sid, _| {
            s.popups.set_focus(None);
            s.html_address.set_focused_for_test(sid);
        }),
        ("stage", |s, _, _, _| {
            s.popups.set_focus(None);
            assert!(s.open_fullscreen_stage("blank"));
        }),
        ("settings request", |s, _, _, _| {
            s.popups.set_focus(None);
            s.settings_open_requested = true;
        }),
        ("tutorial", |s, _, _, _| {
            s.popups.set_focus(None);
            s.tutorial.request_start(0);
            s.tutorial.start_pending();
        }),
    ];
    let mut allowed = 0;
    for (name, setup) in cases {
        let (mut state, mut session, sid, notifications) = scene();
        let engine = session.borrow_mut();
        let read = engine.read();
        setup(&mut state, &read, sid, notifications);
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

/// 보이는 pane·surface 가 없는 배치. surface 범위 팝업은 범위가 숨는다.
fn empty_layout() -> crate::adapters::ui::LayoutContext {
    crate::adapters::ui::LayoutContext {
        active_workspace: 0,
        active_workspace_id: 0,
        pane_rects: Vec::new(),
        surface_rects: Vec::new(),
        active_tabs: Vec::new(),
        active_tab_ids: Vec::new(),
        workspace_rect: None,
        surface_owners: Vec::new(),
        tab_content_rects: Vec::new(),
    }
}

/// C2 표: rename 소유자는 포커스 노드가 rename host 인스턴스일 때만이다. 대조: rename 이 포커스 노드면 소유자다. rename
/// 은 범위를 덮으므로 팝업 칸을 비워도 정규화가 rename 을 포커스 노드로 되돌려 소유자다. 포커스 노드가 rename 이 아닌
/// host 팝업(알림 목록)이거나 rename 없이 팝업 포커스가 없거나, rename 이 열려 있어도 무대·설정 열기 요청이 포커스
/// 노드이거나 rename 범위가 숨었으면 소유자가 없다.
#[test]
fn the_rename_owner_needs_the_rename_focus_node() {
    type Setup =
        fn(&mut crate::state::MainViewState, &crate::runtime::engine_read::EngineRead<'_>, u32);
    let window: Setup = |s, read, sid| open_rename(s, read, sid, PopupScope::Window);
    let cases: [(&str, Setup, bool); 7] = [
        ("rename focus", window, true),
        (
            "cleared focus under the covering rename",
            |s, read, sid| {
                open_rename(s, read, sid, PopupScope::Window);
                s.popups.set_focus(None);
            },
            true,
        ),
        (
            "other host popup focus",
            |s, read, sid| {
                open_rename(s, read, sid, PopupScope::Window);
                s.popups
                    .open_request(
                        &crate::intent::OpenRequest::new("notifications", PopupScope::Window)
                            .focused(),
                        true,
                    )
                    .expect("알림 목록이 열린다");
            },
            false,
        ),
        ("no popup focus", |s, _, _| s.popups.set_focus(None), false),
        (
            "stage",
            |s, read, sid| {
                open_rename(s, read, sid, PopupScope::Window);
                assert!(s.open_fullscreen_stage("blank"));
            },
            false,
        ),
        (
            "settings request",
            |s, read, sid| {
                open_rename(s, read, sid, PopupScope::Window);
                s.settings_open_requested = true;
            },
            false,
        ),
        (
            "hidden scope",
            |s, read, sid| {
                open_rename(s, read, sid, PopupScope::Surface(sid));
                drop(
                    egui::Context::default().run(egui::RawInput::default(), |ctx| {
                        s.popups.prepare_frame_for_draw(ctx, Some(&empty_layout()));
                    }),
                );
            },
            false,
        ),
    ];
    for (name, setup, owns) in cases {
        let (mut state, mut session, sid, notifications) = scene();
        let engine = session.borrow_mut();
        let read = engine.read();
        state.popups.close_instance(
            notifications,
            crate::adapters::ui::popup::PopupCloseReason::CloseButton,
        );
        setup(&mut state, &read, sid);
        state.normalize_focus(&read);
        assert_eq!(
            super::rename_input_owner(&state, &read).is_some(),
            owns,
            "{name}"
        );
    }
}

/// T-IME-4a: Given host 팝업(알림 목록)의 몫 결속만 남고 live host 포커스가 풀렸다, When 단축키를 처리한 뒤 조합 문자를
/// 정하면(`MainView::after_shortcut_consumed`), Then 버리지 않고 PTY 로 확정 전송한다(C4 는 결속을 읽지 않는다).
#[test]
fn a_host_bound_segment_without_live_host_focus_flushes_the_preedit() {
    let (mut state, mut session, _, notifications) = scene();
    state.popups.set_focus(None);
    state
        .popups
        .bind_input_key_owner(PopupNode::Host(notifications));
    let engine = session.borrow_mut();
    state.normalize_focus(&engine.read());
    assert_eq!(
        state.popups.bound_input_key_owner(),
        Some(PopupNode::Host(notifications)),
        "host 결속이 남았다"
    );
    assert!(!state.popups.has_focused(), "live host 포커스는 없다");
    assert!(!crate::view::main::keyboard::shortcut_drops_preedit(&state));
}

/// T-IME-4b: Given plugin 팝업의 몫 결속이 있고 팝업 포커스는 없다, When 단축키를 처리한 뒤 조합 문자를 정하면, Then
/// plugin 키 대상(`plugin_popup_owns_keys`)이 그 결속을 읽어 조합 문자를 버린다.
#[test]
fn a_plugin_bound_segment_clears_the_preedit() {
    let (mut state, mut session, _, _) = scene();
    state.popups.set_focus(None);
    plugin_candidate(&mut state);
    state.popups.bind_input_key_owner(PopupNode::Plugin(42));
    let engine = session.borrow_mut();
    state.normalize_focus(&engine.read());
    assert!(!state.popups.has_focused(), "팝업 포커스는 없다");
    assert!(crate::view::main::keyboard::shortcut_drops_preedit(&state));
}
