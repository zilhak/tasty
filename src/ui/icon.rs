//! 아이콘 렌더링 모듈. painter 기반으로 폰트·플랫폼 무관하게 동일한 표현.
//!
//! 사용:
//! ```ignore
//! use crate::ui::icon::{Icon, Direction};
//! let resp = Icon::Chevron { direction: Direction::Right }.show(ui);
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
    /// 기본 크기/색(theme.subtext1)으로 렌더.
    pub fn show(&self, ui: &mut egui::Ui) -> egui::Response {
        let color = theme::theme().subtext1;
        self.show_sized(ui, DEFAULT_SIZE, color)
    }

    /// 크기/색을 명시해서 렌더.
    pub fn show_sized(&self, ui: &mut egui::Ui, size: f32, color: egui::Color32) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter_at(rect);
        match self {
            Icon::Chevron { direction } => draw_chevron(&painter, rect, *direction, color),
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
