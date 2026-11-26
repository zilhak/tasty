//! 튜토리얼 말풍선의 Esc 는 키 경로가 정한다. 말풍선은 포커스 노드일 때(`KeyNode::Tutorial`)와 직전 그리기에 보였을 때
//! Window 에 붙은 노드(`key_nodes::attached_popups`)로 경로에 서고, 수신자가 말풍선이면 창의 Esc 적용
//! (`MainViewState::apply_escape_key`)이 튜토리얼을 중단한다. 시험은 창의 단계 4(`key_nodes::window_key_decision`)와
//! 단계 11 입구를 창과 같은 함수로 밟고, 말풍선은 창 패스와 같은 순서 함수(`WindowInput::run_pass`)로 그린다.

use super::repro::screen;
use super::{PANE_L, layout, scene, surface_rect_l};
use crate::adapters::ui::input::key_class::KeyEventValue;
use crate::adapters::ui::input::key_nodes::{
    KeyDecision, focused_surface_takes_egui, window_key_decision,
};
use crate::adapters::ui::input::key_route::KeyReceiver;
use crate::adapters::ui::popup::frame_keys::Incoming;
use crate::adapters::ui::popup::window_input_test_support::WindowInput;
use crate::adapters::ui::popup::{PopupInstanceId, PopupScope, defs};
use crate::gfx::gpu::text_selection::HeldPastes;
use crate::runtime::engine_read::EngineRead;
use crate::state::RequestContext;
use crate::state::focus_node::FocusNode;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

fn register(state: &mut RequestContext) {
    crate::i18n::init("en");
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
}

/// 창 패스 하나처럼 남은 몫을 꺼내 튜토리얼 말풍선을 그린다.
fn draw(
    ctx: &egui::Context,
    state: &mut RequestContext,
    engine: &EngineRead<'_>,
    input: &mut WindowInput,
) {
    let theme = tasty_type_appearance::theme::Theme::with_colors_and_zoom(
        tasty_themes::mocha_fallback_colors(),
        false,
        1.0,
    );
    let mut held = HeldPastes::default();
    drop(input.run_pass(
        state,
        ctx,
        screen(),
        &mut held,
        &mut |_| {},
        |ctx, state| {
            crate::adapters::ui::tutorial::draw_tutorial_overlay(
                ctx,
                state,
                engine,
                &layout(),
                screen(),
                &theme,
            );
        },
    ));
}

/// 창이 winit 키 사건 하나로 받은 Escape 누름(`mods` 는 그 사건의 수식키). 끊기 → 단계 4 수신자 → 단계 11 입구를 창과
/// 같은 함수로 밟는다. 단계 4 수신자와 창이 처리했는지를 돌려준다.
fn escape(
    state: &mut RequestContext,
    engine: &EngineRead<'_>,
    input: &mut WindowInput,
    mods: ModifiersState,
) -> (KeyReceiver, bool) {
    let logical = Key::Named(NamedKey::Escape);
    let value = KeyEventValue {
        logical: &logical,
        physical: PhysicalKey::Code(KeyCode::Escape),
        pressed: true,
        mods,
    };
    let incoming = if mods.is_empty() {
        Incoming::Escape
    } else {
        Incoming::Key
    };
    crate::view::main::input_segment::segment_focus(state, engine, input, incoming);
    let egui_surface = focused_surface_takes_egui(state, engine);
    let decision = window_key_decision(state, engine, &value, false, egui_surface);
    let KeyDecision::Path(routed) = decision else {
        panic!("Esc 누름은 경로 키다");
    };
    let consumed = state.apply_escape_key(&value, decision, false, &mut Vec::new(), false);
    (routed.receiver, consumed)
}

fn bare(
    state: &mut RequestContext,
    engine: &EngineRead<'_>,
    input: &mut WindowInput,
) -> (KeyReceiver, bool) {
    escape(state, engine, input, ModifiersState::empty())
}

