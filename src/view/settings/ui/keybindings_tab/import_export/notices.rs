//! 안내 — 버린 plugin 정보 줄과 파싱 실패 인라인 블록(jsx `IeNotices`).
//!
//! 경계: detail 본문의 표·카드가 아닌 알림 자리의 그리기다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, vspace};

use crate::adapters::ui::icons;
use crate::i18n::{t, t_fmt, t_fmt2};

use super::paint::glyph_at;
use super::{CARD_PAD_X, FAILURE_BORDER, FAILURE_FILL, Failure};

/// 버린 plugin override 안내 — 정보(경고 아님): 잘못된 것도 할 일도 없다.
pub(super) fn dropped_notice(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        glyph_at(
            ui,
            icons::HELP_CIRCLE,
            th.icon_glyph_size_sm,
            th.text_muted().to_egui(),
        );
        ui.label(
            egui::RichText::new(text)
                .size(th.font_size_term_sm.value())
                .color(th.text_muted()),
        );
    });
}

/// 파싱 실패 — 보던 대상에 대한 사실이라 toast/popup 이 아니라 detail 영역 안에 인라인.
/// "다른 파일 고르기" 가 눌리면 true.
pub(super) fn parse_failure(ui: &mut egui::Ui, th: &Theme, failure: &Failure) -> bool {
    let danger = th.accent_danger().to_egui();
    let mut clicked = false;
    egui::Frame::new()
        .fill(danger.gamma_multiply(FAILURE_FILL))
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            danger.gamma_multiply(FAILURE_BORDER),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                glyph_at(ui, icons::ALERT_CIRCLE, th.icon_glyph_size_md, danger);
                ui.label(
                    egui::RichText::new(t("settings.keybindings.ie_failure_title"))
                        .size(th.font_size_body.value())
                        .color(danger),
                );
            });
            let body = match failure.line {
                Some(line) => t_fmt2(
                    "settings.keybindings.ie_failure_body_line",
                    &failure.path,
                    &line.to_string(),
                ),
                None => t_fmt("settings.keybindings.ie_failure_body", &failure.path),
            };
            ui.scope(|ui| {
                ui.set_max_width(th.measure_md.value());
                ui.label(
                    egui::RichText::new(body)
                        .size(th.font_size_term_sm.value())
                        .color(th.text_secondary()),
                );
            });
            vspace(ui, th.spacing_xs);
            clicked = Button::new(t("settings.keybindings.ie_choose_another"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, th)
                .clicked();
        });
    clicked
}
