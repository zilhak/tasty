//! `Input` primitive specimen — `tasty_ui_widgets::Input` 격리 카탈로그.
//!
//! 디자인 gallery `components.html` Input Spec 대조용. default · icon(search) ·
//! addon · mono · invalid · disabled. focus 시 ring(즉시). 상태는 egui memory 에
//! 남도록 specimen 마다 독립 버퍼를 thread_local 로 보관.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Input;

use super::glyph;

thread_local! {
    static BUFS: RefCell<[String; 6]> = const {
        RefCell::new([
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ])
    };
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.subtext0)),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);

    BUFS.with(|b| {
        let mut bufs = b.borrow_mut();

        caption(ui, theme, "default · icon(search) · addon — click to focus");
        ui.horizontal(|ui| {
            Input::new()
                .placeholder("Workspace name")
                .width(200.0)
                .show(ui, theme, &mut bufs[0]);
            Input::new()
                .placeholder("Filter…")
                .width(200.0)
                .icon(&|ui, rect, c| glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect))
                .show(ui, theme, &mut bufs[1]);
            Input::new()
                .mono(true)
                .addon("px")
                .width(110.0)
                .show(ui, theme, &mut bufs[2]);
        });

        ui.add_space(8.0);
        caption(ui, theme, "mono · invalid · disabled");
        ui.horizontal(|ui| {
            Input::new()
                .mono(true)
                .placeholder("s_01HXK9")
                .width(200.0)
                .show(ui, theme, &mut bufs[3]);
            Input::new()
                .invalid(true)
                .placeholder("bad value")
                .width(160.0)
                .show(ui, theme, &mut bufs[4]);
            Input::new()
                .enabled(false)
                .placeholder("Disabled")
                .width(160.0)
                .show(ui, theme, &mut bufs[5]);
        });
    });
}