/// 왼쪽 버튼 뗌 하나를 큐에 싣는다(누름은 `WindowInput::press`).
fn release(input: &mut WindowInput, pos: egui::Pos2) {
    input.queue_mut().push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
}

fn start(state: &mut RequestContext) {
    state.tutorial.request_start(0);
    state.tutorial.start_pending();
    assert!(state.tutorial.active.is_some(), "튜토리얼이 시작한다");
}

/// 말풍선 밖의 L 터미널 자리.
fn terminal_point(state: &RequestContext) -> egui::Pos2 {
    let r = surface_rect_l();
    let callout = state.tutorial.callout_rect.expect("말풍선을 그렸다");
    [
        r.left_top(),
        r.left_bottom(),
        r.right_top(),
        r.right_bottom(),
    ]
    .into_iter()
    .map(|p| p + (r.center() - p) * 0.1)
    .find(|p| !callout.contains(*p))
    .expect("말풍선 밖 터미널 자리")
}

/// L 터미널이 포커스이고 튜토리얼이 키보드를 가진 장면에서 말풍선을 두 패스 그린다.
fn terminal_scene() -> (
    RequestContext,
    crate::runtime::engine_session::EngineSession,
    egui::Context,
    WindowInput,
) {
    let (mut state, session) = scene(PANE_L);
    register(&mut state);
    start(&mut state);
    (
        state,
        session,
        egui::Context::default(),
        WindowInput::default(),
    )
}

/// 고른 항목도 주소 확인도 없는 탐색기가 포커스이고 튜토리얼이 활성인 장면. 탐색기가 포커스이므로 말풍선은 키보드를
/// 갖지 않는다.
fn explorer_scene() -> (
    RequestContext,
    crate::runtime::engine_session::EngineSession,
    u32,
    std::path::PathBuf,
) {
    let (mut state, mut session, sid, root) = crate::state::tests::explorer_fixture(false);
    register(&mut state);
    {
        let engine = session.borrow_mut();
        let panel = crate::model::ExplorerPanel::new(sid, root.clone());
        state.explorer_views.get_or_init(&panel, None);
        start(&mut state);
        state.tutorial.keyboard_focus = false;
        state.normalize_focus(&engine.read());
        assert_eq!(
            crate::shortcuts::focused_explorer_surface_id(&state, &engine.read()),
            Some(sid),
            "탐색기가 포커스다"
        );
    }
    (state, session, sid, root)
}

/// T-TUT-1: Given 튜토리얼이 활성이고 말풍선이 보이지만 터미널이 포커스 노드일 때, When bare Esc 를 누르면, Then 터미널이
/// 받아 PTY 로 가고 창은 처리하지 않으며 튜토리얼은 계속된다.
#[test]
fn escape_from_a_terminal_during_the_tutorial_reaches_the_terminal() {
    let (mut state, mut session, ctx, mut input) = terminal_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    state.tutorial.keyboard_focus = false;
    state.normalize_focus(&read);
    draw(&ctx, &mut state, &read, &mut input);
    let sid = state.focused_surface_id(&read).expect("터미널");
    assert_eq!(state.focus_node(&read), FocusNode::Surface(Some(sid)));
    assert!(state.tutorial.callout_rect.is_some(), "말풍선을 그렸다");
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Surface(sid), false)
    );
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// T-TUT-2: Given 말풍선이 포커스 노드일 때, When bare Esc 를 누르면, Then 말풍선이 받아 창이 튜토리얼을 중단한다.
#[test]
fn escape_on_the_focused_callout_interrupts_the_tutorial() {
    let (mut state, mut session, ctx, mut input) = terminal_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    draw(&ctx, &mut state, &read, &mut input);
    assert_eq!(state.focus_node(&read), FocusNode::Tutorial);
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Tutorial, true)
    );
    assert!(state.tutorial.active.is_none(), "튜토리얼이 중단된다");
}

