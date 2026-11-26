//! 패스 출력 입구의 IME 허용(`ime_permission::window_ime_allowed`). 대상은 입구가 정규화한 포커스 노드다(그 패스의
//! 닫기·덮음 갱신 반영).

use super::{PANE_L, scene};
use crate::adapters::ui::popup::focus::PopupNode;
use crate::adapters::ui::popup::{PopupInstanceId, PopupScope, PopupState};
use crate::gfx::gpu::ime_permission::window_ime_allowed;
use crate::intent::OpenRequest;
use crate::state::RequestContext;
use crate::state::focus_node::FocusNode;

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
