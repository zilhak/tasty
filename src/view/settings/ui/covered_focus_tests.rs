//! 창을 덮는 선택기 아래 설정 창 위젯은 egui 포커스를 받지 않는다. 덮음 막이 레이어가 창의 egui 하한이다
//! (`PopupManager::window_cover_floor`, 패스 끝 등록 `PopupManager::finish_pass_focus`). 장면은 별도 창 사건 입구
//! 하네스로 실제 설정 창(`draw_settings_panel`)을 그린다.

use super::*;
use crate::adapters::ui::popup::window_input_test_support::click_widget_at;
use crate::view::window_input::tests::Harness;

/// 지금 egui 포커스 위젯이 직전 패스에 그려진 레이어 층. 포커스가 없으면 None 이다.
fn focused_order(ctx: &egui::Context) -> Option<egui::Order> {
    focused(ctx)
        .and_then(|id| ctx.viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.layer_id.order)))
}

/// 별도 창 사건 입구로 Tab(`shift` 면 Shift+Tab) 하나를 보낸다.
fn tab(h: &mut Harness, st: &mut SettingsUiState, shift: bool) {
    let modifiers = if shift {
        egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::NONE
    };
    h.key_with(st, egui::Key::Tab, modifiers, None);
}

/// 읽기를 끝낸 폴더 선택기가 창을 덮은 설정 창. 버튼 자리를 찾을 출력을 함께 돌려준다.
/// 선택기 시작 폴더. 만들지도 읽지도 않는다(읽기는 빈 목록으로 채운다, `loaded_chooser`).
fn start_dir() -> std::path::PathBuf {
    // 사유: 만들지도 읽지도 않는 경로라 동시 실행이 덮어쓸 것이 없다.
    std::env::temp_dir().join("tasty-covered-focus-start")
}

fn covered() -> (Harness, Panel, SettingsUiState, egui::FullOutput) {
    let (mut p, mut st) = plain_panel();
    let mut h = Harness::new();
    let out = loaded_chooser(&mut h, &mut p, &mut st, start_dir());
    (h, p, st, out)
}

/// 선택기 취소 버튼의 위젯 id.
fn cancel_button(h: &Harness, out: &egui::FullOutput) -> egui::Id {
    click_widget_at(&h.ctx, text_rect_in(out, t("button.cancel")).center()).expect("cancel button")
}

/// 선택기 없이 설정 창을 그리고 L2 사이드바 필터 입력칸에 egui 포커스를 준다. 그 입력칸 id 를 돌려준다.
fn focused_filter(h: &mut Harness, p: &mut Panel, st: &mut SettingsUiState) -> egui::Id {
    drop(drain_settings(h, p, st));
    let out = drain_settings(h, p, st);
    let filter = focusable_at_text(
        &h.ctx,
        &out,
        t("settings.filter.sections"),
        egui::Order::Background,
    );
    h.ctx.memory_mut(|m| m.request_focus(filter));
    drop(drain_settings(h, p, st));
    assert_eq!(focused(&h.ctx), Some(filter), "the filter has the focus");
    filter
}

/// T-TAB-1: Given 설정 창을 덮은 선택기가 이미 그려졌고 egui 포커스가 없다, When Tab 을 치면, Then 포커스는 덮인 L1
/// 탭이 아니라 선택기 안(Foreground) 위젯으로 간다.
#[test]
fn tab_under_a_covering_chooser_keeps_the_settings_tabs_unfocused() {
    let (mut h, mut p, mut st, _) = covered();
    tab(&mut h, &mut st, false);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_eq!(
        focused_order(&h.ctx),
        Some(egui::Order::Foreground),
        "Tab moves into the covering chooser, not to a settings tab"
    );
}

/// T-TAB-1c: Given 설정 창을 덮은 선택기, When Shift+Tab 을 거듭 치면, Then 포커스는 한 번도 덮인 설정 창 위젯으로
/// 가지 않고 선택기 안을 돈다.
#[test]
fn shift_tab_under_a_cover_stays_off_the_covered_window() {
    let (mut h, mut p, mut st, _) = covered();
    let mut inside = 0;
    for step in 0..40 {
        tab(&mut h, &mut st, true);
        drop(drain_settings(&mut h, &mut p, &mut st));
        let order = focused_order(&h.ctx);
        assert_ne!(
            order,
            Some(egui::Order::Background),
            "Shift+Tab {step} moved to a covered settings widget"
        );
        inside += usize::from(order == Some(egui::Order::Foreground));
    }
    assert!(inside > 0, "Shift+Tab moves inside the chooser");
}

