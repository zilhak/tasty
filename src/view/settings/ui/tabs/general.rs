use super::choice::{Choice, choice_select};
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

    ui.spacing_mut().item_spacing.y = th.settings_row_gap().value();
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
    // 걸린다(docs/surfaces/terminal/index.md) — 어느 한쪽만 바뀌면 같은 창에서 표면마다 이동량이 갈린다.
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
        choice_select(
            ui,
            &th,
            "close_behavior",
            &mut settings.general.close_behavior,
            &[
                Choice {
                    value: "ask",
                    label: t("settings.general.close_behavior_ask"),
                },
                Choice {
                    value: "minimize",
                    label: t("settings.general.close_behavior_minimize"),
                },
                Choice {
                    value: "quit",
                    label: t("settings.general.close_behavior_quit"),
                },
            ],
            th.field_width_lg,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::settings::ui::select_probe as probe;

    fn draw(settings: &mut Settings) -> impl FnMut(&mut egui::Ui) + '_ {
        move |ui| draw_general_tab(ui, settings, &[])
    }

    /// 종료 동작 트리거는 바로 아래 언어 트리거와 같은 Select 상자(높이 select-height, 폭 field-width-lg)다.
    #[test]
    fn close_behavior_trigger_matches_the_language_trigger() {
        let th = crate::theme::theme();
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let out = probe::frame(&ctx, &mut draw(&mut settings));
        let close = probe::trigger_around(&out, t("settings.general.close_behavior_ask"))
            .expect("close behavior is drawn as a Select trigger");
        // 빈 언어 목록이라 현재 코드는 목록 밖 행으로 보인다.
        let lang_text = format!(
            "{} {}",
            settings.general.language,
            t("settings.general.language_missing_suffix")
        );
        let lang =
            probe::trigger_around(&out, &lang_text).expect("language is drawn as a Select trigger");
        assert_eq!(close.height(), th.select_height().value());
        assert_eq!(close.height(), lang.height());
        assert_eq!(close.width(), th.field_width_lg.value());
        assert_eq!(close.width(), lang.width());
    }

    /// 고른 옵션은 기존과 같은 문자열로 저장된다.
    #[test]
    fn picking_close_behavior_stores_the_same_strings() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        for (from, to, stored) in [
            (
                "settings.general.close_behavior_ask",
                "settings.general.close_behavior_quit",
                "quit",
            ),
            (
                "settings.general.close_behavior_quit",
                "settings.general.close_behavior_minimize",
                "minimize",
            ),
            (
                "settings.general.close_behavior_minimize",
                "settings.general.close_behavior_ask",
                "ask",
            ),
        ] {
            probe::pick(&ctx, &mut draw(&mut settings), t(from), t(to));
            assert_eq!(settings.general.close_behavior, stored);
        }
    }

    /// 설정 파일의 목록 밖 값은 그대로 보존되고 0번 옵션으로 바로잡을 수 있다.
    #[test]
    fn an_unknown_close_behavior_is_kept_until_a_listed_one_is_picked() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        settings.general.close_behavior = "bogus".into();
        let missing = format!("bogus {}", t("settings.choice_missing_suffix"));
        let out = probe::frame(&ctx, &mut draw(&mut settings));
        assert!(probe::trigger_around(&out, &missing).is_some());
        assert_eq!(settings.general.close_behavior, "bogus");
        probe::pick(
            &ctx,
            &mut draw(&mut settings),
            &missing,
            t("settings.general.close_behavior_ask"),
        );
        assert_eq!(settings.general.close_behavior, "ask");
    }
}
