use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

pub fn draw_accessibility_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    // 디자인 settings_window.jsx:443 — Row(label 좌, Switch 우) + Note. egui
    // `ui.heading()` 은 디자인에 없으므로 제거(L2 사이드바가 "Accessibility" 표시).
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
            .color(th.subtext0),
    );
}
