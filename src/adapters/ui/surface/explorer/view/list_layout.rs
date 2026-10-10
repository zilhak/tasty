//! 이번 프레임에 그린 목록의 칸 배치. 세 보기 모두 모드 안에서 칸 크기와 간격이 같아, 목록 번호와
//! 이 배치만으로 화면 밖 항목의 자리까지 계산한다. 키보드 이동의 한 화면 크기와 영역 선택이 쓴다.

use std::ops::Range;

use super::ExplorerView;

impl ExplorerView {
    /// Grid 의 첫 줄을 그리기 직전에 부른다. `cell` 은 칸 너비·높이, `(cols, lead)` 는 열 수와 첫 줄
    /// 앞의 `..`·이름 입력 칸 수다.
    pub(crate) fn note_grid(
        &mut self,
        ui: &egui::Ui,
        cell: (f32, f32),
        gap: f32,
        cols: (usize, usize),
    ) {
        let (cols, lead) = cols;
        let cell = egui::vec2(cell.0, cell.1);
        let count = self.shown_count();
        self.list_layout = Some(ListLayout::grid(
            ui.cursor().min,
            cell,
            gap,
            cols,
            lead,
            count,
        ));
    }

    /// List 의 첫 항목 줄을 그리기 직전에 부른다.
    pub(crate) fn note_rows(&mut self, ui: &egui::Ui, row_h: f32) {
        let count = self.shown_count();
        self.list_layout = Some(ListLayout::rows(ui.cursor().min, row_h, count));
    }

    /// Detail 표를 그린 뒤 부른다. `drawn` 은 처음 그린 이름 칸의 표 행 번호와 자리, `lead` 는 항목
    /// 앞의 `..`·이름 입력 행 수다. 그린 행이 없으면 배치를 두지 않는다.
    pub(crate) fn note_detail(
        &mut self,
        drawn: Option<(usize, egui::Rect)>,
        lead: usize,
        row_h: f32,
    ) {
        self.list_layout = drawn.map(|(row, cell)| {
            let top = cell.top() + (lead as f32 - row as f32) * row_h;
            ListLayout::rows(egui::pos2(cell.left(), top), row_h, self.shown_count())
        });
    }
}

/// 목록 번호 `i` 의 칸은 첫 줄 앞의 `lead` 칸 다음에 놓인다. 위치 `i + lead` 의 줄은 `/ cols`,
/// 열은 `% cols` 다. Detail·List 는 한 줄에 한 칸이고 줄이 본문 폭 전체라 가로 위치를 보지 않는다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ListLayout {
    /// 위치 0 칸의 왼쪽 위(화면 좌표). 스크롤하면 함께 움직인다.
    pub(crate) origin: egui::Pos2,
    /// 다음 열·다음 줄 칸까지의 거리.
    pub(crate) step: egui::Vec2,
    /// 칸 하나의 크기. 간격은 `step - cell` 이다.
    pub(crate) cell: egui::Vec2,
    pub(crate) cols: usize,
    pub(crate) lead: usize,
    pub(crate) count: usize,
    pub(crate) grid: bool,
}

impl ListLayout {
    /// Detail·List: 항목 0 의 위쪽 끝과 줄 높이.
    pub(crate) fn rows(top: egui::Pos2, row_h: f32, count: usize) -> Self {
        Self {
            origin: top,
            step: egui::vec2(0.0, row_h),
            cell: egui::vec2(f32::INFINITY, row_h),
            cols: 1,
            lead: 0,
            count,
            grid: false,
        }
    }

    /// Grid: 첫 줄 첫 칸의 왼쪽 위, 칸 크기, 칸 사이 간격.
    pub(crate) fn grid(
        origin: egui::Pos2,
        cell: egui::Vec2,
        gap: f32,
        cols: usize,
        lead: usize,
        count: usize,
    ) -> Self {
        Self {
            origin,
            step: cell + egui::vec2(gap, gap),
            cell,
            cols: cols.max(1),
            lead,
            count,
            grid: true,
        }
    }

    /// 한 화면에 들어가는 항목 수. 보이는 높이에 온전히 들어가는 줄 수 × 열 수이며 1 이상이다.
    pub(crate) fn page(&self, visible_h: f32) -> usize {
        let lines = if self.step.y > 0.0 {
            (visible_h / self.step.y).floor().max(1.0) as usize
        } else {
            1
        };
        lines * self.cols
    }

    /// 목록 번호의 줄.
    pub(crate) fn line_of(&self, index: usize) -> usize {
        (index + self.lead) / self.cols
    }

