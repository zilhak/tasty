//! 편집 중인 Input의 상자 안쪽(가로 여백·아이콘 칸)을 눌러도 글자 칸 포커스가 풀리지 않는다.
//! 편집 중이 아닐 때의 누름과 Tab 순서는 그대로다.

// 이유: 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
#![allow(clippy::let_underscore_must_use)]
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_ui_widgets::{AutoComplete, Input};

/// 한 줄에 놓은 Input 네 개: 맨 칸, 아이콘 칸, 읽기 전용 칸, AutoComplete 트리거.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Plain,
    Icon,
    ReadOnly,
    Auto,
}

const FIELDS: [Field; 4] = [Field::Plain, Field::Icon, Field::ReadOnly, Field::Auto];

#[derive(Default)]
struct Frame {
    /// 칸마다 (상자 rect, 글자 칸 id, 이 패스에 포커스를 잃었는가).
    boxes: Vec<(Field, Rect, egui::Id, bool)>,
}

struct Harness {
    ctx: egui::Context,
    bufs: [String; 4],
    active: Option<usize>,
}

impl Harness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            bufs: std::array::from_fn(|_| "value".to_owned()),
            active: None,
        }
    }

    fn frame(&mut self, focus: Option<Field>, events: Vec<Event>) -> Frame {
        let theme = tasty_themes::mocha_fallback();
        let mut frame = Frame::default();
        let icon = |ui: &mut egui::Ui, rect: Rect, color: egui::Color32| {
            ui.painter().circle_filled(rect.center(), 2.0, color);
        };
        // 포커스와 응답만 본다. 패스 출력(도형·텍스처)은 쓰지 않는다.
        let _ = self.ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 300.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for (field, buf) in FIELDS.iter().zip(self.bufs.iter_mut()) {
                            let id = egui::Id::new(("padding_press", *field as u8));
                            let resp = match field {
                                Field::Plain => {
                                    Input::new().id(id).width(200.0).show(ui, &theme, buf)
                                }
                                Field::Icon => Input::new()
                                    .id(id)
                                    .icon(&icon)
                                    .width(200.0)
                                    .show(ui, &theme, buf),
                                Field::ReadOnly => Input::new()
                                    .id(id)
                                    .read_only(true)
                                    .width(200.0)
                                    .show(ui, &theme, buf),
                                Field::Auto => {
                                    AutoComplete::new("padding_press_auto")
                                        .width(200.0)
                                        .show(ui, &theme, buf, &[], &mut self.active)
                                        .response
                                }
                            };
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
        frame
    }

    fn focused(&self) -> Option<egui::Id> {
        self.ctx.memory(|m| m.focused())
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

/// 상자 왼쪽 끝에서 가로 padding의 절반 안쪽. 아이콘 칸은 아이콘 가운데.
fn inside_padding(field: Field, rect: Rect) -> Pos2 {
    let theme = tasty_themes::mocha_fallback();
    let pad = theme.input_padding_x().value();
    let x = match field {
        Field::Icon => rect.min.x + pad + theme.icon_glyph_size_md.value() / 2.0,
        _ => rect.min.x + pad / 2.0,
    };
    pos2(x, rect.center().y)
}

#[test]
fn pressing_inside_an_editing_input_box_keeps_its_focus() {
    for field in FIELDS {
        let mut h = Harness::new();
        h.frame(Some(field), vec![]);
        let f = h.frame(None, vec![]);
        let (_, rect, id, _) = *f.boxes.iter().find(|b| b.0 == field).unwrap();
        assert_eq!(h.focused(), Some(id), "{field:?} 포커스 준비");
        let at = inside_padding(field, rect);
        h.frame(None, vec![Event::PointerMoved(at)]);
        for pressed in [true, false] {
            let f = h.frame(None, vec![button(at, pressed)]);
            let lost = f.boxes.iter().find(|b| b.0 == field).unwrap().3;
            assert!(!lost, "{field:?} 누름 {pressed}에 포커스를 잃었다");
        }
        h.frame(None, vec![]);
        assert_eq!(h.focused(), Some(id), "{field:?} 누름 뒤 포커스");
    }
}

/// 편집 중이 아닌 칸의 여백을 눌러도 포커스가 생기지 않는다(전과 같다).
#[test]
fn pressing_the_padding_of_an_idle_input_does_not_focus_it() {
    let mut h = Harness::new();
    h.frame(None, vec![]);
    let f = h.frame(None, vec![]);
    let (_, rect, _, _) = f.boxes[0];
    let at = inside_padding(Field::Plain, rect);
    h.frame(None, vec![Event::PointerMoved(at)]);
    h.frame(None, vec![button(at, true)]);
    h.frame(None, vec![button(at, false)]);
    h.frame(None, vec![]);
    assert_eq!(h.focused(), None);
}

/// 편집 중 상자가 받는 누름은 Tab 순서에 끼지 않는다: Tab은 다음 칸의 글자 칸으로 간다.
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
