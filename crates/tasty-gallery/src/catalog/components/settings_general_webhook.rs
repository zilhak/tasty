//! General › General 마지막 행인 웹훅 외부 수신 스위치와 그 아래 경고 callout의 예제.
//! 실제 설정 저장은 하지 않는다.

use std::cell::RefCell;
use tasty_type_geometry::length::LogicalPx;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{select, switch, warning_callout};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 specimen 의 행 묶음 폭.
const WIDTH: LogicalPx = LogicalPx(640.0);

/// 본체 settings.general.webhook_allow_external_notice 의 영어 문구.
const NOTICE: &str = "When on, the webhook listener takes every network interface, so anyone \
     who can reach its port can call your registered webhooks. When off, only programs on this \
     computer can. Applies from the next start.";

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            restore: true,
            close: 0,
            language: 0,
            external: false,
        })
    };
}

struct State {
    restore: bool,
    close: usize,
    language: usize,
    external: bool,
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_lg, theme.spacing_lg, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    row(ui, theme, "Restore layout:", |ui| {
                        switch(ui, theme, &mut st.restore, None, true);
                    });
                    row(ui, theme, "Close behavior:", |ui| {
                        select(
                            ui,
                            theme,
                            "gallery_webhook_close",
                            &mut st.close,
                            &["Ask", "Minimize to background", "Quit"],
                            theme.field_width_lg.value(),
                            true,
                        );
                    });
                    row(ui, theme, "Language:", |ui| {
                        select(
                            ui,
                            theme,
                            "gallery_webhook_language",
                            &mut st.language,
                            &["English", "한국어", "日本語"],
                            theme.field_width_md.value(),
                            true,
                        );
                    });
                    row(
                        ui,
                        theme,
                        "Accept webhook calls from other computers:",
                        |ui| {
                            switch(ui, theme, &mut st.external, None, true);
                        },
                    );
                });
                ui.scope(|ui| {
                    ui.set_max_width(theme.measure_md.value());
                    warning_callout(ui, theme, NOTICE, &|ui, rect, c| {
                        icons::ALERT_TRIANGLE
                            .image(rect.height(), c)
                            .paint_at(ui, rect);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("placement", "General › General, last row (after Language)"),
            (
                "row",
                "settings-row grid · 150 label (wraps to 2 lines) · Switch",
            ),
            (
                "callout",
                "warning · alertTriangle 16 · always visible · measure-md",
            ),
            ("gap", "row ↔ callout = the section row gap (space-sm)"),
            ("setting", "[webhook] allow_external · default off"),
            ("apply", "Save / Cancel · from next start"),
        ],
        &[
            TokenChip::without_color("settings-row-min-height", "row"),
            TokenChip::new(
                "accent-warning",
                "callout edge + icon",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::without_color("measure-md", "callout width"),
        ],
    );

    spec::note(
        ui,
        theme,
        "Copy is confirmed as shipped: label “Accept webhook calls from other computers:”, callout as above (settings.general.webhook_allow_external_*). No new section: one row doesn't earn a “Webhooks” L2, and Handler › Hook Handlers is about what runs, not where the listener binds.",
    );
}

/// settings-row 한 행: `settings_label_width` 라벨 열(본문 크기 text-secondary, 넘치면 줄바꿈)
/// + `spacing_lg` gap + 컨트롤. 행 높이 하한은 `settings_row_min_height`.
fn row(ui: &mut egui::Ui, theme: &Theme, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    let min_h = theme.settings_row_min_height().value();
    ui.horizontal(|ui| {
        ui.set_min_height(min_h);
        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
        ui.allocate_ui_with_layout(
            egui::vec2(theme.settings_label_width().value(), min_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(theme.settings_label_width().value());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(theme.font_size_body.value())
                            .color(theme.text_secondary().to_egui()),
                    )
                    .wrap(),
                );
            },
        );
        control(ui);
    });
}
