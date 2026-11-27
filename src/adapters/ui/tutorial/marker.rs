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
    // 두 겹은 정본 box-shadow 두 항의 합성(링 바로 밖이 더 밝다)을 근사하므로 바깥 겹이 glow-inner,
    // 안쪽 겹이 glow-outer 다.
    for (grow, color) in [
        (5.0_f32, theme.tutorial_marker_glow_inner()),
        (2.5, theme.tutorial_marker_glow_outer()),
    ] {
        p.rect_stroke(
            rect.expand(grow),
            radius + grow,
            egui::Stroke::new(theme.focus_ring_width.value() + grow, color.to_egui()),
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

    /// 반투명 accent 의 알파. 정본 식은 accent 알파에 비율을 곱하므로 고정 알파와 갈린다.
    const TRANSLUCENT_ACCENT_ALPHA: u8 = 128;

    /// Mocha 와 accent 만 반투명인 Mocha.
    fn marker_themes() -> [(&'static str, Theme); 2] {
        let mut colors = tasty_themes::mocha_fallback_colors();
        colors.blue = colors.blue.with_alpha(TRANSLUCENT_ACCENT_ALPHA);
        let translucent = Theme::with_colors_and_zoom(colors, false, 1.0);
        assert_eq!(translucent.accent_primary().a(), TRANSLUCENT_ACCENT_ALPHA);
        [
            ("mocha", tasty_themes::mocha_fallback()),
            ("translucent accent", translucent),
        ]
    }

    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
        match shape {
            egui::Shape::Rect(r) => out.push(r.clone()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            _ => {}
        }
    }

    /// 실제 경로(paint_spotlight)로 그린 마커의 발광 두 겹이 정본 토큰 색인지 그린 도형으로 본다. 링에서 더 멀리
    /// 벌린 겹이 glow-inner, 가까운 겹이 glow-outer 다(정본 box-shadow 합성의 근사, `paint_marker`).
    #[test]
    fn marker_glow_layers_paint_the_design_glow_tokens() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let hole = egui::Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(240.0, 120.0));
        for (name, theme) in marker_themes() {
            let ctx = egui::Context::default();
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| paint_spotlight(ctx, screen, hole, &theme),
            );
            let mut rects = Vec::new();
            out.shapes
                .iter()
                .for_each(|c| flatten(&c.shape, &mut rects));
            let ring = theme.accent_primary().to_egui();
            assert_eq!(
                rects
                    .iter()
                    .filter(|r| r.rect == hole && r.stroke.color == ring)
                    .count(),
                1,
                "{name}: 마커 링"
            );
            // 대상과 중심이 같고 더 큰 바깥 stroke 를 발광 겹으로 본다. 벌린 폭은 0.1px 단위로 읽는다.
            let mut glows: Vec<(f32, egui::Color32)> = rects
                .iter()
                .filter(|r| r.stroke.width > 0.0 && r.stroke_kind == egui::StrokeKind::Outside)
                .filter(|r| (r.rect.center() - hole.center()).length() < 0.01)
                .map(|r| {
                    let grow = ((r.rect.width() - hole.width()) / 2.0 * 10.0).round() / 10.0;
                    (grow, r.stroke.color)
                })
                .filter(|(grow, _)| *grow > 0.0)
                .collect();
            glows.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(
                glows,
                vec![
                    (2.5, theme.tutorial_marker_glow_outer().to_egui()),
                    (5.0, theme.tutorial_marker_glow_inner().to_egui()),
                ],
                "{name}: 발광 겹(벌린 폭, 색)"
            );
        }
    }
}
