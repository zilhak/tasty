//! Exercise the real popup draw lifecycle, including its temporarily moved manager.
use super::*;
use crate::model::{PhysicalPx, PhysicalRect};
use crate::state::tests::test_state;

#[test]
fn search_input_focus_text_and_invalid_edge_survive_popup_draw() {
    let (mut state, mut session) = test_state();
    let engine = session.borrow_mut();
    let ctx = egui::Context::default();
    state
        .popups
        .open_at_focused(SEARCH_BAR_POPUP_ID, egui::pos2(100.0, 100.0));
    let frame = |state: &mut MainViewState, events, modifiers| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 720.0),
                )),
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| {
                crate::adapters::ui::draw_popups(
                    ctx,
                    state,
                    &engine.read(),
                    &[],
                    PhysicalRect {
                        x: PhysicalPx(0.0),
                        y: PhysicalPx(0.0),
                        width: PhysicalPx(1280.0),
                        height: PhysicalPx(720.0),
                    },
                    1.0,
                );
            },
        )
    };
    for _ in 0..20 {
        drop(frame(&mut state, vec![], egui::Modifiers::NONE));
    }
    assert!(
        ctx.memory(|m| m.focused().is_some()),
        "open requests text focus without a click"
    );
    drop(frame(
        &mut state,
        vec![egui::Event::Text("needle".into())],
        egui::Modifiers::NONE,
    ));
    assert_eq!(state.search.query, "needle");
    state.search.last_error = Some("invalid regex".into());
    let focused = frame(&mut state, vec![], egui::Modifiers::NONE);
    let red = crate::theme::theme().input_border_invalid().to_egui();
    fn count_red(shape: &egui::Shape, red: egui::Color32) -> usize {
        match shape {
            egui::Shape::Rect(r) if r.stroke.color == red => 1,
            egui::Shape::Vec(children) => children.iter().map(|s| count_red(s, red)).sum(),
            _ => 0,
        }
    }
    let red_boxes = |out: &egui::FullOutput| {
        out.shapes
            .iter()
            .map(|s| count_red(&s.shape, red))
            .sum::<usize>()
    };
    assert_eq!(red_boxes(&focused), 2, "invalid focused edge and ring");
    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    drop(frame(
        &mut state,
        vec![egui::Event::Key {
            key: egui::Key::F,
            physical_key: Some(egui::Key::F),
            pressed: true,
            repeat: false,
            modifiers: ctrl,
        }],
        ctrl,
    ));
    assert!(state.popups.is_open(SEARCH_BAR_POPUP_ID));
    assert!(
        !state.popups.is_focused(SEARCH_BAR_POPUP_ID),
        "find releases the restored manager's focus"
    );
    let blurred = frame(&mut state, vec![], egui::Modifiers::NONE);
    assert!(ctx.memory(|m| m.focused().is_none()));
    assert_eq!(state.search.query, "needle");
    assert_eq!(
        red_boxes(&blurred),
        1,
        "invalid edge remains without a ring"
    );
}

#[test]
fn search_input_has_canonical_height_and_unclipped_ring_in_the_sized_shell() {
    let (mut state, mut session) = test_state();
    let engine = session.borrow_mut();
    let ctx = egui::Context::default();
    state
        .popups
        .open_at_focused(SEARCH_BAR_POPUP_ID, egui::pos2(100.0, 100.0));
    // An undersized stale instance must be repaired by the registered per-frame sizer.
    state.popups.get_mut(SEARCH_BAR_POPUP_ID).unwrap().size.y = 1.0;
    let mut last = None;
    for _ in 0..20 {
        last = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 720.0),
                )),
                ..Default::default()
            },
            |ctx| {
                crate::adapters::ui::draw_popups(
                    ctx,
                    &mut state,
                    &engine.read(),
                    &[],
                    PhysicalRect {
                        x: PhysicalPx(0.0),
                        y: PhysicalPx(0.0),
                        width: PhysicalPx(1280.0),
                        height: PhysicalPx(720.0),
                    },
                    1.0,
                );
            },
        ));
    }
    let theme = crate::theme::theme();
    let out = last.unwrap();
    let (input, clip) = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(rect) if rect.fill == theme.input_raised_bg().to_egui() => {
                Some((rect.rect, s.clip_rect))
            }
            _ => None,
        })
        .expect("actual search Input fill");
    let popup = state.popups.get_mut(SEARCH_BAR_POPUP_ID).unwrap();
    let margin = crate::adapters::ui::popup::content_margin().value();
    assert_eq!(input.height(), theme.input_height().value());
    assert_eq!(popup.size.y - 2.0 * margin, theme.input_height().value());
    // Input's focus ring uses an expanded rect and an outside stroke, both one border token.
    assert!(clip.contains_rect(input.expand(2.0 * theme.border_width.value())));
}
