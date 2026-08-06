//! 마커 오버레이 프리미티브 — 6번째 오버레이. 절대 rect 위에 그리는 독립 링
//! (+정적 glow) + 선택적 스포트라이트 scrim. 정적·무상태·hit-transparent
//! (painter 만, `Sense` 없음 → 클릭은 하위로 통과).
//!
//! 디자인 SoT `gallery/overlays-tutorial.jsx::Marker/Scrim` 의 host 대응.
//! 색·굵기·반경은 전부 `Theme` 토큰(신규 토큰 0). glow box-shadow 는 sanctioned
//! 일회성 오버레이 이펙트.

use tasty_type_appearance::theme::Theme;

/// 스포트라이트 scrim — `screen` 전체를 scrim-bg 로 덮되 `hole`(마커 rect)만
/// 밝게 남긴다. hole 을 뺀 4개 밴드(상/하/좌/우)를 채워 진짜 스포트라이트를 만든다
/// (마커는 스포트라이트가 강조하는 대상이므로 scrim 을 칠하지 않아 원래 밝기를 유지한다).
pub fn paint_spotlight_scrim(
    p: &egui::Painter,
    screen: egui::Rect,
    hole: egui::Rect,
    theme: &Theme,
) {
    let scrim = theme.scrim().to_egui();
    let hole = hole.intersect(screen);
    // 상단 밴드 (screen.top → hole.top).
    if hole.top() > screen.top() {
        p.rect_filled(
            egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, hole.top())),
            0.0,
            scrim,
        );
    }
    // 하단 밴드 (hole.bottom → screen.bottom).
    if hole.bottom() < screen.bottom() {
        p.rect_filled(
            egui::Rect::from_min_max(egui::pos2(screen.min.x, hole.bottom()), screen.max),
            0.0,
            scrim,
        );
    }
    // 좌측 밴드 (hole 높이 구간).
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
    // 우측 밴드.
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

/// 마커 링 — 2px accent-primary 링 + 정적 halo(glow). `rect` 는 대상 위젯 영역
/// (마커는 위젯 테두리를 건드리지 않고 그 위에 독립적으로 얹힌다).
pub fn paint_marker(p: &egui::Painter, rect: egui::Rect, theme: &Theme) {
    let accent = theme.accent_primary();
    let radius = theme.corner_radius.value();
    // 정적 halo — accent 저알파 확장 링 2겹 (sanctioned 일회성 이펙트).
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
    // 크리스프 2px 링.
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.focus_ring_width.value(), accent.to_egui()),
        egui::StrokeKind::Inside,
    );
}
