//! 후보를 누르는 프레임과 떼는 프레임 사이에 입력 원복이 끼지 않아야 한다.
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_ui_widgets::{AutoComplete, AutoCompleteAction, Input, PathField, PathFieldOutcome};

const QUERY: &str = "/tmp/caf";
const CURRENT: &str = "/tmp/current";
const TARGET: &str = "/tmp/cafe\u{301}";

#[derive(Default)]
struct Frame {
    path: Option<PathFieldOutcome>,
    auto: Option<AutoCompleteAction>,
    target: Option<Pos2>,
    other: Option<egui::Id>,
}

struct Harness {
    ctx: egui::Context,
    buffer: String,
    editing: bool,
    active: Option<usize>,
    nfc: bool,
    standalone: bool,
}

impl Harness {
    fn new(nfc: bool, standalone: bool) -> Self {
        Self {
            ctx: egui::Context::default(),
            buffer: QUERY.into(),
            editing: false,
            active: None,
            nfc,
            standalone,
        }
    }

    fn frame(&mut self, focus: bool, events: Vec<Event>) -> Frame {
        let theme = tasty_themes::mocha_fallback();
        let mut result = Frame::default();
        let out = self.ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 300.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if self.standalone {
                            let out = AutoComplete::new("pointer").width(400.0).show(
                                ui,
                                &theme,
                                &mut self.buffer,
                                &[TARGET],
                                &mut self.active,
                            );
                            if focus {
                                out.response.request_focus();
                            }
                            result.auto = Some(out.action);
                        } else {
                            result.path = Some(
                                PathField::new("pointer")
                                    .width(400.0)
                                    .display_nfc(self.nfc)
                                    .focus(focus)
                                    .show(
                                        ui,
                                        &theme,
                                        &mut self.buffer,
                                        &mut self.editing,
                                        &mut self.active,
                                        &[TARGET],
                                        CURRENT,
                                    ),
                            );
                        }
                        let mut other = String::new();
                        result.other =
                            Some(Input::new().width(200.0).show(ui, &theme, &mut other).id);
                    });
                });
            },
        );
        result.target = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == QUERY => Some(t.pos + vec2(5.0, 5.0)),
                _ => None,
            })
            .max_by(|a, b| a.y.total_cmp(&b.y));
        result
    }

    fn open(&mut self) -> Pos2 {
        self.frame(true, vec![]);
        self.frame(false, vec![]);
        self.frame(false, vec![])
            .target
            .expect("후보를 그려야 한다")
    }
}

fn pointer(pos: Pos2, pressed: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        },
    ]
}

fn key(key: Key) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .collect()
}

#[test]
fn pointer_press_keeps_query_until_release_commits_the_raw_candidate() {
    for nfc in [false, true] {
        let mut h = Harness::new(nfc, false);
        let at = h.open();
        h.frame(false, vec![Event::PointerMoved(at)]);
        let down = h.frame(false, pointer(at, true));
        assert_eq!(down.path, Some(PathFieldOutcome::None));
        assert_eq!(h.buffer, QUERY);
        assert!(h.editing);
        let up = h.frame(false, pointer(at, false));
        assert_eq!(up.path, Some(PathFieldOutcome::Navigate(TARGET.into())));
        assert_eq!(h.buffer.as_bytes(), TARGET.as_bytes());
        assert!(!h.editing);
        assert_eq!(h.frame(false, vec![]).path, Some(PathFieldOutcome::None));
    }
}

#[test]
fn outside_press_reverts_and_leaves_the_other_input_focused() {
    let mut h = Harness::new(true, false);
    h.open();
    let at = pos2(500.0, 22.0);
    h.frame(false, vec![Event::PointerMoved(at)]);
    let down = h.frame(false, pointer(at, true));
    assert_eq!(down.path, Some(PathFieldOutcome::Revert));
    assert_eq!(h.buffer, CURRENT);
    h.frame(false, pointer(at, false));
    assert_eq!(h.ctx.memory(|m| m.focused()), down.other);
}

#[test]
fn escape_cancels_a_pressed_candidate_without_committing_on_release() {
    let mut h = Harness::new(true, false);
    let at = h.open();
    h.frame(false, vec![Event::PointerMoved(at)]);
    h.frame(false, pointer(at, true));
    assert_eq!(
        h.frame(false, key(Key::Escape)).path,
        Some(PathFieldOutcome::Revert)
    );
    assert_eq!(h.buffer, CURRENT);
    assert!(!h.editing);
    let up = h.frame(false, pointer(at, false));
    assert!(!matches!(up.path, Some(PathFieldOutcome::Navigate(_))));
    assert!(!h.editing);
}

#[test]
fn releasing_outside_a_pressed_row_does_not_select_it() {
    let mut h = Harness::new(false, false);
    let at = h.open();
    h.frame(false, vec![Event::PointerMoved(at)]);
    h.frame(false, pointer(at, true));
    let outside = pos2(700.0, 200.0);
    let up = h.frame(false, pointer(outside, false));
    assert!(!matches!(up.path, Some(PathFieldOutcome::Navigate(_))));
    assert_eq!(h.buffer, QUERY);
    assert_eq!(
        h.frame(false, pointer(outside, true)).path,
        Some(PathFieldOutcome::Revert)
    );
}

#[test]
fn standalone_autocomplete_returns_the_original_candidate_and_closes() {
    let mut h = Harness::new(false, true);
    let at = h.open();
    h.frame(false, vec![Event::PointerMoved(at)]);
    assert_eq!(
        h.frame(false, pointer(at, true)).auto,
        Some(AutoCompleteAction::None)
    );
    assert_eq!(
        h.frame(false, pointer(at, false)).auto,
        Some(AutoCompleteAction::Pick(TARGET.into()))
    );
    // 단독 AutoComplete는 버퍼 갱신을 호출자에게 맡기는 기존 계약을 유지한다.
    assert_eq!(h.buffer, QUERY);
    assert_eq!(h.ctx.memory(|m| m.focused()), None);
}
