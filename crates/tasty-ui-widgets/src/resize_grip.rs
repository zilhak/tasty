//! 패널 오른쪽 아래 코너의 리사이즈 그립(대각선 두 획).

use tasty_type_appearance::theme::Theme;

/// 시안 SVG 의 viewBox 한 변. 획 좌표(`M11 5 5 11M11 9 9 11`)를 이 값으로 나눠 그립 크기에 비례시킨다.
const VIEWBOX: f32 = 12.0;
/// 두 획의 viewBox 좌표 — (시작 x, 시작 y, 끝 x, 끝 y).
const STROKES: [[f32; 4]; 2] = [[11.0, 5.0, 5.0, 11.0], [11.0, 9.0, 9.0, 11.0]];

/// `rect` 의 오른쪽 아래에서 `inset` 만큼 안쪽에 놓인 한 변 `size` 상자 안의 두 획 끝점.
pub fn resize_grip_segments(rect: egui::Rect, size: f32, inset: f32) -> [[egui::Pos2; 2]; 2] {
    let origin = egui::pos2(rect.right() - inset - size, rect.bottom() - inset - size);
    let at = |x: f32, y: f32| origin + egui::vec2(x, y) * (size / VIEWBOX);
    STROKES.map(|[x0, y0, x1, y1]| [at(x0, y0), at(x1, y1)])
}

/// modhint 패널의 코너 그립을 그린다. 크기 `modhint-grip-size`, 색 `modhint-grip-fg`, 굵기 border-width,
/// 오른쪽·아래 여백 `modhint-grip-inset`(UI 배율을 따른다).
pub fn modhint_resize_grip(painter: &egui::Painter, theme: &Theme, panel: egui::Rect) {
    let bw = theme.border_width.value();
    let stroke = egui::Stroke::new(bw, theme.modhint_grip_fg().to_egui());
    for seg in resize_grip_segments(
        panel,
        theme.modhint_grip_size().value(),
        theme.modhint_grip_inset().value(),
    ) {
        painter.line_segment(seg, stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::resize_grip_segments;

    #[test]
    fn the_strokes_follow_the_kit_svg_path() {
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 50.0));
        // 상자는 x 86..98, y 36..48 이다.
        let [a, b] = resize_grip_segments(rect, 12.0, 2.0);
        assert_eq!(a, [egui::pos2(97.0, 41.0), egui::pos2(91.0, 47.0)]);
        assert_eq!(b, [egui::pos2(97.0, 45.0), egui::pos2(95.0, 47.0)]);
        // 배율 2: 상자 x 72..96, y 22..46.
        let [a, _] = resize_grip_segments(rect, 24.0, 4.0);
        assert_eq!(a, [egui::pos2(94.0, 32.0), egui::pos2(82.0, 44.0)]);
    }
}
