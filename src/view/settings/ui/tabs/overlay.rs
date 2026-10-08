use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::{SettingsRow, settings_label_column, vspace};

/// 토스트 수명을 초 단위로 편집하고 밀리초로 저장한다.
/// 확정 시 0.5초 눈금과 1~10초 범위를 적용한다.
pub fn draw_overlay_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let mut secs = settings.overlay.toast_duration_ms as f64 / 1000.0;

    let row = SettingsRow::new(t("settings.overlay.toast_duration"))
        .caption(t("settings.overlay.toast_duration_desc"));
    let col = settings_label_column(ui, &th, [&row]);
    row.show(ui, &th, col, |ui| {
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
    });
}
