//! Select 테두리로 감싼 egui ComboBox 는 닫힘 select-border, 열림 select-border-focus 로 트리거를 그리고
//! 펼친 목록 안의 위젯 테두리는 바꾸지 않는다. 실제 egui 프레임에서 트리거를 클릭해 연다.

use egui::{Event, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::with_select_combo_frame;

const INSIDE: Pos2 = pos2(40.0, 18.0);
const AWAY: Pos2 = pos2(560.0, 380.0);

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 마지막 프레임의 (트리거 상자의 테두리 색들, 목록 안 입력칸 상자의 테두리 색들).
/// 목록 틀 자신의 테두리(menu-border)는 select-border 와 값이 같을 수 있어 보지 않는다.
fn strokes(theme: &Theme, frames: &[Vec<Event>]) -> (Vec<egui::Color32>, Vec<egui::Color32>) {
    let ctx = egui::Context::default();
    let mut out = None;
    let mut trigger = Rect::NOTHING;
    let mut field = Rect::NOTHING;
    // 펼친 목록은 나타나는 동안 흐리게 그려진다. 프레임마다 시간을 1초씩 넘겨 다 나타난 색을 본다.
    for (i, events) in frames.iter().enumerate() {
        out = Some(ctx.run(
            RawInput {
                time: Some(i as f64),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                focused: true,
                events: events.clone(),
                ..Default::default()
            },
            |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    trigger = with_select_combo_frame(ui, theme, |ui| {
                        egui::ComboBox::from_id_salt("combo")
                            .selected_text("Mono")
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                let mut text = String::new();
                                field = ui.add(egui::TextEdit::singleline(&mut text)).rect;
                            })
                            .response
                            .rect
                    });
                });
            },
        ));
    }
    let out = out.expect("한 프레임 이상 돌렸다");
    let (mut at_trigger, mut below) = (Vec::new(), Vec::new());
    fn visit(
        shape: &egui::Shape,
        trigger: Rect,
        field: Rect,
        at_trigger: &mut Vec<egui::Color32>,
        below: &mut Vec<egui::Color32>,
    ) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    visit(s, trigger, field, at_trigger, below);
                }
            }
            egui::Shape::Rect(r) if r.rect == trigger => at_trigger.push(r.stroke.color),
            // 목록 안 입력칸. egui 기본 스타일의 쉬는 입력칸은 테두리 굵기 0 이므로 색만 본다.
            // 입력칸 상자는 응답 영역을 TextEdit 기본 여백(가로 4, 세로 2)만큼 넓힌 자리다.
            // 목록 틀도 같은 중심이지만 더 크다.
            egui::Shape::Rect(r) if r.rect == field.expand2(vec2(4.0, 2.0)) => {
                below.push(r.stroke.color)
            }
            _ => {}
        }
    }
    for clipped in &out.shapes {
        visit(&clipped.shape, trigger, field, &mut at_trigger, &mut below);
    }
    (at_trigger, below)
}

fn click() -> Vec<Vec<Event>> {
    let press = |pressed| Event::PointerButton {
        pos: INSIDE,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    vec![
        vec![],
        vec![Event::PointerMoved(INSIDE), press(true), press(false)],
        vec![Event::PointerMoved(AWAY)],
        vec![],
    ]
}

#[test]
fn a_closed_combo_paints_the_select_border() {
    let th = theme();
    let (trigger, _) = strokes(&th, &[vec![], vec![Event::PointerMoved(AWAY)]]);
    assert_eq!(trigger, vec![th.select_border().to_egui()]);
}

#[test]
fn an_open_combo_paints_the_focus_border_and_leaves_the_list_alone() {
    let th = theme();
    assert_ne!(th.select_border_focus(), th.select_border());
    let (trigger, below) = strokes(&th, &click());
    assert_eq!(trigger, vec![th.select_border_focus().to_egui()]);
    assert!(!below.is_empty(), "펼친 목록의 입력칸 테두리를 찾지 못했다");
    for c in [
        th.select_border(),
        th.select_border_focus(),
        th.border_strong(),
    ] {
        assert!(
            !below.contains(&c.to_egui()),
            "목록 안 위젯이 트리거 테두리 {c:?} 를 물려받았다: {below:?}"
        );
    }
}
