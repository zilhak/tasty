//! IME 허용(`window_ime_allowed`)은 창의 지금 포커스 노드에서 만든 대상을 따른다. host 팝업 대상의 근거인 egui IME
//! 출력은 방금 그린 몫이 같은 인스턴스일 때만 쓴다.

use super::{ImeTarget, ime_allowed, ime_target, window_ime_allowed};
use crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID;
use crate::adapters::ui::popup::focus::PopupNode;
use crate::adapters::ui::popup::{PopupEscape, PopupInstanceId, PopupKeyboard, PopupScope, defs};
use crate::intent::OpenRequest;
use crate::state::MainViewState;
use crate::state::focus_node::FocusNode;

fn state() -> (MainViewState, crate::runtime::engine_session::EngineSession) {
    crate::i18n::init("en");
    let (mut state, session) = crate::state::tests::test_state();
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    (state, session)
}

fn open(state: &mut MainViewState, id: &'static str, focused: bool) -> PopupInstanceId {
    let request = OpenRequest::new(id, PopupScope::Window).centered();
    let request = if focused { request.focused() } else { request };
    state
        .popups
        .open_request(&request, true)
        .expect("팝업이 열린다")
}

/// 창 범위 plugin 팝업 42 를 선언 `keyboard` 로 두고 포커스 노드로 한다.
fn focus_plugin(state: &mut MainViewState, keyboard: PopupKeyboard) {
    state
        .plugin_popup_key_candidates
        .push((42, tasty_host_plugin::next_popup_z_seq()));
    state
        .plugin_popup_bound_scopes
        .insert(42, ("p".to_string(), Some(PopupScope::Window)));
    state
        .plugin_popup_key_decls
        .insert(42, (keyboard, PopupEscape::Close));
    state.popups.set_focus(Some(PopupNode::Plugin(42)));
}

/// 판정 표: 대상과 egui IME 출력의 모든 조합.
#[test]
fn ime_permission_follows_the_target_for_every_input() {
    for output in [false, true] {
        assert_eq!(
            ime_allowed(ImeTarget::HostPopup { drawn: true }, output),
            output
        );
        assert!(!ime_allowed(ImeTarget::HostPopup { drawn: false }, output));
        assert!(ime_allowed(
            ImeTarget::PluginPopup(PopupKeyboard::Text),
            output
        ));
        assert!(!ime_allowed(
            ImeTarget::PluginPopup(PopupKeyboard::Keys),
            output
        ));
        assert!(!ime_allowed(
            ImeTarget::PluginPopup(PopupKeyboard::None),
            output
        ));
        assert!(ime_allowed(ImeTarget::Other, output));
    }
}

/// 대상 표: 포커스 노드 종류마다 대상. plugin 선언이 없으면 기본값 `Text` 다.
#[test]
fn the_ime_target_follows_the_focus_node_kind() {
    let (mut state, _session) = state();
    let a = PopupInstanceId(7);
    let host = FocusNode::Popup(PopupNode::Host(a));
    assert_eq!(
        ime_target(&state, host, Some(host)),
        ImeTarget::HostPopup { drawn: true }
    );
    assert_eq!(
        ime_target(&state, host, Some(FocusNode::Surface(None))),
        ImeTarget::HostPopup { drawn: false }
    );
    let plugin = FocusNode::Popup(PopupNode::Plugin(42));
    assert_eq!(
        ime_target(&state, plugin, Some(plugin)),
        ImeTarget::PluginPopup(PopupKeyboard::Text),
        "선언이 없으면 Text"
    );
    state
        .plugin_popup_key_decls
        .insert(42, (PopupKeyboard::Keys, PopupEscape::Close));
    assert_eq!(
        ime_target(&state, plugin, Some(plugin)),
        ImeTarget::PluginPopup(PopupKeyboard::Keys)
    );
    for node in [
        FocusNode::Stage,
        FocusNode::SettingsRequest,
        FocusNode::HtmlAddress(1),
        FocusNode::Tutorial,
        FocusNode::Surface(Some(1)),
    ] {
        assert_eq!(
            ime_target(&state, node, Some(host)),
            ImeTarget::Other,
            "{node:?}"
        );
    }
}

/// T-IME-1: Given 포커스 없는 host 팝업(알림 목록)이 열려 있고 surface 가 포커스 노드일 때, When IME 허용을 정하면,
/// Then 켠다.
#[test]
fn ime_is_allowed_when_the_focus_node_is_a_surface_beside_an_unfocused_popup() {
    let (mut state, mut session) = state();
    open(&mut state, "notifications", false);
    let engine = session.borrow_mut();
    let read = engine.read();
    state.normalize_focus(&read);
    assert!(matches!(state.focus_node(&read), FocusNode::Surface(_)));
    assert!(window_ime_allowed(&mut state, &read, None, false));
}