    /// 위치 0 기준 좌표(화면 좌표 − `origin`)의 사각형에 걸치는 항목 번호. Grid 는 줄마다 끊긴
    /// 범위가 여럿이고, Detail·List 는 하나다. 칸 사이 간격에만 걸치면 고르지 않는다.
    pub(crate) fn hits(&self, area: egui::Rect) -> Vec<Range<usize>> {
        if self.count == 0 || self.step.y <= 0.0 {
            return Vec::new();
        }
        let Some(lines) = span(area.y_range(), self.step.y, self.cell.y) else {
            return Vec::new();
        };
        let cols = if self.grid {
            match span(area.x_range(), self.step.x, self.cell.x) {
                Some(c) => c.start..c.end.min(self.cols),
                None => return Vec::new(),
            }
        } else {
            0..1
        };
        if cols.is_empty() {
            return Vec::new();
        }
        let last_line = self.line_of(self.count - 1);
        let mut out: Vec<Range<usize>> = Vec::new();
        for line in lines.start..lines.end.min(last_line + 1) {
            let (from, to) = (line * self.cols + cols.start, line * self.cols + cols.end);
            if to <= self.lead {
                continue;
            }
            let start = from.max(self.lead) - self.lead;
            let end = (to - self.lead).min(self.count);
            if start >= end {
                continue;
            }
            match out.last_mut() {
                Some(prev) if prev.end == start => prev.end = end,
                _ => out.push(start..end),
            }
        }
        out
    }
}

/// 간격 `step`, 크기 `size` 로 0 부터 놓인 칸 중 `range` 와 겹치는 칸 번호. 겹치는 칸이 없으면 None.
fn span(range: egui::Rangef, step: f32, size: f32) -> Option<Range<usize>> {
    if step <= 0.0 {
        return (range.max > 0.0 && range.min < size).then_some(0..1);
    }
    // 칸 k 는 [k·step, k·step + size) 를 차지한다. 사각형이 칸에 걸치려면 k·step < max 이고
    // k·step + size > min 이어야 한다.
    let first = ((range.min - size) / step).floor() as i64 + 1;
    let last = (range.max / step).ceil() as i64 - 1;
    let first = first.max(0);
    if last < first {
        return None;
    }
    Some(first as usize..last as usize + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    }

    #[test]
    fn a_row_list_selects_every_row_the_rectangle_crosses() {
        let l = ListLayout::rows(egui::Pos2::ZERO, 20.0, 100);
        assert_eq!(l.hits(rect(0.0, 30.0, 5.0, 61.0)), vec![1..4]);
        // 줄 경계에 딱 닿기만 하면 그 줄은 고르지 않는다.
        assert_eq!(l.hits(rect(0.0, 20.0, 5.0, 40.0)), vec![1..2]);
        // 목록 위·아래 밖은 잘린다.
        assert_eq!(l.hits(rect(0.0, -50.0, 5.0, 10.0)), vec![0..1]);
        assert_eq!(l.hits(rect(0.0, 1990.0, 5.0, 9000.0)), vec![99..100]);
        assert!(l.hits(rect(0.0, 2001.0, 5.0, 9000.0)).is_empty());
    }

    #[test]
    fn a_grid_selects_the_block_of_cells_and_skips_the_lead_cells() {
        // 4 열, 첫 줄 앞에 `..` 칸 하나. 칸 10×10, 간격 2.
        let l = ListLayout::grid(egui::Pos2::ZERO, egui::vec2(10.0, 10.0), 2.0, 4, 1, 10);
        // 둘째·셋째 열(x 12..34), 첫 두 줄: 위치 1,2 와 5,6 → 항목 0,1 과 4,5.
        assert_eq!(l.hits(rect(13.0, 1.0, 25.0, 13.0)), vec![0..2, 4..6]);
        // 첫 열 첫 줄은 `..` 칸이라 항목이 없다.
        assert_eq!(l.hits(rect(1.0, 1.0, 5.0, 5.0)), Vec::<Range<usize>>::new());
        // 열 사이 간격에만 걸치면 아무것도 고르지 않는다.
        assert!(l.hits(rect(10.5, 0.0, 11.5, 40.0)).is_empty());
        // 마지막 줄은 항목 수에서 끊긴다(위치 8..11 중 항목 7..9).
        assert_eq!(l.hits(rect(0.0, 25.0, 60.0, 30.0)), vec![7..10]);
        // 여러 줄 전체 폭이면 이어진 범위 하나로 합친다.
        assert_eq!(l.hits(rect(0.0, 0.0, 60.0, 20.0)), vec![0..7]);
    }

    #[test]
    fn a_page_is_the_whole_lines_that_fit() {
        let rows = ListLayout::rows(egui::Pos2::ZERO, 20.0, 100);
        assert_eq!(rows.page(205.0), 10);
        assert_eq!(rows.page(5.0), 1);
        let grid = ListLayout::grid(egui::Pos2::ZERO, egui::vec2(10.0, 10.0), 2.0, 4, 0, 100);
        assert_eq!(grid.page(50.0), 16);
    }
}
