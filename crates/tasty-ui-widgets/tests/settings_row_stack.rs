//! 좁은 설정 행 쌓기: 컨트롤이 라벨 옆에 들어가지 않는 행만 쌓고, 쌓인 행은 hysteresis 만큼 여유가
//! 생겨야 되돌린다. 판단은 지난 프레임의 폭으로 하므로 같은 Context 로 여러 프레임을 그린다.

use egui::{Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::SettingsRow;

const ORIGIN: Pos2 = pos2(10.0, 10.0);
/// 한 덩어리 컨트롤의 폭 — 라벨 열 150 + gap 16 옆에 들어가려면 행이 366 이상이어야 한다.
const WIDE: f32 = 200.0;
const NARROW: f32 = 40.0;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 행 하나의 (라벨 줄 rect, 컨트롤 rect).
#[derive(Clone, Copy, Debug)]
struct Drawn {
    row: Rect,
    control: Rect,
}

/// 폭 `width` 에서 컨트롤 폭이 `controls` 인 행들을 한 프레임 그린다.
fn frame(ctx: &egui::Context, th: &Theme, width: f32, controls: &[f32]) -> Vec<Drawn> {
    let mut out = Vec::new();
    // FullOutput 불필요 — 이 시험이 보는 것은 위젯이 보고한 rect 뿐이다.
    drop(ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
            ..Default::default()
        },
        |c| {
            egui::Area::new(egui::Id::new("host"))
                .fixed_pos(ORIGIN)
                .show(c, |ui| {
                    ui.set_width(width);
                    ui.set_max_width(width);
                    let labels = ["Maximum size:", "Next workspace:", "Theme:"];
                    for (i, w) in controls.iter().enumerate() {
                        let mut control = Rect::NOTHING;
                        let row = SettingsRow::new(labels[i % labels.len()]).show(
                            ui,
                            th,
                            th.settings_label_width(),
                            |ui| {
                                control = ui
                                    .allocate_exact_size(vec2(*w, 28.0), egui::Sense::hover())
                                    .0;
                            },
                        );
                        out.push(Drawn {
                            row: row.rect,
                            control,
                        });
                    }
                });
        },
    ));
    out
}

/// 판단이 자리 잡을 때까지 몇 프레임 그린다.
fn settle(ctx: &egui::Context, th: &Theme, width: f32, controls: &[f32]) -> Vec<Drawn> {
    for _ in 0..3 {
        frame(ctx, th, width, controls);
    }
    frame(ctx, th, width, controls)
}

fn side_x(th: &Theme) -> f32 {
    ORIGIN.x + th.settings_label_width().value() + th.settings_label_gap().value()
}

/// 행 폭에서 컨트롤에 남는 폭이 `room` 이 되는 행 폭.
fn width_for_room(th: &Theme, room: f32) -> f32 {
    th.settings_label_width().value() + th.settings_label_gap().value() + room
}

#[test]
fn a_control_that_fits_stays_beside_the_label() {
    let th = theme();
    let ctx = egui::Context::default();
    let rows = settle(&ctx, &th, 900.0, &[WIDE]);
    assert!(
        (rows[0].control.min.x - side_x(&th)).abs() < 0.5,
        "{:?}",
        rows[0]
    );
}

#[test]
fn only_the_row_whose_control_does_not_fit_stacks_under_its_label() {
    let th = theme();
    let ctx = egui::Context::default();
    let width = width_for_room(&th, WIDE - 20.0);
    let rows = settle(&ctx, &th, width, &[NARROW, WIDE]);
    // 좁은 컨트롤 행은 나란히 남는다.
    assert!(
        (rows[0].control.min.x - side_x(&th)).abs() < 0.5,
        "{:?}",
        rows[0]
    );
    // 넓은 컨트롤 행은 행 시작 x 로 내려가고 라벨 줄 아래 stack gap 에 선다.
    let stacked = rows[1];
    assert!(
        (stacked.control.min.x - ORIGIN.x).abs() < 0.5,
        "{stacked:?}"
    );
    // 라벨 줄은 행 최소 높이, 컨트롤 줄은 그 아래 stack gap 에서 시작해 최소 높이 안 가운데에 선다.
    let row_h = th.settings_row_min_height().value();
    let label_bottom = stacked.row.min.y + row_h;
    let line_top = stacked.control.center().y - row_h / 2.0;
    assert!(
        (line_top - label_bottom - th.settings_row_stack_gap().value()).abs() < 0.5,
        "line top {line_top}, label bottom {label_bottom}"
    );
}

#[test]
fn a_stacked_row_goes_back_only_with_the_hysteresis_to_spare() {
    let th = theme();
    let hysteresis = th.settings_row_stack_hysteresis().value();
    let ctx = egui::Context::default();
    // 좁을 때 쌓인다.
    let rows = settle(&ctx, &th, width_for_room(&th, WIDE - 1.0), &[WIDE]);
    assert!((rows[0].control.min.x - ORIGIN.x).abs() < 0.5);
    // 들어가기는 하지만 여유가 hysteresis 보다 작으면 그대로 쌓여 있다.
    let rows = settle(
        &ctx,
        &th,
        width_for_room(&th, WIDE + hysteresis - 1.0),
        &[WIDE],
    );
    assert!(
        (rows[0].control.min.x - ORIGIN.x).abs() < 0.5,
        "여유가 모자란데 되돌렸다"
    );
    // hysteresis 만큼 여유가 생기면 나란히 돌아간다.
    let rows = settle(&ctx, &th, width_for_room(&th, WIDE + hysteresis), &[WIDE]);
    assert!(
        (rows[0].control.min.x - side_x(&th)).abs() < 0.5,
        "여유가 있는데 쌓여 있다"
    );
    // 나란한 행은 딱 맞는 폭까지 나란히 남는다.
    let rows = settle(&ctx, &th, width_for_room(&th, WIDE), &[WIDE]);
    assert!((rows[0].control.min.x - side_x(&th)).abs() < 0.5);
}
