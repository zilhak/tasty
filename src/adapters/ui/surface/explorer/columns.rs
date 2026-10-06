//! 탐색기 아래 행을 고정 사각형 열로 나눈다.
//!
//! 사이드바는 즐겨찾기 최소 높이 때문에 낮은 칸에서 남은 높이보다 커질 수 있다. 행을
//! 자식 크기에 맞춰 늘리는 배치를 쓰면 그 높이가 내용 열과 상태줄을 칸 밖으로 밀어낸다.
//! 그래서 행을 남은 높이의 고정 사각형으로 잡고, 각 열을 자기 사각형에 가둬 넘치는 부분을
//! 잘라 낸다.

use tasty_type_geometry::length::LogicalPx;

/// 남은 공간 전체를 행으로 잡아 `left` 폭 열, `line` 폭 경계선, 나머지 열로 나눈다.
/// 두 열은 위에서 아래로 쌓는 자식 ui이며 자기 사각형 밖을 그리지 않는다.
pub(super) fn split(
    ui: &mut egui::Ui,
    left: LogicalPx,
    line: LogicalPx,
) -> (egui::Ui, egui::Rect, egui::Ui) {
    let (row, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let side = egui::Rect::from_min_size(row.min, egui::vec2(left.value(), row.height()));
    let edge = egui::Rect::from_min_size(
        egui::pos2(side.max.x, row.min.y),
        egui::vec2(line.value(), row.height()),
    );
    let body = egui::Rect::from_min_max(egui::pos2(edge.max.x, row.min.y), row.max);
    let column = |ui: &mut egui::Ui, rect: egui::Rect| {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child
    };
    (column(ui, side), edge, column(ui, body))
}
