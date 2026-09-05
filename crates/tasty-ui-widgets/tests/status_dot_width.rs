//! `status_dot` 의 **폭 계약** 회귀 테스트 (headless egui).
//!
//! 라벨 없는 점이 필요한 자리가 이 위젯을 그대로 부를 수 있어야 한다. 라벨이 비었는데도
//! dot 뒤에 gap 을 할당하면 그 자리만 정렬선이 밀리고, 소비자는 폭을 되빼는 래퍼를 쓰게
//! 된다 — 그 되빼는 값은 위젯 안의 상수와 독립으로 적히므로 한쪽만 바뀌면 갈린다.
//!
//! 술어를 **"gap 이 라벨과 함께 있고 라벨 없이는 없다"** 로 세운다. 폭 하나만 재면
//! 라벨 있는 경우가 맞는지 알 수 없고, 차이만 재면 dot 지름이 토큰에서 오는지 알 수 없다.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _ =` 는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]
use egui::{Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{StatusKind, status_dot};

fn theme(zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, zoom)
}

/// 위젯이 자기 자리에 할당한 폭.
fn allocated_width(theme: &Theme, label: &str) -> f32 {
    let ctx = egui::Context::default();
    let mut w = f32::NAN;
    // `FullOutput` 불필요 — 이 테스트가 보는 것은 위젯이 보고한 rect 뿐이다.
    let _ = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0))),
            focused: true,
            ..Default::default()
        },
        |c| {
            egui::Area::new(egui::Id::new("host"))
                .fixed_pos(pos2(10.0, 10.0))
                .show(c, |ui| {
                    w = status_dot(ui, theme, StatusKind::Running, label, false, true)
                        .rect
                        .width();
                });
        },
    );
    w
}

#[test]
fn an_empty_label_allocates_the_dot_alone() {
    for zoom in [1.0_f32, 2.0] {
        let th = theme(zoom);
        let dot = th.status_dot_size().value();
        assert_eq!(
            allocated_width(&th, ""),
            dot,
            "zoom {zoom}: 라벨이 없는데 dot 지름보다 넓다 — 붙을 것 없는 gap 을 할당했다"
        );
    }
}

#[test]
fn a_label_brings_its_gap_with_it() {
    let th = theme(1.0);
    let dot = th.status_dot_size().value();
    let with = allocated_width(&th, "running");
    // 라벨이 붙으면 dot 보다 넓어야 하고, 그 여분은 gap + 글자 폭이다.
    assert!(
        with > dot,
        "라벨을 넘겼는데 폭이 dot 그대로다 — 라벨 자리가 사라졌다"
    );
    // 라벨이 길어지면 폭도 함께 는다(gap 이 라벨 유무에만 걸리고 길이는 글자가 정한다).
    assert!(allocated_width(&th, "running running") > with);
}

#[test]
fn the_dot_diameter_follows_the_token_at_every_zoom() {
    let one = theme(1.0);
    let two = theme(2.0);
    assert!(
        allocated_width(&two, "") > allocated_width(&one, ""),
        "배율을 안 탄다 — 지름이 토큰이 아니라 지역 상수에서 온다"
    );
}
