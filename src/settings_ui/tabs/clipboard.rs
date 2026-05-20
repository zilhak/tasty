use crate::i18n::t;
use crate::settings::Settings;

/// Clipboard 탭: 히스토리 기능 설정.
/// (복사/붙여넣기/줌 단축키 설정은 Keybindings 탭의 Clipboard/Zoom 서브탭으로 이관됨.)
pub fn draw_clipboard_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    ui.heading(t("settings.clipboard.history_heading"));
    ui.add_space(4.0);

    ui.checkbox(
        &mut settings.clipboard.history_enabled,
        t("settings.clipboard.history_enabled_label"),
    );
    egui::Grid::new("clipboard_history_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.clipboard.history_max_label"));
            ui.add(
                egui::DragValue::new(&mut settings.clipboard.history_max)
                    .range(1..=1000)
                    .speed(1),
            );
            ui.end_row();

            ui.label(t("settings.clipboard.poll_interval_ms_label"));
            ui.add(
                egui::DragValue::new(&mut settings.clipboard.poll_interval_ms)
                    .range(100..=10000)
                    .speed(50),
            );
            ui.end_row();
        });
    ui.label(
        egui::RichText::new(t("settings.clipboard.poll_interval_restart_notice"))
            .small()
            .color(th.yellow),
    );
}

