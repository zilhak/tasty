//! 키보드로 움직이는 현재 항목. 선택과 따로 있으며, 키로 움직인 뒤에만 안쪽 1px 테두리로 보이고
//! 목록을 누르면 다음 키까지 숨는다(design `explorer-cursor-ring`).

use std::path::{Path, PathBuf};

use tasty_type_appearance::theme::Theme;

use super::ExplorerView;
use super::list_layout::ListLayout;

/// 이동 한 번. 키는 `KeybindingSettings::EXPLORER_LIST_BINDING_FIELDS` 가 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Default)]
pub(crate) struct CursorState {
    item: Option<PathBuf>,
    /// 키로 움직인 뒤 아직 목록을 누르지 않았다. 이때만 테두리를 그린다.
    shown: bool,
    /// 지난 프레임에 목록 안에 온전히 보인 항목. 그 항목으로 옮길 때는 스크롤하지 않는다.
    visible: Vec<PathBuf>,
    /// 다음 프레임에 보이도록 스크롤할 항목.
    scroll: Option<PathBuf>,
}

/// 이번 프레임에 그린 항목 하나와 그 칸의 화면 자리.
#[derive(Clone, Debug)]
pub(crate) struct Drawn {
    pub(crate) path: PathBuf,
    /// 잘리기 전 칸 전체.
    pub(crate) rect: egui::Rect,
    /// 목록에서 보이는 부분.
    pub(crate) shown: egui::Rect,
}

impl Drawn {
    /// 칸 전체가 보인다.
    fn whole(&self) -> bool {
        (self.rect.height() - self.shown.height()).abs() < 0.5
            && (self.rect.width() - self.shown.width()).abs() < 0.5
    }
}

impl CursorState {
    /// 키로 움직인 뒤 보이는 현재 항목.
    pub(crate) fn shown_item(&self) -> Option<&Path> {
        self.item.as_deref().filter(|_| self.shown)
    }

    /// 다음 프레임 스크롤 대상을 꺼낸다.
    pub(crate) fn take_scroll(&mut self) -> Option<PathBuf> {
        self.scroll.take()
    }

    /// 목록을 눌렀다. 다음 키는 클릭으로 정한 기준 항목에서 시작한다.
    pub(crate) fn hide(&mut self) {
        self.shown = false;
    }
}

impl ExplorerView {
    /// 현재 항목을 옮기고 선택을 맞춘다. `extend` 면 기준 항목부터 새 현재 항목까지 고르고,
    /// 아니면 새 현재 항목만 고른다. 움직일 곳이 없으면(빈 목록, Detail·List 의 왼쪽·오른쪽)
    /// 아무것도 바꾸지 않는다.
    pub(crate) fn move_cursor(&mut self, step: Step, extend: bool) {
        let count = self.shown_count();
        if count == 0 {
            return;
        }
        let layout = self.list_layout;
        let page = match (layout, self.list_rect) {
            (Some(l), Some(r)) => l.page(r.height()),
            _ => 1,
        };
        // 키로 움직이는 중이면 현재 항목에서, 아니면 클릭·타입어헤드가 정한 기준 항목에서 시작한다.
        let from_path = if self.cursor.shown {
            self.cursor.item.clone()
        } else {
            self.anchor.clone()
        };
        let from = from_path
            .as_deref()
            .and_then(|p| self.shown().position(|e| e.path == p));
        let Some(to) = target(step, from, count, layout.as_ref(), page) else {
            return;
        };
        let Some(entry) = self.shown_range(to..to + 1).pop() else {
            return;
        };
        let path = entry.path;
        if extend {
            if self.anchor.is_none() {
                self.anchor = from_path.or_else(|| Some(path.clone()));
            }
            self.click_select(&path, false, true);
        } else {
            self.select_only(&path);
        }
        if !self.cursor.visible.contains(&path) {
            self.cursor.scroll = Some(path.clone());
        }
        self.cursor.item = Some(path);
        self.cursor.shown = true;
    }
}

