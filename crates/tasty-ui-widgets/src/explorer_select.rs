//! 탐색기 키보드 현재 항목 테두리와 영역 선택 사각형. 본체와 갤러리가 같은 함수로 그린다.

use tasty_type_appearance::theme::Theme;

/// 키보드 현재 항목: 칸 안쪽 1px 테두리. 선택 채움 위에 그리며 배치는 바꾸지 않는다.
pub fn paint_cursor_ring(painter: &egui::Painter, theme: &Theme, rect: egui::Rect) {
    painter.rect_stroke(
        rect,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(
            theme.explorer_cursor_ring_width().value(),
            theme.explorer_cursor_ring().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
}

/// 영역 선택 사각형: 강조 틴트 채움과 1px 테두리, 모서리 없음.
pub fn paint_marquee(painter: &egui::Painter, theme: &Theme, rect: egui::Rect) {
    painter.rect(
        rect,
        0.0,
        theme.explorer_marquee_bg().to_egui(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.explorer_marquee_border().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
}
