use crate::i18n::{LanguageEntry, t};
use crate::settings::Settings;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{LanguageOption, LanguageSelectLabels, language_select, vspace};

/// 휠 한 칸의 이동 거리 범위. 0이면 설정 화면도 스크롤할 수 없으므로 제외한다.
const WHEEL_LINE_SCROLL_MIN: LogicalPx = LogicalPx(10.0);
const WHEEL_LINE_SCROLL_MAX: LogicalPx = LogicalPx(200.0);

pub fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings, languages: &[LanguageEntry]) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.restore_layout_label"));
            tasty_ui_widgets::switch(ui, &th, &mut settings.general.restore_layout, None, true);
            ui.end_row();

            ui.label(t("settings.general.restore_surface_content_label"));
            tasty_ui_widgets::switch(
                ui,
                &th,
                &mut settings.general.restore_surface_content,
                None,
                true,
            );
            ui.end_row();

            ui.label(t("settings.general.workspace_categories_label"));
            tasty_ui_widgets::switch(
                ui,
                &th,
                &mut settings.general.workspace_categories_enabled,
                None,
                true,
            );
            ui.end_row();

            ui.label(t(
                "settings.general.workspace_switch_crosses_category_label",
            ));
            tasty_ui_widgets::switch(
                ui,
                &th,
                &mut settings.general.workspace_switch_crosses_category,
                None,
                true,
            );
            ui.end_row();

            // 휠 한 칸이 스크롤하는 거리. 이 한 값이 host UI 위젯과 plugin 표면 양쪽에
            // 걸린다(ADR-0015) — 어느 한쪽만 바뀌면 같은 창에서 표면마다 이동량이 갈린다.
            ui.label(t("settings.general.wheel_line_scroll_label"));
            let mut wheel = settings.general.wheel_line_scroll as f64;
            if super::number::number_field(
                ui,
                &th,
                "general_wheel_line_scroll",
                &super::number::NumberSpec::int(
                    WHEEL_LINE_SCROLL_MIN.value() as f64,
                    WHEEL_LINE_SCROLL_MAX.value() as f64,
                )
                .suffix(t("settings.number.unit_pt")),
                &mut wheel,
            ) {
                settings.general.wheel_line_scroll = wheel as f32;
            }
            ui.end_row();

            ui.label(t("settings.general.close_behavior_label"));
            tasty_egui_theme::with_popover_frame(ui, &th, |ui| {
                egui::ComboBox::from_id_salt("close_behavior")
                    .selected_text(match settings.general.close_behavior.as_str() {
                        "quit" => t("settings.general.close_behavior_quit"),
                        "minimize" => t("settings.general.close_behavior_minimize"),
                        _ => t("settings.general.close_behavior_ask"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.general.close_behavior,
                            "ask".to_string(),
                            t("settings.general.close_behavior_ask"),
                        );
                        ui.selectable_value(
                            &mut settings.general.close_behavior,
                            "minimize".to_string(),
                            t("settings.general.close_behavior_minimize"),
                        );
                        ui.selectable_value(
                            &mut settings.general.close_behavior,
                            "quit".to_string(),
                            t("settings.general.close_behavior_quit"),
                        );
                    })
            });
            ui.end_row();

            ui.label(t("settings.general.language_label"));
            // 라벨 = `[meta] name`, 없으면 코드 (`LanguageEntry::label`).
            let options: Vec<LanguageOption<'_>> = languages
                .iter()
                .map(|l| LanguageOption {
                    code: &l.code,
                    label: l.label(),
                })
                .collect();
            let labels = LanguageSelectLabels {
                missing_suffix: t("settings.general.language_missing_suffix"),
            };
            language_select(
                ui,
                &th,
                "language_select",
                &mut settings.general.language,
                &options,
                &labels,
                th.field_width_lg.value(),
                true,
            );
            ui.end_row();
        });

    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.general.wheel_line_scroll_desc"))
            .small()
            .color(th.text_muted()),
    );

    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.general.language_restart_notice"))
            .small()
            .color(th.accent_warning()),
    );

    // 웹훅 외부 수신 허용. 토글 아래 경고 callout은 Terminal › TUI의 OSC 52 행과 같은 모양이다.
    vspace(ui, th.spacing_md);
    ui.horizontal(|ui| {
        ui.label(t("settings.general.webhook_allow_external_label"));
        tasty_ui_widgets::switch(ui, &th, &mut settings.webhook.allow_external, None, true);
    });
    vspace(ui, th.spacing_sm);
    tasty_ui_widgets::warning_callout(
        ui,
        &th,
        t("settings.general.webhook_allow_external_notice"),
        &|ui, rect, c| {
            crate::adapters::ui::icons::ALERT_TRIANGLE
                .image(rect.height(), c)
                .paint_at(ui, rect);
        },
    );
}
