//! 아이콘 렌더링 모듈. painter 기반으로 폰트·플랫폼 무관하게 동일한 표현.
//!
//! 사용:
//! ```ignore
//! use crate::ui::icon::{Icon, Direction};
//! let row_h = font_size + ui.spacing().button_padding.y * 2.0;
//! let resp = Icon::Chevron { direction: Direction::Right }.show_in_row(ui, row_h);
//! if resp.clicked() { /* ... */ }
//! ```

use crate::theme;

/// 아이콘의 기본 크기(히트박스 한 변, px).
pub const DEFAULT_SIZE: f32 = 12.0;
const STROKE_WIDTH: f32 = 1.0;

/// 쉐브론·기타 아이콘의 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Direction {
    Right,
    Down,
    Left,
    Up,
}

/// 선언적 아이콘 타입. 새 종류를 더 넣을 때 variant를 추가한다.
#[derive(Debug, Clone, Copy)]
pub enum Icon {
    /// `>` / `v` / `<` / `^` 형태의 꺾쇠.
    Chevron { direction: Direction },
}

impl Icon {
    /// 행(line) 안에 끼워 넣을 때 사용. 가로는 `DEFAULT_SIZE`(아이콘 폭)를 쓰지만
    /// 세로는 `row_h`를 통째로 점유해 같은 행에 있는 다른 위젯(예: selectable_label)과
    /// 행 높이를 맞춘다. 아이콘 자체는 그 박스의 세로 중앙에 `DEFAULT_SIZE × DEFAULT_SIZE`
    /// 사이즈로 그려진다. egui의 `Layout::left_to_right(Align::Center)`는 `allocate_exact_size`로
    /// 먼저 잡힌 작은 rect를 사후에 재정렬하지 않기 때문에, 행 내에서 작은 아이콘이
    /// 위로 치우쳐 보이는 문제를 막는다.
    pub fn show_in_row(&self, ui: &mut egui::Ui, row_h: f32) -> egui::Response {
        let color = theme::theme().subtext1;
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(DEFAULT_SIZE, row_h), egui::Sense::click());
        let icon_rect = egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(DEFAULT_SIZE, DEFAULT_SIZE),
        );
        let painter = ui.painter_at(rect);
        match self {
            Icon::Chevron { direction } => draw_chevron(&painter, icon_rect, *direction, color),
        }
        resp
    }
}

fn draw_chevron(painter: &egui::Painter, rect: egui::Rect, dir: Direction, color: egui::Color32) {
    // 기준 쉐브론(Right): rect 내부에 `>`. 높이의 상·하 끝에서 중앙 우측 꼭짓점으로
    // 꺾이는 두 선분. 여백 비율은 시각 밸런스용.
    let pad_x = rect.width() * 0.30;
    let pad_y = rect.height() * 0.25;
    let top = egui::pos2(rect.min.x + pad_x, rect.min.y + pad_y);
    let mid = egui::pos2(rect.max.x - pad_x, rect.center().y);
    let bot = egui::pos2(rect.min.x + pad_x, rect.max.y - pad_y);

    // 방향에 맞춰 세 점을 회전.
    let center = rect.center();
    let rotate = |p: egui::Pos2| -> egui::Pos2 {
        let v = p - center;
        let (rx, ry) = match dir {
            Direction::Right => (v.x, v.y),
            Direction::Down => (-v.y, v.x),
            Direction::Left => (-v.x, -v.y),
            Direction::Up => (v.y, -v.x),
        };
        center + egui::vec2(rx, ry)
    };
    let stroke = egui::Stroke::new(STROKE_WIDTH, color);
    painter.line_segment([rotate(top), rotate(mid)], stroke);
    painter.line_segment([rotate(mid), rotate(bot)], stroke);
}
