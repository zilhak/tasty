use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

/// 토스트 수명을 초 단위로 편집하고 밀리초로 저장한다.
/// 확정 시 0.5초 눈금과 1~10초 범위를 적용한다.
pub fn draw_overlay_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let mut secs = settings.overlay.toast_duration_ms as f64 / 1000.0;

    egui::Grid::new("overlay_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.overlay.toast_duration"));
            if super::number::number_field(
                ui,
                &th,
                "overlay_toast_duration",
                &super::number::NumberSpec::int(1.0, 10.0)
                    .step(0.5)
                    .decimals(1)
                    .suffix(t("settings.number.unit_s")),
                &mut secs,
            ) {
                settings.overlay.toast_duration_ms = (secs * 1000.0).round() as u64;
            }
            ui.end_row();
        });

    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.overlay.toast_duration_desc"))
            .small()
            .color(th.text_muted()),
    );
}
