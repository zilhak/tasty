//! `Input` primitive specimen — `tasty_ui_widgets::Input` 격리 카탈로그.
//!
//! 디자인 gallery `components.html` Input Spec 대조용. default · icon(search) ·
//! addon · mono · invalid · disabled. focus 시 ring(즉시). 상태는 egui memory 에
//! 남도록 specimen 마다 독립 버퍼를 thread_local 로 보관.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Input;

use super::glyph;
use crate::catalog::specimen::caption;

thread_local! {
    static BUFS: RefCell<[String; 7]> = const {
        RefCell::new([
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ])
    };
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

        ui.add_space(8.0);
        caption(ui, theme, "block — fill container width (width 미지정 → 가용 폭)");
        ui.vertical(|ui| {
            ui.set_max_width(360.0);
            // width() 미호출 → Input 이 가용 폭을 채운다(디자인 `block`).
            Input::new()
                .placeholder("Type to search commands…")
                .icon(&|ui, rect, c| glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect))
                .show(ui, theme, &mut bufs[6]);
        });
    });
}
