//! 화면 조각을 egui로 그려 그려진 선·테두리·글자를 모으는 시험 도우미. raised 표면 외곽·구분선 시험이
//! Mocha·Latte에서 같은 방식으로 그린 결과를 읽게 한 곳에 둔다.

use crate::theme::Theme;

/// Latte 테마. Mocha 기본 색에 Latte 테마 파일을 덮는다.
pub(crate) fn latte() -> Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, is_light.unwrap_or(true))
}

/// Mocha와 Latte.
pub(crate) fn themes() -> [Theme; 2] {
    [tasty_themes::mocha_fallback(), latte()]
}

fn flatten(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| flatten(s, out)),
        other => out.push(other.clone()),
    }
}

/// 한 프레임에 그려진 도형.
pub(crate) struct Painted {
    pub(crate) shapes: Vec<egui::Shape>,
}

/// 새 Context에서 `draw`를 두 번 그려(글꼴이 자리 잡게) 둘째 프레임의 도형을 모은다.
pub(crate) fn paint(mut draw: impl FnMut(&mut egui::Ui)) -> Painted {
    paint_ctx(&egui::Context::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| draw(ui));
    })
}

/// 주어진 Context에서 `run`을 두 번 돌려 둘째 프레임의 도형을 모은다.
pub(crate) fn paint_ctx(ctx: &egui::Context, mut run: impl FnMut(&egui::Context)) -> Painted {
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 1600.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(raw, |ctx| run(ctx));
        shapes.clear();
        for clipped in out.shapes {
            flatten(&clipped.shape, &mut shapes);
        }
    }
    Painted { shapes }
}

impl Painted {
    /// 그려진 곧은 선의 두 끝점과 획 색.
    pub(crate) fn segments(&self) -> Vec<(egui::Pos2, egui::Pos2, egui::Color32)> {
        self.shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::LineSegment { points, stroke } => {
                    Some((points[0], points[1], stroke.color))
                }
                _ => None,
            })
            .collect()
    }
}
