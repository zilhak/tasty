//! 창 파일 드롭 안내가 터미널 영역 안쪽 spacing-sm 에 채움 상자를 두고, 글리프와 라벨 한 줄을
//! 그 상자 가운데에 강조색으로 놓는지 검사한다.

use egui::{Pos2, RawInput, Rect, Shape, vec2};
use tasty_ui_widgets::file_drop_overlay;

const LABEL: &str = "Drop to open";

fn shapes(theme: &tasty_type_appearance::theme::Theme, area: Rect) -> Vec<Shape> {
    let ctx = egui::Context::default();
    let mut out = Vec::new();
    // 첫 프레임은 폰트 준비 전이라 두 번 돌린다.
    for _ in 0..2 {
        let full = ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    file_drop_overlay(ui, theme, area, LABEL);
                });
            },
        );
        out = full.shapes.into_iter().map(|c| c.shape).collect();
    }
    out
}

fn flatten(shapes: Vec<Shape>, out: &mut Vec<Shape>) {
    for s in shapes {
        match s {
            Shape::Vec(v) => flatten(v, out),
            other => out.push(other),
        }
    }
}

#[test]
fn the_box_sits_inside_the_area_and_the_content_is_centred_in_accent_ink() {
    let theme = tasty_themes::mocha_fallback();
    let area = Rect::from_min_size(Pos2::new(40.0, 40.0), vec2(420.0, 160.0));
    let inner = area.shrink(theme.spacing_sm.value());
    let mut flat = Vec::new();
    flatten(shapes(&theme, area), &mut flat);

    let fill = flat
        .iter()
        .find_map(|s| match s {
            Shape::Rect(r) if r.fill == theme.file_drop_overlay_bg().to_egui() => Some(r.rect),
            _ => None,
        })
        .expect("overlay fill drawn");
    assert_eq!(fill, inner, "the fill stays inside the inset edge");

    let (text_rect, color, size) = flat
        .iter()
        .find_map(|s| match s {
            Shape::Text(t) if t.galley.text() == LABEL => Some((
                t.galley.rect.translate(t.pos.to_vec2()),
                t.fallback_color,
                t.galley.job.sections[0].format.font_id.size,
            )),
            _ => None,
        })
        .expect("label drawn");
    assert_eq!(
        size,
        theme.font_size_body.value(),
        "label uses the body size"
    );
    assert_eq!(color, theme.file_drop_overlay_fg().to_egui());
    assert!(inner.contains_rect(text_rect), "{text_rect:?} in {inner:?}");
    let content_left =
        text_rect.left() - theme.spacing_sm.value() - theme.icon_glyph_size_md.value();
    let mid = (content_left + text_rect.right()) / 2.0;
    assert!(
        (mid - inner.center().x).abs() < 0.5,
        "glyph + label centred: {mid} vs {}",
        inner.center().x
    );
}
