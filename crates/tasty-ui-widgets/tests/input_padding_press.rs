//! Input 상자 가로 여백 누름.
//! - 맨 Input(기본): 글자 칸은 padding 안쪽만 덮는다. 글자 칸 바로 바깥(egui 근접 판정 안) 누름은
//!   캐럿을 옮기고 드래그 선택을 시작하며, 더 바깥 여백 누름은 칸 밖 누름이라 포커스를 푼다.
//! - `text_covers_padding`: 글자 칸이 상자 전체를 덮어 여백 누름이 글자 칸 안 누름이다.
//!   그린 모양은 맨 Input과 같다.

// 이유: 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
#![allow(clippy::let_underscore_must_use)]
use egui::text::CCursor;
use egui::text_edit::TextEditState;
use egui::text_selection::CCursorRange;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_ui_widgets::Input;

/// 한 줄에 놓은 Input 두 개: 맨 칸, 여백까지 덮는 칸.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Plain,
    Covers,
}

const FIELDS: [Field; 2] = [Field::Plain, Field::Covers];
const VALUE: &str = "value";

#[derive(Default)]
struct Frame {
    /// 칸마다 (상자 rect, 글자 칸 id, 이 패스에 포커스를 잃었는가).
    boxes: Vec<(Field, Rect, egui::Id, bool)>,
    shapes: Vec<egui::Shape>,
}

struct Harness {
    ctx: egui::Context,
    bufs: [String; 2],
}

fn field_id(field: Field) -> egui::Id {
    egui::Id::new(("padding_press", field as u8))
}

impl Harness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            bufs: std::array::from_fn(|_| VALUE.to_owned()),
        }
    }

    fn frame(&mut self, focus: Option<Field>, events: Vec<Event>) -> Frame {
        let theme = tasty_themes::mocha_fallback();
        let mut frame = Frame::default();
        let out = self.ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 300.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for (field, buf) in FIELDS.iter().zip(self.bufs.iter_mut()) {
                            let resp = Input::new()
                                .id(field_id(*field))
                                .text_covers_padding(*field == Field::Covers)
                                .width(200.0)
                                .show(ui, &theme, buf);
                            if focus == Some(*field) {
                                resp.request_focus();
                            }
                            frame
                                .boxes
                                .push((*field, resp.rect, resp.id, resp.lost_focus()));
                        }
                    });
                });
            },
        );
        frame.shapes = out.shapes.into_iter().map(|c| c.shape).collect();
        frame
    }

    fn focused(&self) -> Option<egui::Id> {
        self.ctx.memory(|m| m.focused())
    }

    /// (primary, secondary) 글자 위치.
    fn range(&self, field: Field) -> Option<(usize, usize)> {
        TextEditState::load(&self.ctx, field_id(field))
            .and_then(|s| s.cursor.char_range())
            .map(|r| (r.primary.index, r.secondary.index))
    }

    /// 칸에 포커스를 주고 커서를 3에 둔다(Claude 탐침과 같은 시작).
    fn focus_at_three(&mut self, field: Field) -> Rect {
        self.frame(Some(field), vec![]);
        let f = self.frame(None, vec![]);
        let (_, rect, id, _) = *f.boxes.iter().find(|b| b.0 == field).unwrap();
        assert_eq!(self.focused(), Some(id), "{field:?} 포커스 준비");
        let mut st = TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(3))));
        st.store(&self.ctx, id);
        self.frame(None, vec![]);
        assert_eq!(self.range(field), Some((3, 3)), "{field:?} 커서 준비");
        rect
    }
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn pad() -> f32 {
    tasty_themes::mocha_fallback().input_padding_x().value()
}

/// 글자 칸 왼쪽 끝(상자 왼쪽 + padding)에서 dx 바깥.
fn left_of_text(rect: Rect, dx: f32) -> Pos2 {
    pos2(rect.min.x + pad() - dx, rect.center().y)
}

/// 누름 → (끌기) → 뗌. 패스마다 이 칸이 포커스를 잃었는지 모은다.
fn press_drag_release(h: &mut Harness, field: Field, at: Pos2, to: Option<Pos2>) -> bool {
    let mut lost = false;
    let mut pass = |h: &mut Harness, events| {
        let f = h.frame(None, events);
        lost |= f.boxes.iter().find(|b| b.0 == field).unwrap().3;
    };
    pass(h, vec![Event::PointerMoved(at)]);
    pass(h, vec![button(at, true)]);
    let end = to.unwrap_or(at);
    if let Some(to) = to {
        pass(h, vec![Event::PointerMoved(to)]);
    }
    pass(h, vec![button(end, false)]);
    pass(h, vec![]);
    lost
}

