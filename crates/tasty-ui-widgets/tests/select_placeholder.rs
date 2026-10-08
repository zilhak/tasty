//! 미선택 안내는 placeholder 색, 선택한 값은 select_fg 색으로 그리는지 비교한다.
//! 열린 목록은 공용 옵션 행이며 현재 값은 selected 글자와 체크로만 구분하고 egui 기본 선택 채움을 쓰지 않는다.

// 이유: 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
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

/// 목록을 연 채로 그린 프레임의 도형.
/// 목록을 연 채로 그린 프레임의 도형. `selected` 가 None 이면 목록 맨 앞에 sentinel 행이 있다.
fn open_frame(theme: &Theme, selected: Option<usize>) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    // 팝업 페이드를 꺼 실제 토큰 색과 비교한다.
    ctx.style_mut(|s| s.animation_time = 0.0);
    let mut out = Vec::new();
    for pass in 0..3 {
        let frame = ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                focused: true,
                ..Default::default()
            },
            |c| {
                egui::Area::new(egui::Id::new("host"))
                    .fixed_pos(pos2(10.0, 10.0))
                    .fade_in(false)
                    .show(c, |ui| {
                        if pass == 0 {
                            let id = ui.make_persistent_id(("tasty_select", "t"));
                            ui.memory_mut(|m| m.open_popup(id));
                        }
                        let mut sel = selected;
                        select_or_placeholder(
                            ui,
                            theme,
                            "t",
                            &mut sel,
                            &["Ask", "Quit"],
                            "Pick one",
                            160.0,
                            true,
                        );
                    });
            },
        );
        out = frame.shapes;
    }
    out
}

fn label_color(shapes: &[egui::epaint::ClippedShape], text: &str) -> Vec<egui::Color32> {
    shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(t.fallback_color),
            _ => None,
        })
        .collect()
}

#[test]
fn an_open_select_marks_the_current_value_with_ink_not_a_fill() {
    let th = theme();
    let shapes = open_frame(&th, Some(1));
    // 트리거 글자 하나와 목록 행 하나.
    assert_eq!(
        label_color(&shapes, "Quit"),
        vec![
            th.select_fg().to_egui(),
            th.menu_item_selected_fg().to_egui()
        ]
    );
    assert_eq!(
        label_color(&shapes, "Ask"),
        vec![th.menu_item_fg().to_egui()]
    );
    let selection_fill = egui::Color32::from(th.surface_active());
    let accent_blocks = shapes
        .iter()
        .filter(|s| {
            matches!(&s.shape, egui::Shape::Rect(r)
                if r.fill == selection_fill || r.fill == th.accent_primary().to_egui())
        })
        .count();
    assert_eq!(accent_blocks, 0, "no fill on the current value");
}

/// 목록 행 라벨 `text` 의 오른쪽, 같은 줄 높이에 그려진 다른 도형(체크 글리프) 수.
fn marks_right_of(shapes: &[egui::epaint::ClippedShape], text: &str) -> usize {
    let labels: Vec<Rect> = shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(s.shape.visual_bounding_rect()),
            _ => None,
        })
        .collect();
    // 마지막 것이 목록 행이다(트리거 글자가 먼저 그려진다).
    let Some(row) = labels.last().copied() else {
        return 0;
    };
    shapes
        .iter()
        .filter(|s| match &s.shape {
            egui::Shape::Mesh(_) => true,
            egui::Shape::Text(t) => t.galley.text() != text,
            _ => false,
        })
        .map(|s| s.shape.visual_bounding_rect())
        .filter(|b| {
            b.left() > row.right() && b.center().y > row.top() && b.center().y < row.bottom()
        })
        .count()
}

/// 아직 값을 고르지 않은 목록의 sentinel 행은 값이 아니다. 체크와 selected 글자 없이 쉬는 행으로 그린다.
/// 같은 검출기가 값을 고른 목록의 현재 값 행에서는 체크를 찾는지 함께 본다.
#[test]
fn the_placeholder_sentinel_row_has_no_check_and_no_selected_ink() {
    let th = theme();
    let shapes = open_frame(&th, None);
    // 트리거의 placeholder 글자와 목록 맨 앞 sentinel 행.
    assert_eq!(
        label_color(&shapes, "Pick one"),
        vec![th.text_placeholder().to_egui(), th.menu_item_fg().to_egui()]
    );
    assert_eq!(
        marks_right_of(&shapes, "Pick one"),
        0,
        "the sentinel has no check"
    );

    let chosen = open_frame(&th, Some(1));
    assert_eq!(
        marks_right_of(&chosen, "Quit"),
        1,
        "the detector finds the value's check"
    );
}
