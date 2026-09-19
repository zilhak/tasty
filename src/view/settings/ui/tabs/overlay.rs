use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

/// General › Overlay — 오버레이류(토스트 등) 표시 설정. 현재는 토스트 수명 1행.
///
/// 내부 저장은 ms(u64)지만 UI 는 사용자 멘탈 모델("몇 초")에 맞춰 초 단위로
/// 노출한다(소수 1자리, `s` suffix). 숫자 칸은 설정 창의 한 모양([`super::number`])
/// 이고, 확정(blur / `↵`) 때 0.5s 눈금으로 스냅한 뒤 ms 로 되쓴다(1.0~10.0s →
/// 1000~10000ms).
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
