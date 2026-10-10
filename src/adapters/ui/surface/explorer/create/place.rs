//! 폴더 안 만들기의 편집 줄 자리. 폴더 행 메뉴로 시작하면 그 폴더가 목록에 있는 동안 입력을 폴더 바로
//! 다음 자리(Grid 는 폴더 칸 다음 칸)에 끼우고, 그 밖에는 목록 맨 앞에 둔다. 목록 순번에 편집 줄 하나를
//! 끼운 "가상 순번" 으로 보이는 범위를 계산한 뒤 실제 항목 범위로 되돌린다.

use std::ops::Range;

use super::super::view::ExplorerView;
use super::super::view::list_layout::ListLayout;
use super::super::{EntryCtx, ExplorerAction, grid_entry};
use super::{Slot, name_row};

/// 대상 폴더가 지금 목록에 있으면 그 순번. 입력을 그 아래에 들여 쓰고 폴더에 놓을 대상 표시를 한다.
pub(in super::super) fn target_row(view: &ExplorerView) -> Option<usize> {
    let create = view.create.as_ref()?;
    if view.shown_dir() == Some(create.dir.as_path()) {
        return None;
    }
    view.shown().position(|e| e.is_dir && e.path == create.dir)
}

/// 편집 줄의 가상 순번. 입력이 없으면 `None`.
pub(in super::super) fn slot_index(view: &ExplorerView) -> Option<usize> {
    view.create.as_ref()?;
    Some(target_row(view).map_or(0, |row| row + 1))
}

/// 목록 순번 `index` 의 가상 순번. 편집 줄 자리와 그 뒤 항목은 한 칸 밀린다.
/// 이번 프레임에 남긴 칸 배치를 편집 칸에 맞춘다. 맨 앞 편집 칸은 `top` 으로 그 자리만큼 밀고,
/// 항목 사이에 낀 편집 칸은 칸 간격을 고르지 않게 만들어 배치를 두지 않는다(그동안 영역 선택과
/// 쪽 단위 이동은 쉰다. 입력 밖을 누르면 입력이 먼저 닫히고 다음 프레임에 배치가 돌아온다).
pub(in super::super) fn fit_layout(
    view: &mut ExplorerView,
    at: Option<usize>,
    top: impl FnOnce(&mut ListLayout),
) {
    match (at, view.list_layout.as_mut()) {
        (Some(0), Some(layout)) => top(layout),
        (Some(_), Some(_)) => view.list_layout = None,
        _ => {}
    }
}

pub(in super::super) fn virtual_index(at: Option<usize>, index: usize) -> usize {
    index + usize::from(at.is_some_and(|a| a <= index))
}

/// 가상 순번 범위를 실제 항목 범위와 편집 줄 위치(범위 시작에서 몇 번째)로 나눈다.
fn split_span(at: Option<usize>, span: Range<usize>) -> (Range<usize>, Option<usize>) {
    let real = |v: usize| v - usize::from(at.is_some_and(|a| a < v));
    let edit = at.filter(|a| span.contains(a)).map(|a| a - span.start);
    (real(span.start)..real(span.end), edit)
}

/// Grid 한 줄의 가상 순번 `span` 을 그린다. `target` 은 이 프레임에 보이게 할 칸의 가상 순번이다.
pub(in super::super) fn grid_run(
    ui: &mut egui::Ui,
    ctx: &EntryCtx<'_>,
    view: &mut ExplorerView,
    at: Option<usize>,
    span: Range<usize>,
    target: Option<usize>,
    action: &mut Option<ExplorerAction>,
) {
    let start = span.start;
    let (real, edit) = split_span(at, span);
    let items = view.shown_range(real);
    for (i, e) in items.iter().enumerate() {
        if edit == Some(i) {
            name_row(ui, ctx.theme, view, Slot::Grid, action);
        }
        let v = start + i + usize::from(edit.is_some_and(|x| x <= i));
        grid_entry(ui, ctx, view, e, target == Some(v), action);
    }
    if edit == Some(items.len()) {
        name_row(ui, ctx.theme, view, Slot::Grid, action);
    }
}

/// Grid 의 첫 줄 아래 줄 배치. `open_span` 이 범위 위 빈자리를 띄운 뒤 부른다.
pub(in super::super) struct GridLines {
    pub(in super::super) at: Option<usize>,
    /// 첫 줄에 놓인 가상 칸 수.
    pub(in super::super) in_first: usize,
    pub(in super::super) cols: usize,
    /// 편집 칸을 포함한 가상 칸 수.
    pub(in super::super) count: usize,
    pub(in super::super) gap: f32,
}

