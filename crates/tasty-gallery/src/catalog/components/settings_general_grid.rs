//! General › General 의 행 격자 예제. 라벨 열은 가장 긴 라벨을 150 … 240 으로 clamp 한 폭이고,
//! 휠 거리·언어 행은 자기 행 아래 caption 을, 웹훅 외부 수신 행은 경고 callout 을 가진다.
//! 실제 설정 저장은 하지 않는다.

use std::cell::RefCell;
use tasty_type_geometry::length::LogicalPx;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, SettingsRow, select, settings_label_column, switch};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 specimen 의 행 묶음 폭.
const WIDTH: LogicalPx = LogicalPx(640.0);

/// 본체 settings.general.* 의 영어 문구.
fn wheel_caption() -> &'static str {
    crate::i18n::t("settings.general.wheel_line_scroll_desc")
}
fn language_caption() -> &'static str {
    crate::i18n::t("settings.general.language_restart_notice")
}
fn notice() -> &'static str {
    crate::i18n::t("settings.general.webhook_allow_external_notice")
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        restore: true,
        close: 0,
        wheel: "50".to_owned(),
        language: 0,
        external: false,
    });
}

struct State {
    restore: bool,
    close: usize,
    wheel: String,
    language: usize,
    external: bool,
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_lg, theme.spacing_lg, |ui| {
                ui.spacing_mut().item_spacing.y = theme.settings_row_gap().value();
                let alert = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
                    icons::ALERT_TRIANGLE
                        .image(rect.height(), c)
                        .paint_at(ui, rect);
                };
                let rows = [
                    SettingsRow::new("Restore layout:"),
                    SettingsRow::new("Close behavior:"),
                    SettingsRow::new(crate::i18n::t("settings.general.wheel_line_scroll_label"))
                        .caption(wheel_caption()),
                    SettingsRow::new("Language:").caption(language_caption()),
                    SettingsRow::new(crate::i18n::t(
                        "settings.general.webhook_allow_external_label",
                    ))
                    .warning(notice(), &alert),
                ];
                let col = settings_label_column(ui, theme, &rows);
                let [restore, close, wheel, language, webhook] = rows;
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    restore.show(ui, theme, col, |ui| {
                        switch(ui, theme, &mut st.restore, None, true);
                    });
                    close.show(ui, theme, col, |ui| {
                        select(
                            ui,
                            theme,
                            "gallery_general_close",
                            &mut st.close,
                            &[
                                "Ask",
                                crate::i18n::t("settings.general.close_behavior_minimize"),
                                "Quit",
                            ],
                            theme.field_width_lg.value(),
                            true,
                        );
                    });
                    wheel.show(ui, theme, col, |ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        Input::new()
                            .mono(true)
                            .width(theme.field_width_xs.value())
                            .show(ui, theme, &mut st.wheel);
                        ui.label(
                            egui::RichText::new("pt")
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    });
                    language.show(ui, theme, col, |ui| {
                        select(
                            ui,
                            theme,
                            "gallery_general_language",
                            &mut st.language,
                            &["English", "한국어", "日本語"],
                            theme.field_width_lg.value(),
                            true,
                        );
                    });
                    webhook.show(ui, theme, col, |ui| {
                        switch(ui, theme, &mut st.external, None, true);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "label column",
                "longest label of the subtab, clamp 150 … 240, wraps past 240 (en: webhook label → 240, 2 lines)",
            ),
            ("gap", "16 label → control"),
            ("between rows", "settings-row-gap 12"),
            (
                "row caption",
                "directly under its row · gap 4 · left edge · measure-md · caption muted",
            ),
            (
                "callout",
                "same slot as a caption · warning · alertTriangle 16 · always visible · measure-md",
            ),
            ("placement", "webhook row last (after Language)"),
            (
                "setting",
                "[webhook] allow_external · default off · Save / Cancel · from next start",
            ),
        ],
        &[
            TokenChip::without_color("settings-label-width", "label column floor 150"),
            TokenChip::without_color("settings-label-max-width", "label column cap 240"),
            TokenChip::without_color("settings-label-gap", "16"),
            TokenChip::without_color("settings-row-gap", "between rows 12"),
            TokenChip::without_color("settings-row-caption-gap", "row → caption 4"),
            TokenChip::new(
                "accent-warning",
                "callout edge + icon",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::without_color("measure-md", "caption / callout width"),
        ],
    );

    spec::note(
        ui,
        theme,
        "Copy is the app's shipped text (settings.general.*). Every settings subtab draws this grid with tasty_ui_widgets::SettingsRow: the column is measured per subtab and locale.",
    );
    // 좁은 폭에서 행이 쌓이는 모습은 같은 행 격자의 연장이라 이 페이지 끝에 둔다.
    super::settings_narrow_rows::draw(ui, theme);
}
