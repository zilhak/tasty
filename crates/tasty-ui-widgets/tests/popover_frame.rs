//! 전역 스타일은 tooltip 틀 토큰을, `with_popover_frame` 안의 부모 스타일은 메뉴 틀 토큰을 쓰는지 확인한다.
//! 래퍼가 끝나면 부모 스타일이 원래 값으로 돌아와야 뒤에 그리는 위젯이 영향을 받지 않는다.
//!
//! 디자인은 메뉴 테두리를 border-strong 으로 정해 두 틀이 채움·테두리·반경 모두 같은 값이 됐다.
//! 그래서 래퍼가 값을 넣고 되돌리는지는 두 틀 어느 것과도 다른 부모 스타일을 기준으로 본다.

use egui::{RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

fn frame_of(v: &egui::Visuals) -> (egui::Color32, egui::Stroke, egui::CornerRadius) {
    (v.window_fill, v.window_stroke, v.menu_corner_radius)
}

#[test]
fn tooltips_use_tooltip_tokens_and_popovers_use_menu_tokens_only_inside_the_wrapper() {
    let th = theme();
    let stroke = |c| egui::Stroke::new(th.border_width.value(), c);
    let tooltip = (
        th.tooltip_bg().into(),
        stroke(th.tooltip_border()),
        th.tooltip_radius().value().into(),
    );
    let menu = (
        th.menu_bg().into(),
        stroke(th.menu_border()),
        th.menu_radius().value().into(),
    );
    // 래퍼 앞의 부모 스타일. 메뉴 틀과 세 값이 모두 달라야 넣기·되돌리기를 구별한다.
    let parent = (
        egui::Color32::from_rgb(1, 2, 3),
        egui::Stroke::new(
            th.border_width.value() + 2.0,
            egui::Color32::from_rgb(4, 5, 6),
        ),
        egui::CornerRadius::same(th.menu_radius().value() as u8 + 3),
    );
    assert!(parent.0 != menu.0 && parent.1 != menu.1 && parent.2 != menu.2);

    let ctx = egui::Context::default();
    tasty_egui_theme::apply_theme_to_egui(&th, &ctx);
    assert_eq!(frame_of(&ctx.style().visuals), tooltip);

    let mut seen = None;
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
        ..Default::default()
    };
    let _output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let v = &mut ui.style_mut().visuals;
            (v.window_fill, v.window_stroke, v.menu_corner_radius) = parent;
            let inside = tasty_egui_theme::with_popover_frame(ui, &th, |ui| {
                (frame_of(&ui.style().visuals), ui.id())
            });
            seen = Some((inside, frame_of(&ui.style().visuals), ui.id()));
        });
    });
    let ((inside, inside_id), after, outer_id) = seen.expect("패널이 그려지지 않았다");
    assert_eq!(inside, menu, "래퍼 안의 팝오버 틀은 메뉴 토큰이어야 한다");
    assert_eq!(after, parent, "래퍼가 끝나면 부모 스타일이 돌아와야 한다");
    assert_eq!(
        inside_id, outer_id,
        "래퍼가 자식 Ui 를 만들면 팝업 ID 가 바뀐다"
    );
    assert_eq!(frame_of(&ctx.style().visuals), tooltip);
}
