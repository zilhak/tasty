//! UI 문장 안의 CLI 조각(code run) 예제. "Hint text" 절의 두 번째 spec 으로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{UiCodeTokens, ui_copy};

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

const SAMPLE: &str =
    "Port 7420 is in use. Start Tasty with `--webhook-port 7421` or close the other app.";

const WHEN: &str = "The proportional UI face joins two hyphens into one dash, so `--webhook-port` \
reads as an en dash. Any CLI command, option or argument inside a UI sentence (error screen, \
banner, settings caption, result card) is drawn as a code run: mono at the sentence's own size, \
a surface-raised fill, 4px side padding, no vertical padding (the line height does not change), \
radius-sm, text-primary ink. The run never breaks inside; the sentence wraps around it. The copy \
keeps the literal option, so it can be typed back. Terminal content is not affected.";

/// 크기 라벨과 예문 한 벌.
fn sample(ui: &mut egui::Ui, theme: &Theme, label: &str, size: LogicalPx, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(label)
                .monospace()
                .size(theme.font_size_micro.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
        ui_copy(ui, theme, SAMPLE, size, color);
    });
}

/// `spec::spec`와 같은 제목·설명 배치. 설명 문장의 CLI 조각도 code run 으로 그린다.
fn heading(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing_xl.value());
    ui.label(
        egui::RichText::new("CLI runs inside UI copy")
            .size(theme.font_size_term_lg.value())
            .strong()
            .color(egui::Color32::from(theme.text_primary())),
    );
    ui.add_space(theme.spacing_xs.value());
    ui_copy(
        ui,
        theme,
        WHEN,
        theme.font_size_body,
        egui::Color32::from(theme.text_secondary()),
    );
    ui.add_space(theme.spacing_md.value());
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    heading(ui, theme);
    stage(ui, theme, StageVariant::Column, |ui| {
        ui.set_max_width(theme.measure_md.value());
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        sample(
            ui,
            theme,
            "13 body",
            theme.font_size_body,
            egui::Color32::from(theme.text_secondary()),
        );
        sample(
            ui,
            theme,
            "11 caption, muted",
            theme.font_size_caption,
            egui::Color32::from(theme.text_muted()),
        );
    });
    meta(
        ui,
        theme,
        &[
            (
                "font",
                "ui-code-font → font-mono · size = the sentence (1em)",
            ),
            ("fill", "ui-code-bg → surface-raised"),
            ("ink", "ui-code-fg → text-primary"),
            ("padding", "ui-code-padding-x 4 · no vertical"),
            ("radius", "ui-code-radius → radius-sm"),
            ("wrap", "never inside the run"),
            (
                "scope",
                "UI copy only · not terminal content · i18n strings keep the literal option",
            ),
        ],
        &[
            TokenChip::without_color("ui-code-font", "mono"),
            TokenChip::new("ui-code-bg", "fill", UiCodeTokens::of(theme).bg),
            TokenChip::new("ui-code-fg", "ink", UiCodeTokens::of(theme).fg),
            TokenChip::without_color("ui-code-padding-x", "→ space-xs"),
            TokenChip::without_color("ui-code-radius", "→ radius-sm"),
        ],
    );
}
