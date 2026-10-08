use crate::i18n::{LanguageEntry, t};
use crate::settings::Settings;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    LanguageOption, LanguageSelectLabels, SettingsRow, language_select, settings_label_column,
    vspace,
};

/// 휠 한 칸의 이동 거리 범위. 0이면 설정 화면도 스크롤할 수 없으므로 제외한다.
const WHEEL_LINE_SCROLL_MIN: LogicalPx = LogicalPx(10.0);
const WHEEL_LINE_SCROLL_MAX: LogicalPx = LogicalPx(200.0);

pub fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings, languages: &[LanguageEntry]) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let alert = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        crate::adapters::ui::icons::ALERT_TRIANGLE
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let rows = [
        SettingsRow::new(t("settings.general.restore_layout_label")),
        SettingsRow::new(t("settings.general.restore_surface_content_label")),
        SettingsRow::new(t("settings.general.workspace_categories_label")),
        SettingsRow::new(t(
            "settings.general.workspace_switch_crosses_category_label",
        )),
        SettingsRow::new(t("settings.general.wheel_line_scroll_label"))
            .caption(t("settings.general.wheel_line_scroll_desc")),
        SettingsRow::new(t("settings.general.close_behavior_label")),
        SettingsRow::new(t("settings.general.language_label"))
            .caption(t("settings.general.language_restart_notice")),
        SettingsRow::new(t("settings.general.webhook_allow_external_label"))
            .warning(t("settings.general.webhook_allow_external_notice"), &alert),
    ];
    let col = settings_label_column(ui, &th, &rows);
    let [
        restore_layout,
        restore_content,
        categories,
        crosses_category,
        wheel_row,
        close_behavior,
        language,
        webhook,
    ] = rows;

    ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
    restore_layout.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(ui, &th, &mut settings.general.restore_layout, None, true);
    });
    restore_content.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.general.restore_surface_content,
            None,
            true,
        );
    });
    categories.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.general.workspace_categories_enabled,
            None,
            true,
        );
    });
    crosses_category.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(
            ui,
            &th,
            &mut settings.general.workspace_switch_crosses_category,
            None,
            true,
        );
    });

    // 휠 한 칸이 스크롤하는 거리. 이 한 값이 host UI 위젯과 plugin 표면 양쪽에
    // 걸린다(ADR-0015) — 어느 한쪽만 바뀌면 같은 창에서 표면마다 이동량이 갈린다.
    wheel_row.show(ui, &th, col, |ui| {
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
    });

    close_behavior.show(ui, &th, col, |ui| {
        tasty_egui_theme::with_popover_frame(ui, &th, |ui| {
            egui::ComboBox::from_id_salt("close_behavior")
                .selected_text(match settings.general.close_behavior.as_str() {
                    "quit" => t("settings.general.close_behavior_quit"),
                    "minimize" => t("settings.general.close_behavior_minimize"),
                    _ => t("settings.general.close_behavior_ask"),
                })
                .show_ui(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    tasty_ui_widgets::menu_option_value(
                        ui,
                        &th,
                        &mut settings.general.close_behavior,
                        "ask".to_string(),
                        t("settings.general.close_behavior_ask"),
                    );
                    tasty_ui_widgets::menu_option_value(
                        ui,
                        &th,
                        &mut settings.general.close_behavior,
                        "minimize".to_string(),
                        t("settings.general.close_behavior_minimize"),
                    );
                    tasty_ui_widgets::menu_option_value(
                        ui,
                        &th,
                        &mut settings.general.close_behavior,
                        "quit".to_string(),
                        t("settings.general.close_behavior_quit"),
                    );
                })
        });
    });

    language.show(ui, &th, col, |ui| {
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
    });

    // 경고 callout 은 스위치 상태와 관계없이 항상 그린다.
    webhook.show(ui, &th, col, |ui| {
        tasty_ui_widgets::switch(ui, &th, &mut settings.webhook.allow_external, None, true);
    });
}
