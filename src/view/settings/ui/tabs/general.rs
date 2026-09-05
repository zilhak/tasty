use crate::i18n::{LanguageEntry, t};
use crate::settings::Settings;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{LanguageOption, LanguageSelectLabels, language_select, vspace};

/// `languages` 는 설정 창이 연 시점에 스캔한 언어 목록(내장 3 + 언어팩 N —
/// `crate::i18n::available_languages`). 콤보는 그 목록만 보여 주고, 현재 설정값이 목록에
/// 없으면(팩 삭제 등) 값을 덮어쓰지 않고 `<code> (not found)` 행으로 유지한다.
/// 휠 노치 거리 슬라이더의 범위. 하한은 0 이 아니다 — 0 이면 휠이 아무 데서도 안
/// 움직여 설정 창을 스크롤해 되돌리는 것조차 막힌다. 상한은 한 노치가 화면을 통째로
/// 넘기지 않을 정도로 둔다.
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
            // 걸린다(ADR-0130) — 어느 한쪽만 바뀌면 같은 창에서 표면마다 이동량이 갈린다.
            ui.label(t("settings.general.wheel_line_scroll_label"));
            ui.add(
                egui::DragValue::new(&mut settings.general.wheel_line_scroll)
                    .range(WHEEL_LINE_SCROLL_MIN.value()..=WHEEL_LINE_SCROLL_MAX.value())
                    .speed(1.0)
                    .fixed_decimals(0)
                    .suffix(" pt"),
            );
            ui.end_row();

            ui.label(t("settings.general.close_behavior_label"));
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
}