/// T-TUT-3: Given 말풍선이 포커스 노드일 때, When 말풍선 밖 터미널을 누른 뒤 bare Esc 를 누르면, Then 누름이 말풍선
/// 키보드 포커스를 꺼(`take_press_snapshot`) 터미널이 그 Esc 를 받고 튜토리얼은 계속된다.
#[test]
fn a_press_on_the_terminal_hands_tutorial_escape_to_the_terminal() {
    let (mut state, mut session, ctx, mut input) = terminal_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    draw(&ctx, &mut state, &read, &mut input);
    let at = terminal_point(&state);
    input.press(&mut state, &read, at);
    release(&mut input, at);
    draw(&ctx, &mut state, &read, &mut input);
    let sid = state.focused_surface_id(&read).expect("터미널");
    assert_eq!(state.focus_node(&read), FocusNode::Surface(Some(sid)));
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Surface(sid), false)
    );
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// 말풍선 재클릭: Given 터미널 누름으로 말풍선이 키보드를 놓았을 때, When 말풍선을 다시 누르고 bare Esc 를 누르면,
/// Then 그 누름을 그린 패스가 말풍선 포커스를 다시 켜 말풍선이 받고 튜토리얼이 중단된다.
#[test]
fn a_click_on_the_callout_after_a_terminal_press_lets_escape_interrupt_the_tutorial() {
    let (mut state, mut session, ctx, mut input) = terminal_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    draw(&ctx, &mut state, &read, &mut input);
    let at = terminal_point(&state);
    input.press(&mut state, &read, at);
    release(&mut input, at);
    draw(&ctx, &mut state, &read, &mut input);
    let callout = state
        .tutorial
        .callout_rect
        .expect("말풍선을 그렸다")
        .center();
    input.press(&mut state, &read, callout);
    release(&mut input, callout);
    draw(&ctx, &mut state, &read, &mut input);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Tutorial,
        "말풍선 누름이 포커스를 되찾는다"
    );
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Tutorial, true)
    );
    assert!(state.tutorial.active.is_none(), "튜토리얼이 중단된다");
}

/// T-TUT-4: Given 고른 항목이 없는 탐색기가 포커스이고 말풍선이 보일 때, When bare Esc 를 누르면, Then 탐색기는 그 Esc 를
/// 쓰지 않으므로 Window 에 붙은 말풍선이 받아 튜토리얼이 중단된다.
#[test]
fn an_escape_the_explorer_does_not_use_interrupts_the_tutorial() {
    let (mut state, mut session, _, _) = explorer_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    draw(&ctx, &mut state, &read, &mut input);
    assert!(state.tutorial.callout_rect.is_some(), "말풍선을 그렸다");
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Tutorial, true)
    );
    assert!(state.tutorial.active.is_none(), "튜토리얼이 중단된다");
}

