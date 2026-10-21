//! surface-raised 위 컨트롤(`Ground::Raised`)이 raised 토큰으로 그려지는지 본다.
//! Latte에서는 border-default가 surface-raised와 같아 panel 상자가 사라지므로 두 바탕의 칠이 달라야 한다.

use egui::{Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Ground, Input, SelectLook};

fn latte() -> Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT)
        .expect("builtin latte.toml ships valid");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, is_light.unwrap_or(true))
}

/// 한 프레임을 그려 사각형의 채움·테두리 색과 글자 색을 모은다.
struct Paint {
    fills: Vec<egui::Color32>,
    strokes: Vec<egui::Color32>,
    texts: Vec<egui::Color32>,
}

fn paint(draw: impl Fn(&mut egui::Ui)) -> Paint {
    let ctx = egui::Context::default();
    let out = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
            ..Default::default()
        },
        |c| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(c, |ui| draw(ui));
        },
    );
    let mut p = Paint {
        fills: Vec::new(),
        strokes: Vec::new(),
        texts: Vec::new(),
    };
    let mut stack: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = stack.pop() {
        match s {
            egui::Shape::Vec(v) => stack.extend(v),
            egui::Shape::Rect(r) => {
                p.fills.push(r.fill);
                p.strokes.push(r.stroke.color);
            }
            egui::Shape::Text(t) => {
                p.texts.push(t.fallback_color);
                for s in &t.galley.job.sections {
                    p.texts.push(s.format.color);
                }
            }
            _ => {}
        }
    }
    p
}

#[test]
fn a_raised_select_uses_the_raised_box_in_every_state() {
    for th in [tasty_themes::mocha_fallback(), latte()] {
        let pinned = |ground, enabled| {
            paint(|ui| {
                tasty_ui_widgets::select_pinned(
                    ui,
                    &th,
                    ground,
                    Some("markdown"),
                    "Select…",
                    LogicalPx(200.0),
                    SelectLook::Normal,
                    enabled,
                );
            })
        };
        let raised = pinned(Ground::Raised, true);
        assert!(raised.fills.contains(&th.select_raised_bg().to_egui()));
        assert!(
            raised
                .strokes
                .contains(&th.select_raised_border().to_egui())
        );
        let disabled = pinned(Ground::Raised, false);
        assert!(
            disabled
                .fills
                .contains(&th.select_raised_disabled_bg().to_egui())
        );
        assert!(
            disabled
                .strokes
                .contains(&th.select_raised_disabled_border().to_egui())
        );
        let panel = pinned(Ground::Panel, true);
        assert!(panel.strokes.contains(&th.select_border().to_egui()));
    }
    let th = latte();
    assert_ne!(
        th.select_raised_border().to_egui(),
        th.select_border().to_egui(),
        "Latte에서 raised 테두리가 panel 테두리와 같으면 이 시험은 아무것도 구분하지 못한다"
    );
}

#[test]
fn a_raised_input_steps_down_to_the_raised_box() {
    for th in [tasty_themes::mocha_fallback(), latte()] {
        let draw = |ground, enabled| {
            paint(|ui| {
                let mut buf = "dockerfile".to_string();
                Input::new()
                    .ground(ground)
                    .enabled(enabled)
                    .width(200.0)
                    .show(ui, &th, &mut buf);
            })
        };
        let raised = draw(Ground::Raised, true);
        assert!(raised.fills.contains(&th.input_raised_bg().to_egui()));
        assert!(raised.strokes.contains(&th.input_raised_border().to_egui()));
        let disabled = draw(Ground::Raised, false);
        assert!(
            disabled
                .fills
                .contains(&th.state_disabled_fill_raised().to_egui())
        );
        assert!(
            disabled
                .strokes
                .contains(&th.state_disabled_border_raised().to_egui())
        );
        let panel = draw(Ground::Panel, true);
        assert!(panel.fills.contains(&th.input_bg().to_egui()));
    }
}

#[test]
fn a_raised_ghost_button_takes_the_raised_label_ink() {
    let th = latte();
    let ink = |ground| {
        paint(|ui| {
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .ground(ground)
                .show(ui, &th);
        })
        .texts
    };
    let raised = th.button_ghost_raised_fg().to_egui();
    let panel = th.button_ghost_fg().to_egui();
    assert_ne!(
        raised, panel,
        "Latte에서 두 글자색이 달라야 시험이 의미가 있다"
    );
    assert!(ink(Ground::Raised).contains(&raised));
    assert!(ink(Ground::Panel).contains(&panel));
}
