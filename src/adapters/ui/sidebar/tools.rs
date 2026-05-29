//! Sidebar 의 tools 버튼 핸들러 — popup 열기.

use crate::state::AppState;

/// 도구 버튼 위쪽, 좌측에 붙여서 tools_menu 팝업을 연다.
pub(crate) fn open_tools_menu(state: &mut AppState, btn_rect: egui::Rect) {
    // tools_menu의 default_size를 popup::defs에서 가져온다
    let menu_size = crate::adapters::ui::popup::defs::find("tools_menu")
        .map(|d| d.default_size)
        .unwrap_or(egui::vec2(160.0, 36.0));
    // 버튼 좌측에 맞추고, 버튼 위쪽으로 올라가도록 배치
    let pos = egui::pos2(btn_rect.min.x, btn_rect.min.y - menu_size.y);
    state.dispatch_intent(
        crate::intent::Intent::OpenPopup {
            id: "tools_menu",
            mode: crate::intent::OpenPopupMode::AtFocused(pos),
        }
        .from_user_menu("tools_button"),
    );
}
