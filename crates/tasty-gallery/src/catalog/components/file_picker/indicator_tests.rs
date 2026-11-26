//! 원격 표시 C안 띠와 B안 호스트 글자가 디자인 토큰 치수로 그려지는지 본다.
use super::indicator::{Indicator, top_strip};
use super::{FpState, Variant};
use tasty_type_appearance::theme::Theme;

fn zoomed(zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, zoom)
}

/// `draw` 를 두 프레임 그리고 마지막 프레임의 모양을 돌려준다.
fn shapes_of(
    theme: &Theme,
    draw: impl Fn(&mut egui::Ui, &Theme),
) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = ctx
            .run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| draw(ui, theme));
            })
            .shapes;
    }
    shapes
}

/// C안 프레임 위 띠는 `fp-remote-strip-height`(selection-edge-width 2) 높이로 칠하고 그 높이를 돌려준다.
#[test]
fn the_border_indicator_strip_is_the_remote_strip_height() {
    let v = Variant::open(FpState::Loaded, true, false).indicated(Indicator::Border);
    for (zoom, want) in [(1.0, 2.0), (1.5, 3.0)] {
        let theme = zoomed(zoom);
        let info: egui::Color32 = theme.accent_info().to_egui();
        let returned = std::cell::Cell::new(0.0);
        let shapes = shapes_of(&theme, |ui, th| returned.set(top_strip(ui, th, v).value()));
        let strips: Vec<f32> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Rect(r) if r.fill == info => Some(r.rect.height()),
                _ => None,
            })
            .collect();
        assert_eq!(strips, vec![want], "배율 {zoom}: 띠 높이");
        assert_eq!(returned.get(), want, "배율 {zoom}: 돌려준 높이");
    }
}
