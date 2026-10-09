use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::{SettingsRow, settings_label_column, vspace};

pub fn draw_accessibility_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let rows = [
        SettingsRow::new(t("settings.accessibility.reduced_motion"))
            .caption(t("settings.accessibility.reduced_motion_desc")),
        // Modifier 키 홀드 시 단축키 안내 오버레이 표시 토글.
        SettingsRow::new(t("settings.accessibility.modifier_hint"))
            .caption(t("settings.accessibility.modifier_hint_desc")),
    ];
    let col = settings_label_column(ui, &th, &rows);
    let [reduced_motion, modifier_hint] = rows;
    ui.spacing_mut().item_spacing.y = th.settings_row_gap().value();

    reduced_motion.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.accessibility.reduced_motion,
            None,
            true,
        );
    });
    modifier_hint.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(ui, &th, &mut settings.modifier_hint.enabled, None, true);
    });
}