/// 옮겨 갈 목록 번호. `from` 이 없으면(현재 항목·기준 항목이 목록에 없음) End 는 마지막, 나머지는
/// 첫 항목이다. Grid 의 위·아래는 한 줄(`cols`)씩, 다음 줄에 같은 열 칸이 없으면 마지막 항목으로 간다.
fn target(
    step: Step,
    from: Option<usize>,
    count: usize,
    layout: Option<&ListLayout>,
    page: usize,
) -> Option<usize> {
    let last = count - 1;
    let Some(i) = from else {
        return Some(if step == Step::End { last } else { 0 });
    };
    let grid = layout.filter(|l| l.grid);
    let cols = grid.map_or(1, |l| l.cols);
    Some(match step {
        Step::Up if i >= cols => i - cols,
        Step::Up => i,
        Step::Down => match grid {
            Some(l) if l.line_of(i) == l.line_of(last) => i,
            _ => (i + cols).min(last),
        },
        Step::Left if grid.is_some() => i.saturating_sub(1),
        Step::Right if grid.is_some() => (i + 1).min(last),
        Step::Left | Step::Right => return None,
        Step::Home => 0,
        Step::End => last,
        Step::PageUp => i.saturating_sub(page),
        Step::PageDown => (i + page).min(last),
    })
}

/// 본문을 그린 뒤 한 번 부른다. 목록을 누르면 테두리를 숨기고, 온전히 보이는 항목을 기록하고, 현재
/// 항목이 화면에 있으면 그 칸 안쪽에 테두리를 그린다.
pub(crate) fn frame(ui: &egui::Ui, theme: &Theme, view: &mut ExplorerView, drawn: &[Drawn]) {
    let Some(list) = view.list_rect else {
        return;
    };
    let pressed = ui.input(|i| {
        i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| list.contains(p))
    });
    if pressed {
        view.cursor.hide();
    }
    view.cursor.visible = drawn
        .iter()
        .filter(|d| d.whole())
        .map(|d| d.path.clone())
        .collect();
    let Some(item) = view.cursor.shown_item() else {
        return;
    };
    let Some(cell) = drawn.iter().find(|d| d.path == item) else {
        return;
    };
    let width = theme.explorer_cursor_ring_width().value();
    let painter = ui
        .ctx()
        .layer_painter(ui.layer_id())
        .with_clip_rect(cell.shown);
    painter.rect_stroke(
        cell.rect,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(width, theme.explorer_cursor_ring().to_egui()),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cols: usize, lead: usize, count: usize) -> ListLayout {
        ListLayout::grid(
            egui::Pos2::ZERO,
            egui::vec2(10.0, 10.0),
            2.0,
            cols,
            lead,
            count,
        )
    }

    #[test]
    fn rows_move_one_item_a_page_or_to_either_end() {
        let rows = ListLayout::rows(egui::Pos2::ZERO, 20.0, 50);
        let go = |step, from| target(step, from, 50, Some(&rows), 10);
        assert_eq!(go(Step::Down, Some(3)), Some(4));
        assert_eq!(go(Step::Up, Some(0)), Some(0));
        assert_eq!(go(Step::Down, Some(49)), Some(49));
        assert_eq!(go(Step::PageDown, Some(45)), Some(49));
        assert_eq!(go(Step::PageUp, Some(15)), Some(5));
        assert_eq!(go(Step::Home, Some(30)), Some(0));
        assert_eq!(go(Step::End, Some(3)), Some(49));
        assert_eq!(go(Step::Left, Some(3)), None);
        assert_eq!(go(Step::Right, Some(3)), None);
        assert_eq!(go(Step::Down, None), Some(0));
        assert_eq!(go(Step::End, None), Some(49));
    }

    #[test]
    fn a_grid_moves_by_cells_and_lines() {
        // 4 열, `..` 칸 하나, 항목 10 개: 줄 0 = 항목 0..3, 줄 1 = 3..7, 줄 2 = 7..10.
        let l = grid(4, 1, 10);
        let go = |step, from| target(step, from, 10, Some(&l), 8);
        assert_eq!(go(Step::Right, Some(2)), Some(3));
        assert_eq!(go(Step::Left, Some(0)), Some(0));
        assert_eq!(go(Step::Down, Some(1)), Some(5));
        assert_eq!(go(Step::Up, Some(5)), Some(1));
        assert_eq!(go(Step::Up, Some(2)), Some(2));
        // 아래 줄에 같은 열 칸이 없으면 마지막 항목으로 간다.
        assert_eq!(go(Step::Down, Some(6)), Some(9));
        // 마지막 줄에서는 움직이지 않는다.
        assert_eq!(go(Step::Down, Some(8)), Some(8));
    }
}
