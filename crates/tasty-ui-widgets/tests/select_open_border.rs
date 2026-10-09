//! 목록이 열린 Select 트리거는 MultiSelect 처럼 select-border-focus 테두리를 쓴다.
//! 닫힌 트리거는 select-border 다. 실제 egui 프레임에서 트리거를 클릭해 연다.

// 이유: 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
#![allow(clippy::let_underscore_must_use)]
use egui::{Event, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::select;

/// CentralPanel 안쪽 여백만큼 들어간 트리거 위치. 클릭 좌표가 트리거 안에 들어가도록 넉넉한 안쪽 점을 쓴다.
const INSIDE: Pos2 = pos2(60.0, 20.0);
const WIDTH: f32 = 160.0;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 프레임마다 입력 이벤트를 넣고 마지막 프레임에서 트리거 상자의 테두리 색을 읽는다.
/// CentralPanel 에 둔다(고정 Area 는 클릭을 위젯까지 넘기지 않았다).
fn trigger_border(theme: &Theme, frames: &[Vec<Event>]) -> egui::Color32 {
    let ctx = egui::Context::default();
    let mut out = None;
    let mut origin = Pos2::ZERO;
    for events in frames {
        out = Some(ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                focused: true,
                events: events.clone(),
                ..Default::default()
            },
            |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    origin = ui.cursor().min;
                    let mut sel = 0;
                    select(ui, theme, "t", &mut sel, &["Ctrl", "Alt"], WIDTH, true);
                });
            },
        ));
    }
    let out = out.expect("한 프레임 이상 돌렸다");
    let trigger = Rect::from_min_size(origin, vec2(WIDTH, theme.select_height().value()));
    let strokes: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect == trigger => Some(r.stroke.color),
            _ => None,
        })
        .collect();
    assert_eq!(
        strokes.len(),
        1,
        "트리거 상자를 하나만 찾아야 한다: {strokes:?}"
    );
    strokes[0]
}

fn click_frames(at: Pos2) -> Vec<Vec<Event>> {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    vec![
        vec![],
        vec![Event::PointerMoved(at), press(true), press(false)],
        // 클릭으로 열린 상태를 다음 프레임의 트리거가 읽는다. 포인터는 트리거 밖으로 뺀다.
        vec![Event::PointerMoved(pos2(500.0, 380.0))],
    ]
}

#[test]
fn an_open_select_paints_the_focus_border() {
    let th = theme();
    assert_ne!(
        th.select_border_focus(),
        th.select_border(),
        "두 색이 같으면 이 테스트는 아무것도 가르지 못한다"
    );
    let open = trigger_border(&th, &click_frames(INSIDE));
    assert_eq!(
        open,
        th.select_border_focus().to_egui(),
        "목록이 열린 트리거가 select-border-focus 가 아니다"
    );
}

#[test]
fn a_closed_select_paints_the_resting_border() {
    let th = theme();
    let closed = trigger_border(
        &th,
        &[vec![], vec![Event::PointerMoved(pos2(500.0, 380.0))]],
    );
    assert_eq!(
        closed,
        th.select_border().to_egui(),
        "닫힌 트리거가 select-border 가 아니다"
    );
}
