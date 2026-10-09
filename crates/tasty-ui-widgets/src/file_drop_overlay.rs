//! 터미널 위로 OS 파일을 끌어 올 때의 창 드롭 안내. 본체와 갤러리가 같이 그린다.

use tasty_type_appearance::theme::Theme;

/// `rect`(터미널 영역) 안쪽 `spacing-sm` 에 채움과 1px 테두리를 그리고, 가운데에 download 글리프와
/// 라벨을 한 줄로 놓는다. 색은 `file-drop-overlay-*` 토큰이다.
pub fn file_drop_overlay(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, label: &str) {
    let painter = ui.painter();
    let radius = theme.corner_radius.value();
    let fg = theme.file_drop_overlay_fg().to_egui();
    let inner = rect.shrink(theme.spacing_sm.value());
    painter.rect_filled(inner, radius, theme.file_drop_overlay_bg().to_egui());
    painter.rect_stroke(
        inner,
        radius,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.file_drop_overlay_border().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );

    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        fg,
    );
    let glyph = theme.icon_glyph_size_md.value();
    let gap = theme.spacing_sm.value();
    let width = glyph + gap + galley.size().x;
    let left = inner.center().x - width / 2.0;
    let glyph_rect = egui::Rect::from_min_size(
        egui::pos2(left, inner.center().y - glyph / 2.0),
        egui::vec2(glyph, glyph),
    );
    tasty_icons::DOWNLOAD
        .image(glyph, fg)
        .paint_at(ui, glyph_rect);
    let text_pos = egui::pos2(
        glyph_rect.right() + gap,
        inner.center().y - galley.size().y / 2.0,
    );
    painter.galley(text_pos, galley, fg);
}
