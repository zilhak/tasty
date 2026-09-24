use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

pub fn draw_accessibility_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_lg.value();
        ui.label(t("settings.accessibility.reduced_motion"));
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.accessibility.reduced_motion,
            None,
            true,
        );
    });
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.accessibility.reduced_motion_desc"))
            .small()
            .color(th.text_muted()),
    );

    vspace(ui, th.spacing_md);

    // Modifier 키 홀드 시 단축키 안내 오버레이 표시 토글 (동일 Row+Switch+Note 패턴).
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_lg.value();
        ui.label(t("settings.accessibility.modifier_hint"));
        tasty_ui_widgets::switch(ui, &th, &mut settings.modifier_hint.enabled, None, true);
    });
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.accessibility.modifier_hint_desc"))
            .small()
            .color(th.text_muted()),
    );
}
