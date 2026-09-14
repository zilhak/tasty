//! 안내 — jsx `IeNotices`: 버린 plugin 정보 줄 · 마이그레이션 불필요 안내문 · 파싱 실패 블록.
//!
//! 경계: 본체 `import_export/notices.rs` 와 같은 자리 — 표·카드가 아닌 알림 자리의 그리기다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::catalog::icons;

use super::paint::{glyph_at, intro, intro_secondary};
use super::{CARD_PAD_X, FAILURE_BORDER, FAILURE_FILL, IE_DISCARDED, IE_FILE};

/// jsx `IeNotices` — 버린 plugin override 안내(정보, 경고 아님) · 마이그레이션 불필요 안내문 ·
/// 파싱 실패 인라인 블록.
pub(super) fn notices(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
    dropped_notice(ui, theme);
    intro(
        ui,
        theme,
        theme.measure_lg,
        &format!("{IE_FILE} — 7 of 73 bindings change. No option bindings to migrate."),
    );
    parse_failure(ui, theme);
}

fn dropped_notice(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        glyph_at(
            ui,
            icons::HELP_CIRCLE,
            theme.icon_glyph_size_sm,
            theme.text_muted().to_egui(),
        );
        ui.label(
            egui::RichText::new(format!(
                "2 plugin overrides were dropped — those plugins aren't installed here \
                 ({IE_DISCARDED})."
            ))
            .size(theme.font_size_term_sm.value())
            .color(theme.text_muted().to_egui()),
        );
    });
}

/// 파싱 실패 — 보던 대상에 대한 사실이라 toast/popup 이 아니라 detail 영역 안에 인라인.
fn parse_failure(ui: &mut egui::Ui, theme: &Theme) {
    let danger = theme.accent_danger().to_egui();
    egui::Frame::new()
        .fill(danger.gamma_multiply(FAILURE_FILL))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            danger.gamma_multiply(FAILURE_BORDER),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, theme.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                glyph_at(ui, icons::ALERT_CIRCLE, theme.icon_glyph_size_md, danger);
                ui.label(
                    egui::RichText::new("This file can't be read as keybindings")
                        .size(theme.font_size_body.value())
                        .color(danger),
                );
            });
            intro_secondary(
                ui,
                theme,
                "~/Downloads/settings.json — expected a keybinding export (TOML, a [keybindings] \
                 table); parsing stopped at line 1. Nothing was changed.",
            );
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            Button::new("Choose another file")
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, theme);
        });
}
