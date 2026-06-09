//! Sidebar 의 tools 버튼 핸들러 — popup 열기.

use crate::state::AppState;

/// 도구 버튼 위쪽, 좌측에 붙여서 tools_menu 팝업을 연다.
///
/// menu_size 는 *현재 등록된 plugin 도구 수까지 반영한 동적 크기* 를 받는다.
/// PopupDef.sizer 가 매 프레임 재계산하지만 popup pos 는 open 시점에 한 번만
/// 결정되므로, 정확한 height 로 botton-up 정렬하려면 여기서 같은 식으로 계산해야 한다.
pub(crate) fn open_tools_menu(
    state: &mut AppState,
    engine: &crate::core::CoreState,
    btn_rect: egui::Rect,
) {
    let menu_size = crate::adapters::ui::tools_menu::tools_menu_current_size(state, engine);
    // 버튼 좌측에 맞추고, 버튼 위쪽으로 올라가도록 배치
    let pos = egui::pos2(btn_rect.min.x, btn_rect.min.y - menu_size.y);
    state.dispatch_intent(
        crate::intent::UiIntent::OpenPopup {
            id: "tools_menu",
            mode: crate::intent::OpenPopupMode::AtFocused(pos),
        }
        .from_user_menu("tools_button"),
    );
}
