//! 도구 메뉴 팝업. 사이드바의 "도구" 버튼 위에 떠서 도구 목록을 보여준다.

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

pub fn draw_tools_menu(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let th = theme::theme();
    let width = ui.available_width();

    // Clipboard History
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
    }
    ui.painter().text(
        egui::pos2(rect.min.x + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        t("tools_menu.clipboard_history"),
        egui::FontId::proportional(th.font_size_body.value()),
        if resp.hovered() { th.text.into() } else { th.subtext0.into() },
    );
    if resp.clicked() {
        crate::clipboard_viewer_ui::open_clipboard_viewer_popup(state);
        return PopupAction::Close;
    }

    PopupAction::None
}
