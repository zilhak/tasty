//! W2(`webview_sync::key_release::release_webview_key_focus`): WebView 는 키 쥘 곳이 그 surface 이고 이번 프레임에
//! 드러날 때만 OS 키 포커스를 지킨다. 장면은 좌우 두 surface(L·R)의 실제 엔진이고 엔진 포커스는 L 이다. WebView 는
//! 시험용이고 overlay 값은 동기화처럼 `has_egui_overlay_open` 이다.

use std::cell::Cell;

use super::repro::open_host_popup;
use super::{PANE_L, SURFACE_L, SURFACE_R, scene};
use crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID;
use crate::adapters::ui::popup::{PopupId, PopupScope, PopupState, defs};
use crate::app::webview_sync::key_release::{
    KeyFocusWebView, note_native_webview_focus, release_webview_key_focus,
};
use crate::core::CoreState;
use crate::intent::OpenRequest;
use crate::state::RequestContext;
use crate::state::webview_overlay::OverlayMode;

#[derive(Default)]
struct FakeWebView {
    holds: Cell<bool>,
    released: Cell<u32>,
}

impl KeyFocusWebView for FakeWebView {
    fn holds_keyboard_focus(&self) -> bool {
        self.holds.get()
    }

    fn release_keyboard_focus(&self) {
        self.released.set(self.released.get() + 1);
        self.holds.set(false);
    }
}

fn holding() -> FakeWebView {
    let wv = FakeWebView::default();
    wv.holds.set(true);
    wv
}

fn defs_scene() -> (
    RequestContext,
    crate::runtime::engine_session::EngineSession,
) {
    let (mut state, session) = scene(PANE_L);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    (state, session)
}

/// 동기화 한 번. overlay 값은 동기화처럼 지금 상태에서 읽는다.
fn sync(
    state: &mut RequestContext,
    core: &CoreState,
    mode: OverlayMode,
    wvs: &[(u32, &FakeWebView)],
) {
    let overlay_open = state.has_egui_overlay_open();
    release_webview_key_focus(state, core, mode, overlay_open, wvs);
}

/// 창 범위 덮개 정의 `id` 를 등록하고 연다.
fn open_window_cover(state: &mut RequestContext, id: PopupId, focused: bool) {
    let mut p = PopupState::new(
        id,
        "cover".to_string(),
        egui::vec2(120.0, 60.0),
        PopupScope::Window,
    );
    p.look = p.look.covering(true);
    p.declared_look = p.look;
    state.popups.register(p);
    let request = OpenRequest::new(id, PopupScope::Window).centered();
    let request = if focused { request.focused() } else { request };
    state
        .popups
        .open_request(&request, focused)
        .expect("덮개가 열린다");
}

fn notice(state: &mut RequestContext) {
    assert!(crate::adapters::ui::restore_notice::push(
        state,
        crate::adapters::ui::restore_notice::RestoreNoticeEntry {
            id: 1,
            title: "title".into(),
            cause: "cause".into(),
            label: "label".into(),
            path: "/a".into(),
            body: "body".into(),
        },
    ));
}

/// T-W2-1: Given 창 범위 팝업(명령 팔레트)이 포커스 노드일 때, When 동기화하면, Then OS 키 포커스를 쥔 모든 WebView
/// 에서 회수한다.
#[test]
fn a_window_popup_focus_node_releases_every_webview() {
    let (mut state, mut session) = defs_scene();
    state
        .popups
        .open_request(
            &OpenRequest::new(COMMAND_PALETTE_POPUP_ID, PopupScope::Window)
                .centered()
                .focused(),
            true,
        )
        .expect("팔레트가 열린다");
    let (l, r) = (holding(), holding());
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l), (SURFACE_R, &r)],
    );
    assert_eq!((l.released.get(), r.released.get()), (1, 1));
}

/// T-W2-2: Given R 의 Surface 범위 팝업이 포커스 노드이고 L WebView 가 OS 키 포커스를 쥘 때, When 동기화하면, Then L
/// 에서 회수한다(J-2 (a)).
#[test]
fn a_surface_popup_focus_node_in_r_releases_the_webview_in_l() {
    let (mut state, mut session) = defs_scene();
    open_host_popup(&mut state, SURFACE_R, true);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-3: Given 포커스를 가져오지 않은 plugin 팝업이 열려 있고 모드가 `Hide` 가 아니며 L 이 포커스 노드일 때, When
/// 동기화하면, Then L WebView 는 OS 키 포커스를 지킨다.
#[test]
fn an_open_passive_plugin_popup_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    state.plugin_popup_open = true;
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
    assert!(l.holds.get());
}

/// T-W2-4: Given 안쪽 컨트롤이 키를 갖지 않은 복원 안내가 떠 있을 때, When 동기화하면, Then 포커스 노드 L 의
/// WebView 는 지킨다.
#[test]
fn an_unfocused_restore_notice_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    notice(&mut state);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Hide,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
}