/// 맨 Input은 c56c436601 이전과 같다(Claude 탐침 값): 글자 칸에서 2·4 바깥 누름은 캐럿을 처음으로,
/// 그 자리에서 끝까지 끈 드래그는 (5, 0) 선택. 8 바깥은 egui 근접 판정 밖이라 포커스를 푼다.
#[test]
fn a_plain_input_keeps_the_caret_and_drag_just_outside_its_text() {
    for dx in [2.0, 4.0] {
        let mut h = Harness::new();
        let rect = h.focus_at_three(Field::Plain);
        let lost = press_drag_release(&mut h, Field::Plain, left_of_text(rect, dx), None);
        assert!(!lost, "dx={dx} 누름에 포커스를 잃었다");
        assert_eq!(h.range(Field::Plain), Some((0, 0)), "dx={dx} 누름 캐럿");

        let mut h = Harness::new();
        let rect = h.focus_at_three(Field::Plain);
        let end = pos2(rect.max.x - pad(), rect.center().y);
        press_drag_release(&mut h, Field::Plain, left_of_text(rect, dx), Some(end));
        assert_eq!(h.range(Field::Plain), Some((5, 0)), "dx={dx} 드래그 선택");
    }
    let mut h = Harness::new();
    let rect = h.focus_at_three(Field::Plain);
    let lost = press_drag_release(&mut h, Field::Plain, left_of_text(rect, 8.0), None);
    assert!(lost, "dx=8 여백 누름은 칸 밖 누름이다(전과 같다)");
    assert_eq!(h.focused(), None);
}

/// 여백까지 덮는 칸: 왼쪽 여백(2·4·8) 누름은 포커스를 지키고 캐럿을 처음으로, 오른쪽 여백 누름은
/// 끝으로 보낸다. 누름과 뗌이 한 패스여도 같다.
#[test]
fn a_covering_input_takes_padding_presses_as_text_presses() {
    for dx in [2.0, 4.0, 8.0] {
        let mut h = Harness::new();
        let rect = h.focus_at_three(Field::Covers);
        let lost = press_drag_release(&mut h, Field::Covers, left_of_text(rect, dx), None);
        assert!(!lost, "dx={dx} 누름에 포커스를 잃었다");
        assert_eq!(h.range(Field::Covers), Some((0, 0)), "dx={dx} 누름 캐럿");

        let mut h = Harness::new();
        let rect = h.focus_at_three(Field::Covers);
        let right = pos2(rect.max.x - pad() + dx, rect.center().y);
        let lost = press_drag_release(&mut h, Field::Covers, right, None);
        assert!(!lost, "dx={dx} 오른쪽 여백 누름에 포커스를 잃었다");
        assert_eq!(
            h.range(Field::Covers),
            Some((5, 5)),
            "dx={dx} 오른쪽 누름 캐럿"
        );

        let mut h = Harness::new();
        let rect = h.focus_at_three(Field::Covers);
        let at = left_of_text(rect, dx);
        h.frame(None, vec![Event::PointerMoved(at)]);
        let f = h.frame(None, vec![button(at, true), button(at, false)]);
        assert!(!f.boxes[1].3, "dx={dx} 한 패스 누름·뗌에 포커스를 잃었다");
        h.frame(None, vec![]);
        assert_eq!(h.focused(), Some(field_id(Field::Covers)));
    }
}

/// 여백까지 덮는 칸도 쉴 때·포커스일 때 그린 모양(글자 galley·커서·테두리·링)이 맨 칸과 같다.
/// 두 칸을 따로 그려 도형 목록을 상자 위치만큼 옮겨 비교한다.
#[test]
fn a_covering_input_draws_the_same_shapes_as_a_plain_one() {
    for focus in [false, true] {
        let shapes = |field: Field| {
            let mut h = Harness::new();
            h.bufs = std::array::from_fn(|_| VALUE.to_owned());
            if focus {
                h.focus_at_three(field);
            } else {
                h.frame(None, vec![]);
            }
            let f = h.frame(None, vec![]);
            let rect = f.boxes.iter().find(|b| b.0 == field).unwrap().1;
            let inside: Vec<egui::Shape> = f
                .shapes
                .into_iter()
                .filter(|s| rect.expand(2.0).contains_rect(s.visual_bounding_rect()))
                .map(|mut s| {
                    s.translate(-rect.min.to_vec2());
                    s
                })
                .collect();
            assert!(inside.len() >= 3, "{field:?} 도형 {}개", inside.len());
            inside
        };
        assert_eq!(
            shapes(Field::Covers),
            shapes(Field::Plain),
            "focus={focus} 그린 모양"
        );
    }
}

/// 편집 중이 아닌 칸의 여백을 눌러도 포커스가 생기지 않는다(전과 같다).
#[test]
fn pressing_the_padding_of_an_idle_input_does_not_focus_it() {
    let mut h = Harness::new();
    h.frame(None, vec![]);
    let f = h.frame(None, vec![]);
    let (_, rect, _, _) = f.boxes[0];
    let at = pos2(rect.min.x + pad() / 2.0, rect.center().y);
    h.frame(None, vec![Event::PointerMoved(at)]);
    h.frame(None, vec![button(at, true)]);
    h.frame(None, vec![button(at, false)]);
    h.frame(None, vec![]);
    assert_eq!(h.focused(), None);
}

/// Tab은 다음 칸의 글자 칸으로 간다.
#[test]
fn tab_from_an_editing_input_goes_to_the_next_text_field() {
    let mut h = Harness::new();
    h.frame(Some(Field::Plain), vec![]);
    let f = h.frame(None, vec![]);
    let next = f.boxes[1].2;
    let tab = |pressed| Event::Key {
        key: Key::Tab,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    h.frame(None, vec![tab(true), tab(false)]);
    h.frame(None, vec![]);
    assert_eq!(h.focused(), Some(next));
}
