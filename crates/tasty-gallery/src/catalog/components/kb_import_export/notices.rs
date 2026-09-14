//! 안내 — jsx `IeNotices` · `IeBlockG`: 버린 plugin 정보 줄 · 마이그레이션 불필요 안내문 ·
//! 알림 블록(파싱 실패 · 내보내기 실패 · 번들 경고).
//!
//! 경계: 본체 `import_export/notices.rs` 와 같은 자리 — 표·카드가 아닌 알림 자리의 그리기다.
//!
//! 알림 블록은 **레시피 하나**다(jsx `IeBlockG`) — 톤 · 글리프 · 제목(+개수) · 본문 · 액션 행.
//! 실패와 경고는 톤만 다르다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::catalog::icons::{self, MockGlyph};

use super::paint::{glyph_at, intro, intro_secondary};
use super::{GROUP_CHEVRON_GAP, IE_DISCARDED, IE_FILE, NOTICE_BLOCK_BORDER, NOTICE_BLOCK_FILL};

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
    parse_failure(ui, theme, true);
}

pub(super) fn dropped_notice(ui: &mut egui::Ui, theme: &Theme) {
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

/// jsx `IeBlockG` — 톤 틴트 배경 + 톤 테두리 · 헤더(글리프 · 제목 · 우측 개수) · 본문 · 액션
/// 행(버튼 라벨과 variant). 눌린 액션의 인덱스를 돌려준다.
// reason: 레시피의 칸(톤 · 글리프 · 제목 · 개수 · 본문 · 액션)이 그대로 인자다 — 본체 짝과 같은
// 모양이다.
#[allow(clippy::too_many_arguments)]
pub(super) fn notice_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    tone: egui::Color32,
    glyph: MockGlyph,
    title: &str,
    count: Option<&str>,
    body: impl FnOnce(&mut egui::Ui),
    actions: &[(&str, ButtonVariant)],
) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::new()
        .fill(tone.gamma_multiply(NOTICE_BLOCK_FILL))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            tone.gamma_multiply(NOTICE_BLOCK_BORDER),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_all(theme.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                glyph_at(ui, glyph, theme.icon_glyph_size_md, tone);
                ui.label(
                    egui::RichText::new(title)
                        .size(theme.font_size_body.value())
                        .color(tone),
                );
                if let Some(count) = count {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(count)
                                .monospace()
                                .size(theme.font_size_caption.value())
                                .color(tone),
                        );
                    });
                }
            });
            body(ui);
            if !actions.is_empty() {
                tasty_ui_widgets::vspace(ui, theme.spacing_xs);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    for (i, (label, variant)) in actions.iter().enumerate() {
                        if Button::new(label)
                            .variant(*variant)
                            .size(ControlSize::Sm)
                            .show(ui, theme)
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                });
            }
        });
    clicked
}

/// 파싱 실패 — 보던 대상에 대한 사실이라 toast/popup 이 아니라 detail 영역 안에 인라인.
/// 줄 번호가 없으면 줄 구절을 **빼지 않고 바꾼다**(jsx `IeParseFailG`).
pub(super) fn parse_failure(ui: &mut egui::Ui, theme: &Theme, with_line: bool) {
    let why = if with_line {
        "parsing stopped at line 1."
    } else {
        "the file isn't TOML."
    };
    notice_block(
        ui,
        theme,
        theme.accent_danger().to_egui(),
        icons::ALERT_CIRCLE,
        "This file can't be read as keybindings",
        None,
        |ui| {
            intro_secondary(
                ui,
                theme,
                &format!(
                    "~/Downloads/settings.json — expected a keybinding export (TOML, a \
                     [keybindings] table); {why} Nothing was changed."
                ),
            );
        },
        &[("Choose another file", ButtonVariant::Secondary)],
    );
}

/// 번들 경고 한 줄 — 흐린 글머리 · 12 text-secondary.
pub(super) fn notice_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GROUP_CHEVRON_GAP.value();
        ui.label(
            egui::RichText::new("·")
                .size(theme.font_size_term_sm.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_term_sm.value())
                .color(theme.text_secondary().to_egui()),
        );
    });
}
