use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

/// General › Overlay — 오버레이류(토스트 등) 표시 설정. 현재는 토스트 수명 1행.
///
/// 내부 저장은 ms(u64)지만 UI 는 사용자 멘탈 모델("몇 초")에 맞춰 초 단위로
/// 노출한다(소수 1자리, `" s"` suffix). DragValue 는 f32 초 위에서 편집하고
/// 즉시 0.5s step 으로 스냅해 ms 로 되쓴다(1.0~10.0s → 1000~10000ms).
pub fn draw_overlay_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let mut secs = settings.overlay.toast_duration_ms as f32 / 1000.0;

    egui::Grid::new("overlay_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.overlay.toast_duration"));
            let resp = ui.add(
                egui::DragValue::new(&mut secs)
                    .range(1.0..=10.0)
                    .speed(0.5)
                    .fixed_decimals(1)
                    .suffix(" s"),
            );
            if resp.changed() {
                // 0.5s step 스냅 후 ms 로 저장.
                let stepped = (secs * 2.0).round() / 2.0;
                settings.overlay.toast_duration_ms = (stepped * 1000.0).round() as u64;
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
