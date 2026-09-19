//! scrim 이 실제로 **어느 rect 에 칠해지는지**를 그려 보고 재는 시험.
//!
//! [`super::draw`] 의 순수 판정기(`pick_scrim_layers`·`scope_bounds`)는 그 파일의 유닛
//! 시험이 든다. 여기서 재는 것은 그 판정이 **painter 까지 이어졌는가**다 — 판정기가
//! 옳아도 호출부가 여전히 화면 전체를 칠하면 소스만 읽어서는 그 차이가 안 보인다.
//! 그래서 egui 프레임을 한 번 돌려 나온 도형에서 scrim 색 사각형만 골라 센다.

use crate::adapters::ui::LayoutContext;
use crate::adapters::ui::popup::{PopupScope, defs, frame::draw_popup_layer};
use crate::state::AppState;
use crate::state::tests::test_state;

const SURFACE: u32 = 17;

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))
}

fn surface_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(100.0, 60.0), egui::vec2(400.0, 360.0))
}

fn layout() -> LayoutContext {
    LayoutContext {
        active_workspace: 0,
        pane_rects: vec![],
        surface_rects: vec![(SURFACE, surface_rect())],
        active_tabs: vec![],
    }
}

fn prepared() -> (AppState, crate::core::CoreState) {
    let (mut state, engine) = test_state();
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    (state, engine)
}

/// 한 프레임을 그려 scrim 색으로 칠해진 사각형들의 rect 를 모은다.
fn scrim_rects(state: &mut AppState, engine: &mut crate::core::CoreState) -> Vec<egui::Rect> {
    let scrim = egui::Color32::from(crate::theme::theme().scrim());
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(screen()),
        ..Default::default()
    };
    let out = ctx.run(raw, |ctx| {
        draw_popup_layer(ctx, state, engine, &layout());
    });
    let mut found = Vec::new();
    for clipped in &out.shapes {
        if let egui::Shape::Rect(r) = &clipped.shape
            && r.fill == scrim
        {
            found.push(r.rect);
        }
    }
    found
}

/// surface 범위 popup 의 scrim 은 그 칸 하나를 덮는다 — 화면 전체가 아니다.
#[test]
fn a_surface_scoped_popup_dims_only_its_own_surface() {
    let (mut state, mut engine) = prepared();
    state.dialogs.convert_popup = Some(SURFACE);
    state
        .popups
        .open_with_scope("convert_surface", PopupScope::Surface(SURFACE));
    let rects = scrim_rects(&mut state, &mut engine);
    assert_eq!(rects, vec![surface_rect()]);
}

/// 창 범위 popup 은 종전대로 창 전체를 덮는다.
#[test]
fn a_window_scoped_popup_still_dims_the_whole_window() {
    let (mut state, mut engine) = prepared();
    state
        .popups
        .open_with_scope("command_palette", PopupScope::Window);
    let rects = scrim_rects(&mut state, &mut engine);
    assert_eq!(rects, vec![screen()]);
}

/// 부모와 자식이 같은 scope 를 쓰면 scrim 은 **한 번만** 칠해진다. 두 번 칠하면 같은
/// 알파가 곱해져 그 칸만 두 배로 어두워진다.
#[test]
fn a_parent_and_its_child_picker_share_one_scrim() {
    let (mut state, mut engine) = prepared();
    state.dialogs.convert_popup = Some(SURFACE);
    state
        .popups
        .open_with_scope("convert_surface", PopupScope::Surface(SURFACE));
    state.popups.open_with_scope(
        super::file_picker::FILE_PICKER_POPUP_ID,
        PopupScope::Surface(SURFACE),
    );
    let rects = scrim_rects(&mut state, &mut engine);
    assert_eq!(rects, vec![surface_rect()]);
}

/// scrim 을 요구하는 popup 이 **둘** 같은 칸에 떠도 scrim 은 한 번이다.
///
/// 위 부모+자식 시험과 재는 것이 다르다 — 거기서 자식(file picker)은 애초에 scrim 을
/// 요구하지 않으므로 그 시험은 "자식이 제 것을 덧그리지 않는다" 만 든다. 중복 제거
/// 자체는 scrim 을 **둘 다 요구하는** 이 배치에서만 재진다.
#[test]
fn two_scrim_popups_in_one_scope_still_paint_one_scrim() {
    let (mut state, mut engine) = prepared();
    state.dialogs.convert_popup = Some(SURFACE);
    state
        .popups
        .open_with_scope("convert_surface", PopupScope::Surface(SURFACE));
    state
        .popups
        .open_with_scope("port_scanner", PopupScope::Surface(SURFACE));
    let rects = scrim_rects(&mut state, &mut engine);
    assert_eq!(rects, vec![surface_rect()]);
}

/// 창 scrim 이 있으면 그 안의 surface scrim 은 안 칠한다 — 겹치는 자리가 두 배로
/// 어두워지지 않는다.
#[test]
fn a_window_scrim_absorbs_the_surface_scrim_under_it() {
    let (mut state, mut engine) = prepared();
    state.dialogs.convert_popup = Some(SURFACE);
    state
        .popups
        .open_with_scope("convert_surface", PopupScope::Surface(SURFACE));
    state
        .popups
        .open_with_scope("command_palette", PopupScope::Window);
    let rects = scrim_rects(&mut state, &mut engine);
    assert_eq!(rects, vec![screen()]);
}
