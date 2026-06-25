//! Foundations Type specimen — 디자인(4) "Type" Spec.
//!
//! Spec "Two families, hard 14px cap, hierarchy by weight". 두 패밀리(UI sans /
//! mono D2Coding), 14px UI 상한, 위계는 크기가 아니라 weight 로. typeScaleRow
//! 4 행 (heading 13/600 · body 13/400 · caption 11/400 · mono 14).

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage};

const SAMPLE: &str = "The quick brown fox jumps over the lazy dog";

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        scale_row(
            ui,
            theme,
            "heading",
            egui::RichText::new(SAMPLE)
                .size(theme.font_size_heading.value())
                .strong(),
        );
        scale_row(
            ui,
            theme,
            "body",
            egui::RichText::new(SAMPLE).size(theme.font_size_body.value()),
        );
        scale_row(
            ui,
            theme,
            "caption",
            egui::RichText::new(SAMPLE).size(theme.font_size_caption.value()),
        );
        scale_row(
            ui,
            theme,
            "mono",
            egui::RichText::new(SAMPLE)
                .monospace()
                .size(theme.font_size_max.value()),
        );
    });

    meta(
        ui,
        theme,
        &[
            ("UI cap", "font-size-max 14px — UI never exceeds"),
            ("heading", "body size + weight 600 (not larger)"),
            ("mono", "D2Coding, term/code surfaces"),
        ],
        &[
            TokenChip::new("font-ui", "sans family", ec(theme.text_primary())),
            TokenChip::new("font-mono", "D2Coding", ec(theme.text_secondary())),
            TokenChip::new("font-size-body", "13px", ec(theme.text_primary())),
            TokenChip::new("font-size-caption", "11px", ec(theme.text_muted())),
            TokenChip::new(
                "font-weight-semibold",
                "600 heading",
                ec(theme.accent_primary()),
            ),
        ],
    );
    note(
        ui,
        theme,
        "위계는 크기가 아니라 weight 로 — heading 은 body 와 같은 13px 에 600 weight 만 더한다. \
         egui RichText 는 normal / strong(600) 만 노출해 medium(500)·bold(700) 세분화는 미지원.",
    );
}

/// typeScaleRow — [150px 라벨(토큰 + px)] [샘플].
fn scale_row(ui: &mut egui::Ui, theme: &Theme, name: &str, sample: egui::RichText) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
        ui.allocate_ui(
            egui::vec2(
                theme.tab_width.value(),
                theme.item_height_interactive.value(),
            ),
            |ui| {
                ui.label(
                    egui::RichText::new(name)
                        .monospace()
                        .size(theme.font_size_micro.value())
                        .color(ec(theme.text_muted())),
                );
            },
        );
        ui.label(sample.color(ec(theme.text_primary())));
    });
}