/// T-W2-5: Given 복원 안내 안쪽 컨트롤이 키를 가질 때, When 동기화하면, Then 키 쥘 곳이 없어 L WebView 에서
/// 회수한다.
#[test]
fn a_restore_notice_holding_keys_releases_the_webview() {
    let (mut state, mut session) = defs_scene();
    notice(&mut state);
    crate::adapters::ui::restore_notice::move_inside_focus(
        &egui::Context::default(),
        &mut state.popups,
        1,
    );
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-6: Given 다른 W1 항 없이 L 을 덮는 passive host 팝업 하나만 열렸을 때, When 동기화하면, Then 정규화로 그
/// 덮개가 포커스 노드라 L WebView 에서 회수한다.
#[test]
fn a_lone_passive_host_cover_over_the_focused_html_surface_releases_it() {
    let (mut state, mut session) = defs_scene();
    let cover = open_host_popup(&mut state, SURFACE_L, false);
    assert!(state.popups.instance_is_visible_passive(cover));
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-7: Given L 이 포커스 노드이고 다른 곳의 포커스 없는 팝업으로 overlay 가 열렸으며 모드가 `Hide` 일 때, When
/// 동기화하면, Then L 은 이번 프레임에 숨으므로 회수한다.
#[test]
fn a_hide_mode_overlay_releases_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    open_host_popup(&mut state, SURFACE_R, true);
    state.popups.set_focus(None);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Hide,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// T-W2-8: Given 포커스 노드가 창 범위 팝업이라 L 에서 한 번 회수했을 때, When 키 쥘 곳과 기록이 그대로인 채 native
/// 포커스 알림 없이 L 이 다시 OS 포커스를 얻고 동기화하면, Then 다시 회수한다.
#[test]
fn a_webview_that_regains_os_focus_while_not_the_holder_is_released_again() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_top", true);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    l.holds.set(true);
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 2);
}

/// T-W2-9: Given 창 범위 팝업이 포커스 노드인 구간에서 L 을 회수했을 때, When 같은 구간에 R WebView 가 OS 키 포커스를
/// 쥔 채 나타나 동기화하면, Then R 도 회수한다.
#[test]
fn a_webview_appearing_later_in_the_same_overlay_is_released() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_top", true);
    let (l, r) = (holding(), holding());
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l), (SURFACE_R, &r)],
    );
    assert_eq!((l.released.get(), r.released.get()), (1, 1));
}

/// T-W2-10: Given R 의 비-passive host 팝업이 열린 채 포커스만 잃고 L 이 포커스 노드이며 모드가 `Hide` 가 아닐 때,
/// When 동기화하면, Then L WebView 는 지킨다.
#[test]
fn an_unfocused_host_popup_elsewhere_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    open_host_popup(&mut state, SURFACE_R, true);
    state.popups.set_focus(None);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
}

/// T-W2-11: Given R pane 의 포커스 잃은 이름 바꾸기 팝업(탭 이름)이 열려 있고 L 이 포커스 노드이며 모드가 `Hide` 가 아닐 때, When
/// 동기화하면, Then L WebView 는 지킨다.
#[test]
fn an_unfocused_rename_popup_keeps_the_focused_webview() {
    let (mut state, mut session) = defs_scene();
    let rename = crate::adapters::ui::rename_popup::RENAME_POPUP_ID;
    state
        .popups
        .open_request(
            &OpenRequest::new(rename, PopupScope::Pane(super::PANE_R))
                .centered()
                .focused(),
            true,
        )
        .expect("이름 바꾸기가 열린다");
    state.popups.set_focus(None);
    assert!(state.popups.is_open(rename));
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 0);
}

/// T-W2-12: Given 창 범위 덮개가 포커스 노드라 L 을 회수해 기록을 세웠을 때, When L 의 native 포커스 알림이 기록을
/// 지우고(덮여 포커스 노드는 그대로) L 이 다시 OS 포커스를 얻어 동기화하면, Then 다시 회수한다.
#[test]
fn a_native_focus_notice_clears_the_record_and_the_next_sync_releases_again() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_top", true);
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert!(state.webview_key_released.contains(&SURFACE_L));
    note_native_webview_focus(&mut state, SURFACE_L);
    assert!(!state.webview_key_released.contains(&SURFACE_L));
    l.holds.set(true);
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 2);
}

/// T-W2-13: Given 부모·자식이 아닌 창 범위 덮개 둘 가운데 아래는 passive, 위는 포커스 팝업일 때, When 그 패스에 위
/// 팝업이 닫히고 첫 동기화가 오면, Then 정규화로 passive 덮개가 포커스 노드라 L WebView 에서 회수한다.
#[test]
fn closing_the_top_window_cover_releases_the_webview_under_a_passive_cover_in_the_same_pass() {
    let (mut state, mut session) = defs_scene();
    open_window_cover(&mut state, "w2_cover_below", false);
    open_window_cover(&mut state, "w2_cover_top", true);
    state.popups.close("w2_cover_top");
    assert_eq!(state.popups.focus(), None, "닫기가 관리자 칸을 비운다");
    let l = holding();
    let engine = session.borrow_mut();
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l)],
    );
    assert_eq!(l.released.get(), 1);
}

/// 기록은 키 쥘 곳이 된 surface 와 사라진 surface 에서 지운다.
#[test]
fn the_record_clears_for_the_holder_and_for_a_gone_surface() {
    let (mut state, mut session) = defs_scene();
    let engine = session.borrow_mut();
    state
        .webview_key_released
        .extend([SURFACE_L, SURFACE_R, 999]);
    let (l, r) = (FakeWebView::default(), FakeWebView::default());
    sync(
        &mut state,
        &engine.read(),
        OverlayMode::Opaque,
        &[(SURFACE_L, &l), (SURFACE_R, &r)],
    );
    assert_eq!(
        state
            .webview_key_released
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [SURFACE_R]
    );
}