/// T-TAB-2a: Given 필터 입력칸에 egui 포커스가 있던 채 선택기가 창을 덮었다, When Escape 로 선택기를 닫으면, Then
/// 덮였던 필터 입력칸에 포커스가 없다.
#[test]
fn escape_close_leaves_no_focus_on_the_covered_widget() {
    let (mut p, mut st) = plain_panel();
    let mut h = Harness::new();
    let filter = focused_filter(&mut h, &mut p, &mut st);
    drop(loaded_chooser(&mut h, &mut p, &mut st, start_dir()));
    h.key(&mut st, egui::Key::Escape, None);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert!(
        !st.popups.is_open(FILE_PICKER_POPUP_ID),
        "Escape closes the chooser"
    );
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_ne!(
        focused(&h.ctx),
        Some(filter),
        "the covered filter has no focus after the close"
    );
}

/// T-TAB-2b: Given T-TAB-2a 와 같은 장면, When 취소 버튼으로 선택기를 닫으면, Then 덮였던 필터 입력칸에 포커스가 없다.
#[test]
fn cancel_close_leaves_no_focus_on_the_covered_widget() {
    let (mut p, mut st) = plain_panel();
    let mut h = Harness::new();
    let filter = focused_filter(&mut h, &mut p, &mut st);
    let out = loaded_chooser(&mut h, &mut p, &mut st, start_dir());
    h.click(&mut st, text_rect_in(&out, t("button.cancel")).center());
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert!(
        !st.popups.is_open(FILE_PICKER_POPUP_ID),
        "Cancel closes the chooser"
    );
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_ne!(
        focused(&h.ctx),
        Some(filter),
        "the covered filter has no focus after the close"
    );
}

/// T-TAB-3: Given 선택기 확정 버튼(선택기 안 마지막 위젯이 아님)에 egui 포커스가 있다, When Tab 을 치면, Then
/// 포커스는 선택기 안의 다른 위젯으로 옮겨진다.
#[test]
fn tab_inside_the_cover_moves_between_cover_widgets() {
    let (mut h, mut p, mut st, out) = covered();
    let confirm = click_widget_at(
        &h.ctx,
        text_rect_in(&out, t("picker.folder.confirm")).center(),
    )
    .expect("confirm button");
    h.ctx.memory_mut(|m| m.request_focus(confirm));
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_eq!(
        focused(&h.ctx),
        Some(confirm),
        "the confirm button has the focus"
    );
    tab(&mut h, &mut st, false);
    drop(drain_settings(&mut h, &mut p, &mut st));
    let next = focused(&h.ctx);
    assert!(
        next.is_some() && next != Some(confirm),
        "Tab moves the focus: {next:?}"
    );
    assert_eq!(
        focused_order(&h.ctx),
        Some(egui::Order::Foreground),
        "the next widget is inside the chooser"
    );
}

/// T-TAB-6: Given 선택기 숨은 경로 메뉴(선택기의 등록 자식, 막이 위 Foreground 레이어)가 열렸다, When 메뉴 항목 위로
/// 포인터를 옮기고 그 항목에 포커스를 주면, Then 그 자리의 레이어는 하한이 아니라 메뉴이고 항목은 포커스를 지킨다.
#[test]
fn a_dropdown_inside_the_cover_keeps_focus_and_hover() {
    let root = tempfile::tempdir().expect("임시 폴더");
    let mut deep = root.path().to_path_buf();
    for i in 0..40 {
        deep.push(format!("d{i:02}"));
    }
    std::fs::create_dir_all(&deep).expect("깊은 폴더");
    let (mut p, mut st) = plain_panel();
    let mut h = Harness::new();
    let out = loaded_chooser(&mut h, &mut p, &mut st, deep);
    h.click(&mut st, text_rect_in(&out, "…").center());
    drop(drain_settings(&mut h, &mut p, &mut st));
    // 처음 나타난 메뉴 영역은 크기만 잰다. 그다음 패스의 출력에서 항목 자리를 읽는다.
    drop(drain_settings(&mut h, &mut p, &mut st));
    let out = drain_settings(&mut h, &mut p, &mut st);
    assert!(
        h.ctx.memory(|m| m.any_popup_open()),
        "the crumbs menu is open"
    );
    let item = text_rect_in(&out, "d00").center();
    let floor = st.popups.window_cover_floor().expect("the chooser floor");
    h.hover(item);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert!(
        h.ctx.memory(|m| m.any_popup_open()),
        "the crumbs menu stays open"
    );
    let layer = h.ctx.layer_id_at(item).expect("a layer under the item");
    assert_ne!(layer, floor, "the menu item is above the floor");
    assert_eq!(layer.order, egui::Order::Foreground);
    let id = click_widget_at(&h.ctx, item).expect("the menu item");
    h.ctx.memory_mut(|m| m.request_focus(id));
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert!(
        h.ctx.memory(|m| m.any_popup_open()),
        "the crumbs menu stays open"
    );
    assert_eq!(focused(&h.ctx), Some(id), "the menu item keeps the focus");
}

