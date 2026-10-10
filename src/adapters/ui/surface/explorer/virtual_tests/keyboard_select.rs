//! 키보드 현재 항목과 영역 선택을 세 보기 모드에서 시험한다. 화면 밖 항목까지 가는지, 끄는 동안
//! 가장자리 띠에서 스크롤하며 화면 밖 항목도 고르는지 본다.

use egui::{Event, Modifiers, PointerButton, Pos2};
use tasty_model::ExplorerViewMode;

use super::super::view::cursor::Step;
use super::{CELL, Harness, MODES, N, on_screen, path};

fn selected(h: &Harness) -> Vec<usize> {
    let mut out: Vec<usize> = (0..N)
        .filter(|&i| h.view.selected.contains(&path(i)))
        .collect();
    out.sort_unstable();
    out
}

fn button(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    }
}

#[test]
fn keys_move_the_current_item_off_screen_and_scroll_to_it() {
    for mode in MODES {
        let mut h = Harness::new(mode);
        h.focused = true;
        h.frame(Vec::new());
        h.frame(Vec::new());
        h.view.move_cursor(Step::End, false);
        assert_eq!(selected(&h), vec![N - 1], "{mode:?}: End");
        let drawn = h.settle();
        assert!(
            on_screen(&drawn, N - 1).is_some(),
            "{mode:?}: scrolled to the end"
        );
        assert_eq!(h.view.cursor.shown_item(), Some(path(N - 1).as_path()));

        h.view.move_cursor(Step::PageUp, false);
        let page = N - 1 - selected(&h)[0];
        assert!(page > 1, "{mode:?}: a page is more than one item ({page})");
        let drawn = h.settle();
        assert!(
            on_screen(&drawn, N - 1 - page).is_some(),
            "{mode:?}: page target shown"
        );

        // Shift 를 더하면 기준 항목부터 넓힌다.
        h.view.move_cursor(Step::Up, true);
        h.view.move_cursor(Step::Up, true);
        let cols = h.view.list_layout.map_or(1, |l| l.cols);
        let from = N - 1 - page;
        let want: Vec<usize> = (from - 2 * cols..=from).collect();
        assert_eq!(selected(&h), want, "{mode:?}: extended by two lines");

        // Detail·List 의 왼쪽·오른쪽은 움직이지 않고, Grid 에서는 한 칸 간다.
        h.view.move_cursor(Step::Home, false);
        h.view.move_cursor(Step::Right, false);
        let expect = if mode == ExplorerViewMode::Grid { 1 } else { 0 };
        assert_eq!(selected(&h), vec![expect], "{mode:?}: right");
    }
}

#[test]
fn a_click_hides_the_ring_until_the_next_key() {
    let mut h = Harness::new(ExplorerViewMode::List);
    h.focused = true;
    h.frame(Vec::new());
    h.view.move_cursor(Step::Down, false);
    let drawn = h.frame(Vec::new());
    assert!(h.view.cursor.shown_item().is_some());
    let at = on_screen(&drawn, 5).expect("item 5").center();
    h.frame(super::click(at));
    assert_eq!(h.view.cursor.shown_item(), None);
    // 다음 키는 클릭한 항목에서 시작한다.
    h.view.move_cursor(Step::Down, false);
    assert_eq!(selected(&h), vec![6]);
}

/// 목록 아래 빈 곳에서 위로 끌면 사각형에 걸친 줄을 고른다.
#[test]
fn dragging_from_empty_space_selects_the_rows_it_crosses() {
    for mode in [ExplorerViewMode::Detail, ExplorerViewMode::List] {
        let mut h = Harness::new(mode);
        h.view.entries.truncate(10);
        h.frame(Vec::new());
        let drawn = h.frame(Vec::new());
        let r7 = on_screen(&drawn, 7).expect("item 7");
        let r9 = on_screen(&drawn, 9).expect("item 9");
        let below = egui::pos2(r9.center().x + 40.0, r9.bottom() + 60.0);
        h.frame(vec![Event::PointerMoved(below), button(below, true)]);
        for step in 1..=10 {
            let t = step as f32 / 10.0;
            let at = below + (r7.center() - below) * t;
            h.frame(vec![Event::PointerMoved(at)]);
        }
        assert!(h.view.marquee.active(), "{mode:?}: marquee started");
        assert_eq!(selected(&h), vec![7, 8, 9], "{mode:?}");
        h.frame(vec![button(r7.center(), false)]);
        assert!(!h.view.marquee.active(), "{mode:?}: released");
        assert_eq!(selected(&h), vec![7, 8, 9], "{mode:?}: kept after release");
    }
}

/// Grid 오른쪽 빈 곳에서 아래 가장자리 띠로 끌고 있으면 목록이 스크롤되고 화면 밖이던 항목도 고른다.
#[test]
fn holding_the_drag_in_the_bottom_band_scrolls_and_keeps_selecting() {
    let mut h = Harness::new(ExplorerViewMode::Grid);
    h.frame(Vec::new());
    h.frame(Vec::new());
    let layout = h.view.list_layout.expect("grid layout");
    let list = h.view.list_rect.expect("list rect");
    let right = layout.origin.x + layout.cols as f32 * layout.step.x;
    assert!(
        right + 4.0 < list.right(),
        "the grid leaves room on the right"
    );
    let start = egui::pos2((right + list.right()) / 2.0, layout.origin.y + 4.0);
    let band = egui::pos2(layout.origin.x + 4.0, list.bottom() - 4.0);
    h.frame(vec![Event::PointerMoved(start), button(start, true)]);
    for step in 1..=10 {
        let at = start + (band - start) * (step as f32 / 10.0);
        h.frame(vec![Event::PointerMoved(at)]);
    }
    let before = selected(&h).len();
    for _ in 0..60 {
        h.frame(Vec::new());
    }
    let after = selected(&h);
    assert!(
        after.len() > before + layout.cols,
        "scrolling kept selecting: {before} -> {}",
        after.len()
    );
    // 처음 누른 줄(항목 0 이 있는 줄)은 화면 밖으로 나가도 선택에 남는다.
    assert_eq!(after.first(), Some(&0));
    assert!(CELL.contains(band));
}
