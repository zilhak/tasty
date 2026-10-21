//! 표시 문자열과 별개로 편집·복사·이동의 원문을 실제 입력 이벤트로 확인한다.
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_ui_widgets::{PathField, PathFieldOutcome};

const RAW: &str = "/tmp/\u{1100}\u{1161}/cafe\u{301}";

struct State {
    buffer: String,
    editing: bool,
    active: Option<usize>,
}

fn frame(
    ctx: &egui::Context,
    state: &mut State,
    nfc: bool,
    focus: bool,
    events: Vec<Event>,
) -> (egui::FullOutput, PathFieldOutcome) {
    let theme = tasty_themes::mocha_fallback();
    let mut outcome = PathFieldOutcome::None;
    let out = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 300.0))),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                outcome = PathField::new("nfd")
                    .display_nfc(nfc)
                    .focus(focus)
                    .width(400.0)
                    .go_icon(&|_, _, _| {})
                    .show(
                        ui,
                        &theme,
                        &mut state.buffer,
                        &mut state.editing,
                        &mut state.active,
                        &[RAW],
                        RAW,
                    );
            });
        },
    );
    (out, outcome)
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

#[test]
fn nfc_idle_paint_never_enters_edit_copy_submit_or_candidate_values() {
    let ctx = egui::Context::default();
    let mut state = State {
        buffer: RAW.into(),
        editing: false,
        active: None,
    };
    let (out, _) = frame(&ctx, &mut state, true, false, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "/tmp/가/café"))
    );
    assert_eq!(state.buffer.as_bytes(), RAW.as_bytes());
    frame(
        &ctx,
        &mut state,
        true,
        false,
        pointer(pos2(80.0, 22.0), true),
    );
    frame(
        &ctx,
        &mut state,
        true,
        false,
        pointer(pos2(80.0, 22.0), false),
    );
    assert!(state.editing);
    let id = ctx.memory(|m| m.focused()).unwrap();
    let range = egui::TextEdit::load_state(&ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert_eq!(
        range.primary.index.max(range.secondary.index),
        RAW.chars().count()
    );
    assert_eq!(range.primary.index.min(range.secondary.index), 0);
    let (copied, _) = frame(&ctx, &mut state, true, false, vec![Event::Copy]);
    assert!(copied.platform_output.commands.iter().any(
        |c| matches!(c, egui::OutputCommand::CopyText(text) if text.as_bytes() == RAW.as_bytes())
    ));
    let (_, outcome) = frame(&ctx, &mut state, true, false, key(Key::Enter));
    assert_eq!(outcome, PathFieldOutcome::Navigate(RAW.into()));
    frame(&ctx, &mut state, true, false, vec![]);
    frame(&ctx, &mut state, true, true, vec![]);
    assert!(state.editing);
    // 프로그램 포커스 요청 뒤 입력 필드가 키 필터를 설치하는 다시 그리기를 거친다.
    frame(&ctx, &mut state, true, false, vec![]);
    frame(&ctx, &mut state, true, false, key(Key::ArrowDown));
    assert_eq!(state.active, Some(0));
    let (_, picked) = frame(&ctx, &mut state, true, false, key(Key::Enter));
    assert_eq!(picked, PathFieldOutcome::Navigate(RAW.into()));
    assert_eq!(state.buffer, RAW);
    ctx.memory_mut(|m| m.surrender_focus(id));
    state.editing = false;
    frame(
        &ctx,
        &mut state,
        true,
        false,
        pointer(pos2(394.0, 22.0), true),
    );
    let (_, go) = frame(
        &ctx,
        &mut state,
        true,
        false,
        pointer(pos2(394.0, 22.0), false),
    );
    assert_eq!(go, PathFieldOutcome::Navigate(RAW.into()));
}

#[test]
fn normal_path_fields_keep_raw_display_and_focus_request_uses_raw_char_count() {
    let ctx = egui::Context::default();
    let mut state = State {
        buffer: RAW.into(),
        editing: false,
        active: None,
    };
    let (out, _) = frame(&ctx, &mut state, false, false, vec![]);
    assert!(
        !out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "/tmp/가/café"))
    );
    frame(&ctx, &mut state, true, true, vec![]);
    let id = ctx.memory(|m| m.focused()).unwrap();
    let range = egui::TextEdit::load_state(&ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert_eq!(
        range.primary.index.max(range.secondary.index),
        RAW.chars().count()
    );
    assert_eq!(state.buffer, RAW);
    frame(
        &ctx,
        &mut state,
        true,
        false,
        vec![Event::Text("/different".into())],
    );
    assert_eq!(state.buffer, "/different");
    let (_, cancelled) = frame(&ctx, &mut state, true, false, key(Key::Escape));
    assert_eq!(cancelled, PathFieldOutcome::Revert);
    assert_eq!(state.buffer, RAW);
}