/// Given 탐색기가 포커스이고 말풍선을 그린 뒤 전체화면 무대만 그린 패스가 지났을 때, When 무대를 닫고 일반 화면을 다시
/// 그리기 전에 bare Esc 를 누르면, Then 직전 패스에 말풍선이 없었으므로 말풍선은 붙은 수신자가 아니고 튜토리얼은 계속된다.
#[test]
fn a_stage_pass_leaves_no_callout_record_for_the_next_escape() {
    let (mut state, mut session, _, _) = explorer_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    draw(&ctx, &mut state, &read, &mut input);
    assert!(state.tutorial.callout_rect.is_some(), "말풍선을 그렸다");
    assert!(state.open_fullscreen_stage("blank"));
    let mut held = HeldPastes::default();
    drop(input.run_pass(
        &mut state,
        &ctx,
        screen(),
        &mut held,
        &mut |_| {},
        |ctx, state| crate::adapters::ui::draw_fullscreen_stage(ctx, state, &read),
    ));
    assert!(state.close_fullscreen_stage());
    let (receiver, consumed) = bare(&mut state, &read, &mut input);
    assert_ne!(receiver, KeyReceiver::Tutorial);
    assert!(!consumed);
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// T-TUT-5: Given 같은 장면에서 탐색기에 고른 항목이 있을 때, When bare Esc 를 누르면, Then 탐색기의 선택 해제 단축키가
/// 받고 창은 처리하지 않으며 튜토리얼은 계속된다.
#[test]
fn an_escape_the_explorer_uses_keeps_the_tutorial() {
    let (mut state, mut session, sid, root) = explorer_scene();
    state
        .explorer_views
        .get_mut(sid)
        .expect("탐색기 view")
        .select_only(&root.join("a.txt"));
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    draw(&ctx, &mut state, &read, &mut input);
    assert!(crate::shortcuts::explorer_takes_clear_selection(
        &state, &read
    ));
    let (receiver, consumed) = bare(&mut state, &read, &mut input);
    assert!(
        matches!(receiver, KeyReceiver::Shortcut { .. }),
        "{receiver:?}"
    );
    assert!(!consumed);
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// T-TUT-6: Given 말풍선이 포커스 노드일 때, When Shift+Esc 를 누르면, Then bare Esc 가 아니라 창의 Esc 적용 대상이 아니고
/// 튜토리얼은 계속된다.
#[test]
fn shift_escape_does_not_interrupt_the_tutorial() {
    let (mut state, mut session, ctx, mut input) = terminal_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    draw(&ctx, &mut state, &read, &mut input);
    assert_eq!(state.focus_node(&read), FocusNode::Tutorial);
    let (_, consumed) = escape(&mut state, &read, &mut input, ModifiersState::SHIFT);
    assert!(!consumed);
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// T-TUT-9: Given 고른 항목이 없는 탐색기가 포커스이고 말풍선과 Esc 를 받는 Window 팝업(알림 목록, 포커스 없음)이 함께
/// 보일 때, When bare Esc 를 누르면, Then 붙은 팝업이 먼저 받고 튜토리얼은 계속된다. 붙은 팝업이 선언 `Ignore` 이면
/// 건너뛰어 말풍선이 받는다.
#[test]
fn an_escape_with_a_window_popup_and_the_callout_goes_to_the_popup_first() {
    let (mut state, mut session, _, _) = explorer_scene();
    let notifications = state
        .popups
        .open_request(
            &crate::intent::OpenRequest::new("notifications", PopupScope::Window),
            false,
        )
        .expect("알림 목록이 열린다");
    {
        let engine = session.borrow_mut();
        let read = engine.read();
        state.normalize_focus(&read);
        let ctx = egui::Context::default();
        let mut input = WindowInput::default();
        draw(&ctx, &mut state, &read, &mut input);
        assert!(!state.popups.is_instance_focused(notifications));
        assert_eq!(
            bare(&mut state, &read, &mut input),
            (KeyReceiver::Popup(notifications), true)
        );
        assert!(state.tutorial.active.is_some(), "알림 목록이 먼저 받는다");
    }

    let (mut state, mut session, _, _) = explorer_scene();
    let z = tasty_host_plugin::next_popup_z_seq();
    state.plugin_popup_key_candidates.push((42, z));
    state
        .plugin_popup_bound_scopes
        .insert(42, ("p".to_string(), Some(PopupScope::Window)));
    state.plugin_popup_key_decls.insert(
        42,
        (
            crate::adapters::ui::popup::PopupKeyboard::Keys,
            crate::adapters::ui::popup::PopupEscape::Ignore,
        ),
    );
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    draw(&ctx, &mut state, &read, &mut input);
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Tutorial, true)
    );
    assert!(
        state.tutorial.active.is_none(),
        "Ignore 팝업을 건너뛰어 말풍선이 받는다"
    );
}

/// T-TUT-10: Given 튜토리얼이 활성이고 직전 패스의 포커스 노드가 팝업(명령 팔레트)이라 말풍선을 그리지 않았을 때, When
/// 새로 그리기 전에 그 팝업이 닫혀 탐색기가 포커스 노드가 된 뒤 bare Esc 를 누르면, Then 보이지 않는 말풍선은 붙은
/// 수신자가 아니라 튜토리얼은 계속된다.
#[test]
fn a_callout_not_drawn_in_the_last_pass_takes_no_attached_escape() {
    let (mut state, mut session, sid, _) = explorer_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    draw(&ctx, &mut state, &read, &mut input);
    let palette: PopupInstanceId = state
        .popups
        .open_request(
            &crate::intent::OpenRequest::new(
                crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID,
                PopupScope::Window,
            )
            .centered()
            .focused(),
            true,
        )
        .expect("명령 팔레트가 열린다");
    state.normalize_focus(&read);
    assert!(matches!(state.focus_node(&read), FocusNode::Popup(_)));
    draw(&ctx, &mut state, &read, &mut input);
    state.popups.close_instance(
        palette,
        crate::adapters::ui::popup::PopupCloseReason::Command,
    );
    state.normalize_focus(&read);
    assert_eq!(state.focus_node(&read), FocusNode::Surface(Some(sid)));
    let (receiver, consumed) = bare(&mut state, &read, &mut input);
    assert_ne!(receiver, KeyReceiver::Tutorial);
    assert!(!consumed);
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// Given 튜토리얼이 활성이고 파일 피커가 포커스 노드일 때, When bare Esc 를 누르면, Then 피커가 받고
/// 튜토리얼은 계속된다.
#[test]
fn escape_with_a_picker_focus_keeps_the_tutorial() {
    let (mut state, mut session, ctx, mut input) = terminal_scene();
    let engine = session.borrow_mut();
    let read = engine.read();
    draw(&ctx, &mut state, &read, &mut input);
    let picker = state
        .popups
        .open_request(
            &crate::intent::OpenRequest::new(
                crate::adapters::ui::popup::file_picker::FILE_PICKER_POPUP_ID,
                PopupScope::Window,
            )
            .centered()
            .focused(),
            true,
        )
        .expect("파일 피커가 열린다");
    state.normalize_focus(&read);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(crate::adapters::ui::popup::focus::PopupNode::Host(picker))
    );
    assert_eq!(
        bare(&mut state, &read, &mut input),
        (KeyReceiver::Popup(picker), true)
    );
    assert!(state.tutorial.active.is_some(), "튜토리얼은 계속된다");
}

/// 별도 창 붙은 목록: Given 별도 창 관리자에 포커스 없는 비덮음 Window 팝업(알림 목록)이 있을 때, When 창 범위 노드의 붙은
/// 목록을 만들면(`key_nodes::separate_window_attached`), Then 그 host 팝업 항목 하나만 선언 Esc(Close)와 함께 든다. 별도
/// 창에는 말풍선 항목이 없다.
#[test]
fn a_separate_window_attached_list_holds_only_its_host_popups() {
    let (mut state, mut session) = scene(PANE_L);
    register(&mut state);
    let notifications = state
        .popups
        .open_request(
            &crate::intent::OpenRequest::new("notifications", PopupScope::Window),
            false,
        )
        .expect("알림 목록이 열린다");
    state.normalize_focus(&session.borrow_mut().read());
    let attached = crate::adapters::ui::input::key_nodes::separate_window_attached(&state.popups);
    assert_eq!(
        attached.len(),
        1,
        "보이는 비덮음 팝업 하나가 붙는다: {attached:?}"
    );
    assert!(
        format!("{:?}", attached[0].node).contains(&format!("Host({notifications:?})")),
        "{attached:?}"
    );
    assert_eq!(
        attached[0].escape,
        crate::adapters::ui::popup::PopupEscape::Close
    );
}
