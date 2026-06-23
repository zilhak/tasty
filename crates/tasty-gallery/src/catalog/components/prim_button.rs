//! `Button` primitive specimen — `tasty_ui_widgets::Button` 격리 카탈로그.
//!
//! 디자인 gallery `components.html` 의 Button Spec 과 1:1 대조용. 본체 팝업과
//! **동일한** `tasty_ui_widgets::Button` 을 호출한다(mirror 아님 — demo=main).
//! variant(primary/secondary/ghost/danger/agent) × size(sm/md/lg) × state.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use super::glyph;

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.subtext0)),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

    caption(ui, theme, "variant — primary · secondary · ghost · danger · agent (md)");
    ui.horizontal(|ui| {
        Button::new("Primary").variant(ButtonVariant::Primary).show(ui, theme);
        Button::new("Secondary").variant(ButtonVariant::Secondary).show(ui, theme);
        Button::new("Ghost").variant(ButtonVariant::Ghost).show(ui, theme);
        Button::new("Danger").variant(ButtonVariant::Danger).show(ui, theme);
        Button::new("Agent").variant(ButtonVariant::Agent).show(ui, theme);
    });

    ui.add_space(8.0);
    caption(ui, theme, "size — sm(24) · md(28) · lg(32) (secondary)");
    ui.horizontal(|ui| {
        Button::new("Small").variant(ButtonVariant::Secondary).size(ControlSize::Sm).show(ui, theme);
        Button::new("Medium").variant(ButtonVariant::Secondary).size(ControlSize::Md).show(ui, theme);
        Button::new("Large").variant(ButtonVariant::Secondary).size(ControlSize::Lg).show(ui, theme);
    });

    ui.add_space(8.0);
    caption(ui, theme, "state — disabled (opacity 0.45)");
    ui.horizontal(|ui| {
        Button::new("Primary").variant(ButtonVariant::Primary).enabled(false).show(ui, theme);
        Button::new("Secondary").variant(ButtonVariant::Secondary).enabled(false).show(ui, theme);
        Button::new("Ghost").variant(ButtonVariant::Ghost).enabled(false).show(ui, theme);
    });

    ui.add_space(8.0);
    caption(ui, theme, "icon — leadingIcon · trailingIcon · 양쪽 (icon-size-md, gap space-sm)");
    ui.horizontal(|ui| {
        Button::new("New tab")
            .variant(ButtonVariant::Primary)
            .leading_icon(&|ui, rect, c| glyph::PLUS.image(rect.height(), c).paint_at(ui, rect))
            .show(ui, theme);
        Button::new("Settings")
            .variant(ButtonVariant::Secondary)
            .trailing_icon(&|ui, rect, c| glyph::SETTINGS.image(rect.height(), c).paint_at(ui, rect))
            .show(ui, theme);
        Button::new("Search")
            .variant(ButtonVariant::Ghost)
            .leading_icon(&|ui, rect, c| glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect))
            .trailing_icon(&|ui, rect, c| glyph::CLOSE.image(rect.height(), c).paint_at(ui, rect))
            .show(ui, theme);
    });

    ui.add_space(8.0);
    caption(ui, theme, "block — fill container width");
    Button::new("Block primary").variant(ButtonVariant::Primary).block(true).show(ui, theme);
}
