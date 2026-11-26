//! 패스 출력 입구의 IME 허용(`ime_permission::window_ime_allowed`). 대상은 입구가 정규화한 포커스 노드다(그 패스의
//! 닫기·덮음 갱신 반영). 근거로 읽는 그린 몫의 노드는 그 몫을 그릴 때 확정한 노드다. 키가 들지 않은 몫도 그리기가
//! 정한 노드를 출력과 함께 운반하고, 출력 입구가 그리기·닫기 뒤의 지금 노드로 채우지 않는다.

use super::repro::screen;
use super::{PANE_L, layout, scene};
use crate::adapters::ui::popup::focus::PopupNode;
use crate::adapters::ui::popup::frame::draw_popup_layer;
use crate::adapters::ui::popup::window_input_test_support::WindowInput;
use crate::adapters::ui::popup::{
    PopupCloseReason, PopupId, PopupInstanceId, PopupScope, PopupState,
};
use crate::gfx::gpu::ime_permission::window_ime_allowed;
use crate::gfx::gpu::text_selection::HeldPastes;
use crate::intent::OpenRequest;
use crate::state::RequestContext;
use crate::state::focus_node::FocusNode;

const PARENT: PopupId = "pass_ime_parent";
const CHILD: PopupId = "pass_ime_child";

/// 창 범위 부모 B 와 그 자식 A 를 열어 A 를 포커스 노드로 둔다.
fn parent_and_child(state: &mut RequestContext) -> (PopupInstanceId, PopupInstanceId) {
    for id in [PARENT, CHILD] {
        state.popups.register(PopupState::new(
            id,
            "test",
            egui::vec2(200.0, 100.0),
            PopupScope::Window,
        ));
    }
    let parent = state
        .popups
        .open_request(
            &OpenRequest::new(PARENT, PopupScope::Window)
                .centered()
                .focused(),
            true,
        )
        .expect("부모가 열린다");
    let mut request = OpenRequest::new(CHILD, PopupScope::Window)
        .at(egui::pos2(520.0, 520.0))
        .focused();
    request.parent = Some(parent);
    let child = state
        .popups
        .open_request(&request, true)
        .expect("자식이 열린다");
    (parent, child)
}

/// 키 없는 패스 하나: host 팝업 층을 그리고, `close` 면 그 패스 끝에 자식을 닫는다.
fn keyless_pass(
    ctx: &egui::Context,
    state: &mut RequestContext,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    input: &mut WindowInput,
    close: Option<PopupInstanceId>,
) {
    let mut held = HeldPastes::default();
    drop(input.run_pass(
        state,
        ctx,
        screen(),
        &mut held,
        &mut |_| {},
        |ctx, state| {
            draw_popup_layer(ctx, state, engine, &layout());
            if let Some(child) = close {
                state
                    .popups
                    .close_instance(child, PopupCloseReason::CloseButton);
            }
        },
    ));
}

/// X3: Given IME 위젯을 그린 host 자식 A 가 부모 B 위의 포커스 노드일 때, When 키 없는 패스가 A 를 그린 뒤 그 패스 끝에
/// A 가 닫혀 B 로 포커스가 돌아오면, Then A 의 IME 출력은 B 의 허용 근거가 아니다. 대조: A 를 닫지 않은 패스는 A 의
/// 출력으로 켠다.
#[test]
fn a_child_closed_at_the_end_of_a_keyless_pass_lends_no_ime_output_to_its_parent() {
    let (mut state, mut session) = scene(PANE_L);
    let (parent, child) = parent_and_child(&mut state);
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    keyless_pass(&ctx, &mut state, &read, &mut input, None);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Host(child))
    );
    let drawn = state.closed_segment_focus;
    assert!(
        window_ime_allowed(&mut state, &read, drawn, true),
        "대조: 그린 자식이 포커스 노드면 그 출력으로 켠다"
    );
    keyless_pass(&ctx, &mut state, &read, &mut input, Some(child));
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Host(parent))
    );
    let drawn = state.closed_segment_focus;
    assert!(!window_ime_allowed(&mut state, &read, drawn, true));
}

