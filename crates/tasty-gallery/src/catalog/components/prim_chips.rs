//! Badge · Tag · Kbd primitive specimen — 디자인 gallery `components.html` 대조.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{badge, badge_dot, kbd, tag, BadgeVariant, TagVariant};

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.subtext0)),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

    caption(ui, theme, "Badge — count · dot");
    ui.horizontal(|ui| {
        badge(ui, theme, "3", BadgeVariant::Danger);
        badge(ui, theme, "99+", BadgeVariant::Danger);
        badge(ui, theme, "12", BadgeVariant::Primary);
        badge(ui, theme, "new", BadgeVariant::Agent);
        badge(ui, theme, "ok", BadgeVariant::Success);
        badge(ui, theme, "7", BadgeVariant::Neutral);
        ui.add_space(8.0);
        badge_dot(ui, theme, BadgeVariant::Danger);
        badge_dot(ui, theme, BadgeVariant::Agent);
        badge_dot(ui, theme, BadgeVariant::Success);
    });

    ui.add_space(8.0);
    caption(ui, theme, "Tag — variants (outlined default · accent · agent · state dot)");
    ui.horizontal(|ui| {
        tag(ui, theme, "terminal", TagVariant::Default, false);
        tag(ui, theme, "markdown", TagVariant::Accent, false);
        tag(ui, theme, "plugin", TagVariant::Agent, false);
        tag(ui, theme, "running", TagVariant::Success, true);
        tag(ui, theme, "readonly", TagVariant::Warning, true);
        tag(ui, theme, "error", TagVariant::Danger, true);
    });

    ui.add_space(8.0);
    caption(ui, theme, "Kbd — keycaps");
    ui.horizontal(|ui| {
        kbd(ui, theme, "Ctrl+K");
        kbd(ui, theme, "Ctrl+Shift+N");
        kbd(ui, theme, "Esc");
    });
}
