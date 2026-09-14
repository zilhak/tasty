//! `select_or_placeholder` 의 **트리거 색 계약** 회귀 테스트 (headless egui).
//!
//! 아직 안 고른 드롭다운은 placeholder 를 `text_placeholder` 색으로 그려야 한다 — sentinel 을
//! 첫 옵션으로 끼운 `select` 는 그것을 `select_fg` 로 그려 값처럼 읽힌다. 술어를 "안 골랐으면
//! placeholder 색 · 골랐으면 `select_fg` 색" 양쪽으로 세운다. 한쪽만 재면 색이 상태를 안 따라도
//! 통과한다.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _ =` 는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
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
    // 첫 프레임은 Area 의 크기 재기 프레임이라 도형을 버린다 — 두 번째 프레임을 본다. fade-in 을
    // 끄지 않으면 글자 색이 알파 감쇠된 채 나와 토큰과 비교할 수 없다.
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
