//! Dialog helper 데모.
//!
//! 본체 `src/adapters/ui/dialog.rs::rename_popup_default_size()` 의 시각화 +
//! rename popup 의 시각 layout (title bar / TextEdit / 버튼) 을 mock 으로 표현.
//!
//! **Tier 2 범위**: popup frame 의 *시각 구성* 만. 실제 입력 처리, AppState mutation,
//! Enter/Escape 핸들링, callback 등은 *모두 빠져있다* — 그 부분은 Tier 3.
//!
//! - `rename_popup_default_size()` = `(280, TITLE_BAR_HEIGHT + CONTENT_MARGIN*2 + 64)` 상수.
//! - 본체 의존: 없음. 상수는 gallery 에 로컬 복제 (POC 후 공유 lib crate 분리 검토 — Tier 3 패턴 문서).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

use crate::catalog::popup_frame::{self, CONTENT_MARGIN, ContentInset, TITLE_BAR_HEIGHT};

/// 본체 dialog::rename_popup_default_size() 와 동등.
fn rename_popup_default_size() -> egui::Vec2 {
    egui::vec2(280.0, TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + 64.0)
}

thread_local! {
    static MOCK_BUF: RefCell<String> = RefCell::new(String::from("My Tab"));
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "rename_popup_default_size() → 280 × (TITLE_BAR_HEIGHT + CONTENT_MARGIN*2 + 64)",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!(
            "= {:.0} × {:.0} (logical px)",
            rename_popup_default_size().x,
            rename_popup_default_size().y,
        ))
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    // Popup frame mock: title bar + content area + 버튼.
    let size = rename_popup_default_size();
    popup_frame::draw(
        ui,
        theme,
        "Rename tab",
        size.x,
        size.y,
        ContentInset::INSET,
        |child_ui| {
            MOCK_BUF.with(|b| {
                let mut buf = b.borrow_mut();
                child_ui.add_sized(
                    [child_ui.available_width(), 22.0],
                    egui::TextEdit::singleline(&mut *buf)
                        .font(egui::FontId::proportional(theme.font_size_body.value()))
                        .margin(egui::Margin::symmetric(4, 2)),
                );
            });

            child_ui.add_space(8.0);
            child_ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 갤러리 mock — 클릭 응답 의도적으로 무시 (시각 layout 검증 전용).
                    let _cancel_resp = ui.button("Cancel");
                    let _save_resp = ui.button("Save");
                });
            });
        },
    );

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "⚠ POC visual mock — 실제 입력 처리 / AppState mutation 은 Tier 3 (draw_rename_popup).",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
