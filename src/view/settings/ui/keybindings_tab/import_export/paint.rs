//! 칠하기 헬퍼 — 안내문 · 아이콘 · 고정 폭 라벨 · 말줄임 galley.
//!
//! 경계: 특정 화면에 속하지 않고 두 화면 이상이 함께 쓰는 원자 그리기다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::adapters::ui::icons;

/// 안내문 — 12 muted, max-width `measure`.
pub(super) fn intro(ui: &mut egui::Ui, th: &Theme, measure: LogicalPx, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(measure.value());
        ui.label(
            egui::RichText::new(text)
                .size(th.font_size_term_sm.value())
                .color(th.text_muted()),
        );
    });
}

pub(super) fn glyph_at(
    ui: &mut egui::Ui,
    glyph: icons::Icon,
    size: LogicalPx,
    tint: egui::Color32,
) {
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
