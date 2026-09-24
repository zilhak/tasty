//! 제목·본문·설명·고정폭 글꼴의 크기와 강조를 비교한다.

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
            (
                "UI cap",
                "font-size-max 14px at scale 1; larger roles are separate",
            ),
            (
                "heading",
                "body size; strong color approximates design weight 600",
            ),
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
        "디자인의 제목은 본문과 같은 기본 크기에 굵기 600을 사용한다. 이 egui 예제의 RichText::strong은 글꼴 굵기를 바꾸지 않고 강조색을 선택하므로 그 차이를 색으로 근사한다.",
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
