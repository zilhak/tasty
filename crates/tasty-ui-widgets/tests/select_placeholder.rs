//! 미선택 안내는 placeholder 색, 선택한 값은 select_fg 색으로 그리는지 비교한다.

// 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
#![allow(clippy::let_underscore_must_use)]
use egui::{Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::select_or_placeholder;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 트리거에 칠해진 글자들의 색.
fn text_colors(theme: &Theme, selected: Option<usize>) -> Vec<egui::Color32> {
    let ctx = egui::Context::default();
    // 측정이 끝난 두 번째 프레임을 읽고 페이드를 꺼 실제 토큰 색과 비교한다.
    let mut out = None;
    for _ in 0..2 {
        out = Some(ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0))),
                focused: true,
                ..Default::default()
            },
            |c| {
                egui::Area::new(egui::Id::new("host"))
                    .fixed_pos(pos2(10.0, 10.0))
                    .fade_in(false)
                    .show(c, |ui| {
                        let mut sel = selected;
                        select_or_placeholder(
                            ui,
                            theme,
                            "t",
                            &mut sel,
                            &["Ctrl", "Alt"],
                            "Select a modifier",
                            160.0,
                            true,
                        );
                    });
            },
        ));
    }
    let out = out.expect("두 프레임을 돌렸다");
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.fallback_color),
            _ => None,
        })
        .collect()
}

#[test]
fn an_unchosen_select_paints_the_placeholder_tone() {
    let th = theme();
    assert_ne!(
        th.text_placeholder(),
        th.select_fg(),
        "두 색이 같으면 이 테스트는 아무것도 가르지 못한다"
    );
    let colors = text_colors(&th, None);
    assert_eq!(
        colors,
        vec![th.text_placeholder().to_egui()],
        "안 고른 트리거가 placeholder 색이 아니다 — placeholder 가 값처럼 읽힌다"
    );
}

#[test]
fn a_chosen_select_paints_the_value_tone() {
    let th = theme();
    let colors = text_colors(&th, Some(1));
    assert_eq!(
        colors,
        vec![th.select_fg().to_egui()],
        "고른 트리거가 값 색이 아니다"
    );
}
