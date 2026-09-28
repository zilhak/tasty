//! 이동 대기 대상 표시. 대상 rect 안쪽의 대시 링과, 대상이 보이지 않을 때
//! 가장 가까운 보이는 컨테이너에 두는 move 글리프를 그린다.
//! 다른 테두리는 모두 실선이므로 대시 패턴으로 구분한다. 링은 대상 rect에서 가장 마지막에 그려
//! 대시가 아래 테두리를 덮고 간격으로 아래가 보이게 한다. 정적 표시이며 입력을 받지 않는다.

use tasty_type_appearance::theme::Theme;

/// rect 안쪽에 대시 링을 그린다. 이웃과 공유하는 divider를 덮지 않도록 선 전체를 rect 안에 둔다.
/// 변마다 대시 위상을 새로 시작한다. 가로 변은 폭 전체, 세로 변은 높이 전체를 덮어 모서리를 비우지 않는다.
pub fn paint_move_source_ring(painter: &egui::Painter, theme: &Theme, rect: egui::Rect) {
    let width = theme.move_source_ring_width().value();
    if rect.width() < width || rect.height() < width {
        return;
    }
    let stroke = egui::Stroke::new(width, theme.move_source_ring());
    let dash = theme.move_source_dash().value();
    let gap = theme.move_source_dash_gap().value();
    let inner = rect.shrink(width * 0.5);
    let edges = [
        [
            egui::pos2(rect.min.x, inner.min.y),
            egui::pos2(rect.max.x, inner.min.y),
        ],
        [
            egui::pos2(inner.max.x, rect.min.y),
            egui::pos2(inner.max.x, rect.max.y),
        ],
        [
            egui::pos2(rect.max.x, inner.max.y),
            egui::pos2(rect.min.x, inner.max.y),
        ],
        [
            egui::pos2(inner.min.x, rect.max.y),
            egui::pos2(inner.min.x, rect.min.y),
        ],
    ];
    for edge in edges {
        painter.extend(egui::Shape::dashed_line(&edge, stroke, dash, gap));
    }
}

/// 탭 칸과 워크스페이스 행에 두는 move 글리프의 한 변 길이.
pub fn move_source_glyph_size(theme: &Theme) -> f32 {
    theme.move_source_glyph_size().value()
}

/// `slot` 가운데에 move 글리프를 그린다. 텍스트는 두지 않는다.
pub fn paint_move_source_glyph(ui: &egui::Ui, theme: &Theme, slot: egui::Rect) {
    let size = move_source_glyph_size(theme);
    let rect = egui::Rect::from_center_size(slot.center(), egui::vec2(size, size));
    tasty_icons::MOVE
        .image(size, theme.move_source_glyph().into())
        .paint_at(ui, rect);
}

/// 접힌 레일 아바타의 왼쪽 아래 모서리에 move 칩을 그린다.
/// 오른쪽 위는 알림 점, 오른쪽 아래는 mirror 칩이 쓴다. 칩은 아바타 밖으로 border 폭만큼 나온다.
/// `bed`는 칩 바탕색으로, 레일 배경과 같게 전달한다.
pub fn paint_move_source_chip(
    ui: &egui::Ui,
    theme: &Theme,
    avatar: egui::Rect,
    bed: egui::Color32,
) {
    let chip = theme.move_source_chip_size().value();
    let outset = theme.border_width.value();
    let rect = egui::Rect::from_min_size(
        egui::pos2(avatar.min.x - outset, avatar.max.y + outset - chip),
        egui::vec2(chip, chip),
    );
    ui.painter().circle_filled(rect.center(), chip * 0.5, bed);
    let glyph = theme.move_source_chip_glyph_size().value();
    let glyph_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    tasty_icons::MOVE
        .image(glyph, theme.move_source_glyph().into())
        .paint_at(ui, glyph_rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl FnOnce(&egui::Ui, &Theme)) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let theme = tasty_themes::mocha_fallback();
        let mut f = Some(f);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(f) = f.take() {
                    f(ui, &theme);
                }
            });
        });
        out.shapes
    }

    fn ring_segments(rect: egui::Rect) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        let theme = tasty_themes::mocha_fallback();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            paint_move_source_ring(&painter, &theme, rect);
        });
        out.shapes.into_iter().map(|c| c.shape).collect()
    }

    #[test]
    fn ring_stays_inside_the_rect() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 40.0));
        let shapes = ring_segments(rect);
        assert!(!shapes.is_empty());
        for shape in &shapes {
            let egui::Shape::LineSegment { points, stroke } = shape else {
                panic!("dashes are line segments: {shape:?}");
            };
            let half = stroke.width * 0.5;
            for p in points {
                // 선의 바깥 가장자리까지 rect 안에 있어야 이웃 divider를 덮지 않는다.
                assert!(p.x >= rect.min.x - 0.01 && p.x <= rect.max.x + 0.01);
                assert!(p.y >= rect.min.y - 0.01 && p.y <= rect.max.y + 0.01);
            }
            let horizontal = (points[0].y - points[1].y).abs() < 0.01;
            if horizontal {
                assert!(points[0].y - half >= rect.min.y - 0.01);
                assert!(points[0].y + half <= rect.max.y + 0.01);
            } else {
                assert!(points[0].x - half >= rect.min.x - 0.01);
                assert!(points[0].x + half <= rect.max.x + 0.01);
            }
        }
    }

    #[test]
    fn ring_is_dashed_not_solid() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(64.0, 32.0));
        // 4px 대시와 4px 간격이면 64px 변 하나에 대시가 8개 나온다.
        assert!(ring_segments(rect).len() > 4);
    }

    #[test]
    fn ring_skips_rects_thinner_than_the_stroke() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 40.0));
        assert!(ring_segments(rect).is_empty());
    }

    #[test]
    fn glyph_and_chip_paint_without_panic() {
        let shapes = run(|ui, theme| {
            let slot = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(12.0, 12.0));
            paint_move_source_glyph(ui, theme, slot);
            let avatar = egui::Rect::from_min_size(egui::pos2(20.0, 0.0), egui::vec2(28.0, 28.0));
            paint_move_source_chip(ui, theme, avatar, theme.bg_sidebar().into());
        });
        assert!(!shapes.is_empty());
    }
}