/// T-IME-2: Given 선언 `Keys` 인 plugin 팝업이 포커스 노드일 때, When IME 허용을 정하면, Then 끈다.
#[test]
fn ime_is_off_for_a_keys_plugin_popup_focus_node() {
    let (mut state, mut session) = state();
    focus_plugin(&mut state, PopupKeyboard::Keys);
    let engine = session.borrow_mut();
    let read = engine.read();
    state.normalize_focus(&read);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Plugin(42))
    );
    assert!(!window_ime_allowed(&mut state, &read, None, false));
}

/// T-IME-3: Given 선언 `Text` 인 plugin 팝업이 포커스 노드일 때, When IME 허용을 정하면, Then 켠다.
#[test]
fn ime_is_on_for_a_text_plugin_popup_focus_node() {
    let (mut state, mut session) = state();
    focus_plugin(&mut state, PopupKeyboard::Text);
    let engine = session.borrow_mut();
    let read = engine.read();
    state.normalize_focus(&read);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Plugin(42))
    );
    assert!(window_ime_allowed(&mut state, &read, None, false));
}

/// T-IME-5: Given 포커스를 가진 host 팝업(명령 팔레트)이 관리자 칸에 남은 채 전체화면 무대가 열렸을 때, When 무대
/// 프레임 출력 입구가 IME 허용을 정하면, Then 포커스 노드가 무대라 켠다.
#[test]
fn ime_is_on_for_a_stage_over_an_open_host_popup() {
    let (mut state, mut session) = state();
    open(&mut state, COMMAND_PALETTE_POPUP_ID, true);
    assert!(state.open_fullscreen_stage("blank"));
    let engine = session.borrow_mut();
    let read = engine.read();
    assert!(state.popups.has_focused(), "관리자 칸에 팔레트가 남는다");
    assert_eq!(state.focus_node(&read), FocusNode::Stage);
    assert!(window_ime_allowed(&mut state, &read, None, false));
}

/// T-IME-8a: Given 그린 몫이 host 텍스트 팝업(명령 팔레트)이라 egui IME 출력이 있지만 지금 노드가 선언 `Keys` 인 plugin
/// 팝업일 때, When IME 허용을 정하면, Then 끈다.
#[test]
fn ime_is_off_when_the_drawn_widget_has_output_but_the_next_node_is_a_keys_plugin() {
    let (mut state, mut session) = state();
    let palette = open(&mut state, COMMAND_PALETTE_POPUP_ID, true);
    let drawn = FocusNode::Popup(PopupNode::Host(palette));
    focus_plugin(&mut state, PopupKeyboard::Keys);
    let engine = session.borrow_mut();
    let read = engine.read();
    state.normalize_focus(&read);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Plugin(42))
    );
    assert!(!window_ime_allowed(&mut state, &read, Some(drawn), true));
}

/// T-IME-8b: Given 그린 몫이 host 텍스트 팝업 A(명령 팔레트, IME 출력 있음)이고 지금 노드가 다른 host 팝업 B(알림 목록)일
/// 때, When IME 허용을 정하면, Then A 의 출력은 B 의 근거가 아니라 끈다. 같은 몫이 B 를 그렸으면 그 출력으로 켠다.
#[test]
fn ime_output_of_one_host_instance_does_not_allow_another() {
    let (mut state, mut session) = state();
    let a = open(&mut state, COMMAND_PALETTE_POPUP_ID, true);
    let b = open(&mut state, "notifications", true);
    let engine = session.borrow_mut();
    let read = engine.read();
    state.normalize_focus(&read);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Host(b))
    );
    assert!(!window_ime_allowed(
        &mut state,
        &read,
        Some(FocusNode::Popup(PopupNode::Host(a))),
        true
    ));
    assert!(window_ime_allowed(
        &mut state,
        &read,
        Some(FocusNode::Popup(PopupNode::Host(b))),
        true
    ));
}

/// 패스 닫기가 닫는 몫의 노드를 남긴다: Given 키 사건으로 몫의 노드가 정해진 그리기 몫, When 그 패스를 닫으면, Then
/// `closed_segment_focus` 가 그 몫의 노드다.
#[test]
fn closing_a_pass_keeps_the_node_of_the_closed_segment() {
    let (mut state, mut session) = state();
    let palette = open(&mut state, COMMAND_PALETTE_POPUP_ID, true);
    let engine = session.borrow_mut();
    let read = engine.read();
    state.normalize_focus(&read);
    let node = FocusNode::Popup(PopupNode::Host(palette));
    state.segment_focus = Some(node);
    state.close_input_pass(&egui::Context::default());
    assert_eq!(state.closed_segment_focus, Some(node));
}
