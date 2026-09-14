//! 칠하기 헬퍼 — 안내문 · 캡션 · 아이콘 · 고정 폭 라벨 · 말줄임 galley.
//!
//! 경계: 본체 `import_export/paint.rs` 와 같은 자리 — 두 Spec 이상이 함께 쓰는 원자 그리기다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::icons::MockGlyph;

/// 안내문 — 12 muted, line-height ui, max-width `measure`.
pub(super) fn intro(ui: &mut egui::Ui, theme: &Theme, measure: LogicalPx, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(measure.value());
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_term_sm.value())
                .color(theme.text_muted().to_egui()),
        );
    });
}

/// 카드 설명문 — 12 text-secondary, max-width measure-lg.
pub(super) fn intro_secondary(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(theme.measure_lg.value());
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_term_sm.value())
                .color(theme.text_secondary().to_egui()),
        );
    });
}

pub(super) fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

pub(super) fn glyph_at(ui: &mut egui::Ui, glyph: MockGlyph, size: LogicalPx, tint: egui::Color32) {
    let s = size.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
    glyph.image(s, tint).paint_at(ui, rect);
}

/// 고정 폭 한 줄 라벨(말줄임).
pub(super) fn fixed_label(
    ui: &mut egui::Ui,
    width: LogicalPx,
    text: &str,
    font: egui::FontId,
    fg: egui::Color32,
) {
    let g = truncated(ui, text, font, fg, width.value());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), g.rect.height()),
        egui::Sense::hover(),
    );
    ui.painter().galley(rect.min, g, egui::Color32::PLACEHOLDER);
}

pub(super) fn truncated(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.fonts(|f| f.layout_job(job))
}
