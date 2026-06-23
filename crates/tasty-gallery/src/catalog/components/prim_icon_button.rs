//! `IconButton` primitive specimen — `tasty_ui_widgets::IconButton` 격리 카탈로그.
//!
//! 디자인 gallery `components.html` IconButton Spec 대조용. ghost(테두리 없음)/
//! solid/active × md/sm. 글리프는 `super::glyph`(디자인 icons.json 미러).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant};

use super::glyph;

use crate::catalog::specimen::caption;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

    caption(ui, theme, "ghost(기본·테두리 없음) · solid · active · sm");
    ui.horizontal(|ui| {
        IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .show(ui, theme, &|ui, rect, c| {
                glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect)
            });
        IconButton::new()
            .variant(IconButtonVariant::Solid)
            .show(ui, theme, &|ui, rect, c| {
                glyph::PLUS.image(rect.height(), c).paint_at(ui, rect)
            });
        IconButton::new()
            .active(true)
            .show(ui, theme, &|ui, rect, c| {
                glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect)
            });
        IconButton::new()
            .size(ControlSize::Sm)
            .show(ui, theme, &|ui, rect, c| {
                glyph::PLUS.image(rect.height(), c).paint_at(ui, rect)
            });
    });

    ui.add_space(8.0);
    caption(ui, theme, "disabled (opacity 0.45)");
    ui.horizontal(|ui| {
        IconButton::new()
            .enabled(false)
            .show(ui, theme, &|ui, rect, c| {
                glyph::CLOSE.image(rect.height(), c).paint_at(ui, rect)
            });
    });
}