/// 창 범위 덮개 정의 `id` 를 등록하고 연다.
fn open_window_cover(
    state: &mut RequestContext,
    id: &'static str,
    focused: bool,
) -> PopupInstanceId {
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
        .expect("덮개가 열린다")
}

/// X2: Given 창 범위 덮개 둘 가운데 아래 B 는 passive, 위 A 는 IME 위젯을 그린 포커스 팝업일 때, When 그 패스에 A 가
/// 닫히고 출력 입구가 IME 허용을 정하면, Then 입구가 정규화한 포커스 노드(B)를 읽어 A 의 출력을 근거로 켜지 않는다
/// (T-W2-13 과 같은 전이).
#[test]
fn closing_the_top_cover_in_the_pass_judges_ime_on_the_passive_cover_left() {
    let (mut state, mut session) = scene(PANE_L);
    let below = open_window_cover(&mut state, "ime_cover_below", false);
    let top = open_window_cover(&mut state, "ime_cover_top", true);
    state.popups.close("ime_cover_top");
    let engine = session.borrow_mut();
    let read = engine.read();
    assert!(!window_ime_allowed(
        &mut state,
        &read,
        Some(FocusNode::Popup(PopupNode::Host(top))),
        true
    ));
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Host(below))
    );
}

/// 위젯 IME 출력 하나.
fn ime_output() -> egui::output::IMEOutput {
    let rect = egui::Rect::from_min_size(egui::pos2(530.0, 530.0), egui::vec2(80.0, 20.0));
    egui::output::IMEOutput {
        rect,
        cursor_rect: rect,
    }
}

/// X7: Given IME 위젯을 그린 host 자식 A 가 부모 B 위의 포커스 노드일 때, When 한 번의 egui 실행에서 첫 내부 패스가 A 를
/// 그리며 IME 출력을 내고 A 를 닫은 뒤 그 패스를 버리고(discard), 둘째 패스가 B 를 그리며 IME 출력을 내지 않으면, Then
/// egui 가 최종 출력에 남긴 A 의 IME 출력은 B 의 허용 근거가 아니다. 대조: 둘째 패스도 IME 출력을 내면 그 출력(B)으로
/// 켠다.
#[test]
fn an_ime_output_kept_from_a_discarded_pass_lends_nothing_to_the_next_pass_node() {
    let run = |second_ime: bool| {
        let (mut state, mut session) = scene(PANE_L);
        let (parent, child) = parent_and_child(&mut state);
        let engine = session.borrow_mut();
        let read = engine.read();
        let ctx = egui::Context::default();
        let mut input = WindowInput::default();
        keyless_pass(&ctx, &mut state, &read, &mut input, None);
        let mut held = HeldPastes::default();
        let mut passes = 0;
        let (_, out) = input.run_pass(
            &mut state,
            &ctx,
            screen(),
            &mut held,
            &mut |_| {},
            |ctx, state| {
                passes += 1;
                draw_popup_layer(ctx, state, &read, &layout());
                if passes == 1 {
                    ctx.output_mut(|o| o.ime = Some(ime_output()));
                    state
                        .popups
                        .close_instance(child, PopupCloseReason::CloseButton);
                    ctx.request_discard("the child closed");
                } else if second_ime {
                    ctx.output_mut(|o| o.ime = Some(ime_output()));
                }
            },
        );
        assert_eq!(passes, 2, "egui 가 둘째 내부 패스를 돌렸다");
        assert!(
            out.platform_output.ime.is_some(),
            "최종 출력에 IME 출력이 남는다"
        );
        assert_eq!(
            state.focus_node(&read),
            FocusNode::Popup(PopupNode::Host(parent))
        );
        let drawn = state.closed_segment_focus;
        window_ime_allowed(&mut state, &read, drawn, true)
    };
    assert!(
        run(true),
        "대조: 둘째 패스가 낸 출력은 그 패스 노드의 근거다"
    );
    assert!(
        !run(false),
        "버린 패스의 A 출력은 둘째 패스 노드 B 를 켜지 않는다"
    );
}
