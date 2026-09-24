//! 입력을 받지 않고 대상 위에 강조 링과 조명을 그린다. 클릭은 아래 화면으로 전달된다.

use tasty_type_appearance::theme::Theme;

/// 대상 영역은 원래 밝기로 남기고 나머지 화면을 네 영역으로 나누어 어둡게 한다.
pub fn paint_spotlight_scrim(
    p: &egui::Painter,
    screen: egui::Rect,
    hole: egui::Rect,
    theme: &Theme,
) {
    let scrim = theme.scrim().to_egui();
    let hole = hole.intersect(screen);
    if hole.top() > screen.top() {
        p.rect_filled(
            egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, hole.top())),
            0.0,
            scrim,
        );
    }
    if hole.bottom() < screen.bottom() {
        p.rect_filled(
            egui::Rect::from_min_max(egui::pos2(screen.min.x, hole.bottom()), screen.max),
            0.0,
            scrim,
        );
    }
    if hole.left() > screen.left() {
        p.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(screen.min.x, hole.top()),
                egui::pos2(hole.left(), hole.bottom()),
            ),
            0.0,
            scrim,
        );
    }
    if hole.right() < screen.right() {
        p.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(hole.right(), hole.top()),
                egui::pos2(screen.max.x, hole.bottom()),
            ),
            0.0,
            scrim,
        );
    }
}

/// 대상 위에 독립된 강조 링과 고정된 번짐 효과를 그린다.
pub fn paint_marker(p: &egui::Painter, rect: egui::Rect, theme: &Theme) {
    let accent = theme.accent_primary();
    let radius = theme.corner_radius.value();
    for (grow, alpha) in [(5.0_f32, 60u8), (2.5, 110)] {
        p.rect_stroke(
            rect.expand(grow),
            radius + grow,
            egui::Stroke::new(
                theme.focus_ring_width.value() + grow,
                accent.with_alpha(alpha).to_egui(),
            ),
            egui::StrokeKind::Outside,
        );
    }
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.focus_ring_width.value(), accent.to_egui()),
        egui::StrokeKind::Inside,
    );
}
