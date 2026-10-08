//! 입력을 받지 않고 대상 위에 강조 링과 조명을 그린다. 클릭은 아래 화면으로 전달된다.

use tasty_type_appearance::theme::Theme;

/// scrim과 마커를 말풍선 레이어에 그린다. 말풍선보다 먼저 호출해야 말풍선이 그 위에 원래 밝기로
/// 합성된다. egui는 같은 Order 안에서 Area가 아닌 레이어를 Area 레이어보다 나중에 합성하므로, 따로
/// 만든 레이어에 그리면 scrim이 말풍선을 덮는다.
pub fn paint_spotlight(ctx: &egui::Context, screen: egui::Rect, hole: egui::Rect, theme: &Theme) {
    let painter = ctx.layer_painter(super::callout::callout_layer());
    paint_spotlight_scrim(&painter, screen, hole, theme);
    paint_marker(&painter, hole, theme);
}

/// 대상 영역은 원래 밝기로 남기고 나머지 화면을 네 영역으로 나누어 어둡게 한다.
fn paint_spotlight_scrim(p: &egui::Painter, screen: egui::Rect, hole: egui::Rect, theme: &Theme) {
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
fn paint_marker(p: &egui::Painter, rect: egui::Rect, theme: &Theme) {
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

#[cfg(test)]
mod tests {
    use super::super::callout::{self, CalloutProps};
    use super::*;

    /// 실제 경로(paint_spotlight → draw_callout)로 그린 프레임에서 말풍선 배경이 scrim보다 뒤에
    /// 합성되는지 본다. FullOutput.shapes는 합성 순서대로 나열된다.
    #[test]
    fn callout_is_composited_above_the_spotlight_scrim() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        // TabHeader 단계처럼 화면 위쪽 띠를 가리키면 말풍선은 그 아래(scrim 영역)에 놓인다.
        let hole = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(1200.0, 28.0));
        let size = egui::vec2(callout::CALLOUT_W.value(), 160.0);
        let placement = callout::place_callout(
            hole,
            size,
            screen,
            theme.spacing_md.value(),
            theme.spacing_sm.value(),
        );
        assert!(
            placement.pos.y >= hole.bottom(),
            "callout sits outside the hole"
        );
        let frame = |ctx: &egui::Context| {
            paint_spotlight(ctx, screen, hole, &theme);
            callout::draw_callout(
                ctx,
                &theme,
                placement,
                CalloutProps {
                    step: 3,
                    total: 5,
                    title: "Tabs",
                    body: "body",
                    first: false,
                    last: false,
                    ready: true,
                    prepare: false,
                    size,
                    keyboard_focus: false,
                    anchored: true,
                },
            );
        };
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        // 새 Area의 첫 프레임은 크기만 재는 보이지 않는 패스라 두 번째 프레임을 본다.
        drop(ctx.run(input(), frame));
        let out = ctx.run(input(), frame);
        // Frame은 그림자와 배경을 Shape::Vec 하나로 묶어 내보낸다. 말풍선 배경은 Area 나타남 효과로
        // 색의 alpha가 프레임마다 달라 색 대신 배치 위치로 찾는다.
        fn has_rect(shape: &egui::Shape, hit: &dyn Fn(&egui::epaint::RectShape) -> bool) -> bool {
            match shape {
                egui::Shape::Rect(r) => hit(r),
                egui::Shape::Vec(v) => v.iter().any(|s| has_rect(s, hit)),
                _ => false,
            }
        }
        let last = |hit: &dyn Fn(&egui::epaint::RectShape) -> bool| {
            out.shapes.iter().rposition(|s| has_rect(&s.shape, hit))
        };
        let scrim_fill = theme.scrim().to_egui();
        let scrim = last(&|r| r.fill == scrim_fill).expect("scrim drawn");
        let raised =
            last(&|r| r.rect.min == placement.pos && r.blur_width == 0.0).expect("callout drawn");
        assert!(
            raised > scrim,
            "callout background ({raised}) must be composited after the scrim ({scrim})"
        );
    }
}
