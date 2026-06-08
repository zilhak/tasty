//! `tasty_egui_theme::hint_text` 데모.
//!
//! 본체 (`tasty`) 의 모든 placeholder 텍스트가 같은 함수를 거치므로 여기서
//! 보이는 색이 곧 메인 앱에서 보이는 색이다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

thread_local! {
    static BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("TextEdit::hint_text(tasty_egui_theme::hint_text(&theme, \"...\"))")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    BUF.with(|b| {
        let mut buf = b.borrow_mut();
        ui.add(
            egui::TextEdit::singleline(&mut *buf)
                .hint_text(tasty_egui_theme::hint_text(theme, "Type something here…"))
                .desired_width(320.0),
        );
    });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(format!(
            "placeholder = #{:02x}{:02x}{:02x}",
            theme.placeholder.r, theme.placeholder.g, theme.placeholder.b
        ))
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