/// T-TAB-7: Given 필터 입력칸 위를 덮은 선택기를 취소로 닫고 막이가 없는 패스가 한 번 반영됐다, When 필터 입력칸을
/// 누르면, Then 그 입력칸이 포커스를 얻는다.
#[test]
fn a_press_after_the_cover_closes_reaches_the_covered_widget() {
    let (mut p, mut st) = plain_panel();
    let mut h = Harness::new();
    let filter = focused_filter(&mut h, &mut p, &mut st);
    let at = h
        .ctx
        .viewport(|v| v.prev_pass.widgets.get(filter).map(|w| w.rect))
        .expect("filter")
        .center();
    let out = loaded_chooser(&mut h, &mut p, &mut st, start_dir());
    h.click(&mut st, text_rect_in(&out, t("button.cancel")).center());
    drop(drain_settings(&mut h, &mut p, &mut st));
    drop(drain_settings(&mut h, &mut p, &mut st));
    h.click(&mut st, at);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_eq!(
        focused(&h.ctx),
        Some(filter),
        "the press reaches the filter"
    );
}

/// T-TAB-8: Given 선택기 안에 포커스가 없는 채 Escape 로 선택기를 닫았다, When 다음 몫에 Tab 을 치면, Then 포커스가
/// 창 위젯(Background)으로 간다.
#[test]
fn the_first_tab_after_the_cover_closes_moves_inside_the_window() {
    let (mut h, mut p, mut st, _) = covered();
    assert_eq!(
        focused(&h.ctx),
        None,
        "nothing has the focus inside the chooser"
    );
    h.key(&mut st, egui::Key::Escape, None);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert!(
        !st.popups.is_open(FILE_PICKER_POPUP_ID),
        "Escape closes the chooser"
    );
    tab(&mut h, &mut st, false);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_eq!(
        focused_order(&h.ctx),
        Some(egui::Order::Background),
        "the first Tab after the close moves to a window widget"
    );
}

/// T-TAB-8b: Given 선택기 취소 버튼에 egui 포커스가 있다, When Escape 로 닫고 다음 몫에 Tab 을 치면, Then 닫힌 버튼의
/// 포커스가 그 Tab 을 삼키지 않고 포커스가 창 위젯(Background)으로 간다.
#[test]
fn closing_a_cover_with_focus_inside_lets_the_first_tab_move_in_the_window() {
    let (mut h, mut p, mut st, out) = covered();
    let cancel = cancel_button(&h, &out);
    h.ctx.memory_mut(|m| m.request_focus(cancel));
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_eq!(
        focused(&h.ctx),
        Some(cancel),
        "the cancel button has the focus"
    );
    h.key(&mut st, egui::Key::Escape, None);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert!(
        !st.popups.is_open(FILE_PICKER_POPUP_ID),
        "Escape closes the chooser"
    );
    tab(&mut h, &mut st, false);
    drop(drain_settings(&mut h, &mut p, &mut st));
    assert_eq!(
        focused_order(&h.ctx),
        Some(egui::Order::Background),
        "the first Tab after the close moves to a window widget"
    );
}

/// T-TAB-10: Given 선택기 확정 버튼에 egui 포커스가 있다, When 방향키(↓·↑·←·→)를 치면, Then 포커스가 덮인 설정 창
/// 위젯에 남지 않는다. egui 방향 이동은 하한을 보지 않지만 덮인 위젯은 다음 그리기에서 포커스를 놓는다.
#[test]
fn arrow_keys_inside_the_cover_leave_no_focus_on_the_covered_window() {
    let (mut h, mut p, mut st, out) = covered();
    let confirm = click_widget_at(
        &h.ctx,
        text_rect_in(&out, t("picker.folder.confirm")).center(),
    )
    .expect("confirm button");
    h.ctx.memory_mut(|m| m.request_focus(confirm));
    drop(drain_settings(&mut h, &mut p, &mut st));
    for key in [
        egui::Key::ArrowDown,
        egui::Key::ArrowUp,
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
    ] {
        h.key(&mut st, key, None);
        drop(drain_settings(&mut h, &mut p, &mut st));
        assert_ne!(
            focused_order(&h.ctx),
            Some(egui::Order::Background),
            "{key:?} left the focus on a covered settings widget"
        );
    }
}