/// 첫 줄 아래에서 화면에 걸친 `span` 줄을 그린다. 편집 칸이 그 밖의 줄에 있으면 계산한 자리에
/// 따로 그린다. 그리지 않으면 입력이 포커스를 잃는다.
pub(in super::super) fn grid_lines(
    ui: &mut egui::Ui,
    ctx: &EntryCtx<'_>,
    view: &mut ExplorerView,
    lines: GridLines,
    span: Range<usize>,
    action: &mut Option<ExplorerAction>,
) {
    let GridLines {
        at,
        in_first,
        cols,
        count,
        gap,
    } = lines;
    let pitch = gap + ctx.metrics.cell_h;
    let top = ui.cursor().top() - span.start as f32 * pitch;
    if let Some(a) = at.filter(|a| *a >= in_first)
        && !span.contains(&((a - in_first) / cols))
    {
        let (line, col) = ((a - in_first) / cols, (a - in_first) % cols);
        let min = egui::pos2(
            ui.max_rect().left() + col as f32 * (super::super::CELL_W.value() + gap),
            top + line as f32 * pitch + gap,
        );
        let rect = egui::Rect::from_min_size(min, egui::vec2(super::super::CELL_W.value(), pitch));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        name_row(&mut child, ctx.theme, view, Slot::Grid, action);
    }
    for line in span {
        let start = in_first + line * cols;
        ui.add_space(gap);
        ui.horizontal(|ui| {
            let run = start..(start + cols).min(count);
            grid_run(ui, ctx, view, at, run, None, action);
        });
    }
}

/// List 의 보이는 범위를 실제 항목 범위와 편집 줄 위치로 나눈다. `open_span` 이 범위 위 빈자리를
/// 띄운 뒤 부른다. 편집 줄은 표 행 높이라 목록 행보다 높으므로 위로 지나간 편집 줄의 차이만큼 더
/// 띄운다. 편집 줄이 범위 밖이면 계산한 자리에 따로 그려 입력 포커스를 지킨다.
pub(in super::super) fn list_span(
    ui: &mut egui::Ui,
    theme: &tasty_type_appearance::theme::Theme,
    view: &mut ExplorerView,
    (at, row_h): (Option<usize>, f32),
    span: &Range<usize>,
    action: &mut Option<ExplorerAction>,
) -> (Range<usize>, Option<usize>) {
    let (real, edit) = split_span(at, span.clone());
    if let Some(a) = at.filter(|_| edit.is_none()) {
        let top = ui.cursor().top() - span.start as f32 * row_h;
        let min = egui::pos2(ui.max_rect().left(), top + a as f32 * row_h);
        let size = egui::vec2(ui.available_width(), theme.table_cell_height().value());
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(min, size)));
        name_row(&mut child, theme, view, Slot::List, action);
    }
    if at.is_some_and(|a| a < span.start) {
        ui.add_space(theme.table_cell_height().value() - row_h);
    }
    (real, edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edit_slot_shifts_the_items_after_it() {
        // 편집 줄이 순번 3 에 끼면 가상 0..5 는 항목 0..4 와 3 번째 자리의 편집 줄이다.
        assert_eq!(split_span(Some(3), 0..5), (0..4, Some(3)));
        assert_eq!(split_span(Some(3), 3..6), (3..5, Some(0)));
        assert_eq!(split_span(Some(3), 4..8), (3..7, None));
        assert_eq!(split_span(Some(0), 0..2), (0..1, Some(0)));
        assert_eq!(split_span(None, 2..5), (2..5, None));
        assert_eq!(virtual_index(Some(3), 2), 2);
        assert_eq!(virtual_index(Some(3), 3), 4);
        assert_eq!(virtual_index(None, 3), 3);
    }

    #[test]
    fn the_layout_moves_past_a_leading_edit_slot_and_steps_aside_for_one_between_items() {
        let rows = ListLayout::rows(egui::Pos2::ZERO, 20.0, 5);
        let mut view = ExplorerView::new();
        view.list_layout = Some(rows);
        fit_layout(&mut view, None, |l| l.origin.y += 28.0);
        assert_eq!(view.list_layout, Some(rows));
        fit_layout(&mut view, Some(0), |l| l.origin.y += 28.0);
        assert_eq!(view.list_layout.map(|l| l.origin.y), Some(28.0));
        fit_layout(&mut view, Some(3), |l| l.origin.y += 28.0);
        assert_eq!(view.list_layout, None);
    }
}
