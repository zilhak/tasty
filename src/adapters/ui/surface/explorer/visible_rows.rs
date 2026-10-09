//! 목록을 화면에 걸친 줄만 그리는 데 쓰는 계산. 세 보기 모드 모두 모드 안에서 줄 높이가 같아
//! 줄 번호와 높이만으로 화면 범위와 스크롤 위치를 구한다.

use tasty_type_appearance::theme::Theme;

use super::view::ExplorerView;
use crate::settings::EffectiveFont;

/// 위에서부터 `pitch` 간격으로 놓일 `count` 줄 가운데 지금 화면에 걸치는 줄 범위.
/// 화면 밖 줄은 그리지 않고 같은 높이의 빈자리로 둔다. 기준은 지금 커서 위치다.
pub(super) fn visible_span(ui: &egui::Ui, pitch: f32, count: usize) -> std::ops::Range<usize> {
    if pitch <= 0.0 || count == 0 {
        return 0..0;
    }
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let first = ((clip.top() - top) / pitch).floor().max(0.0) as usize;
    let last = ((clip.bottom() - top) / pitch).ceil().max(0.0) as usize + 1;
    first.min(count)..last.min(count)
}

/// 화면 밖 줄로 스크롤한다. `line` 은 지금 커서에서 센 줄 번호다.
pub(super) fn scroll_to_line(ui: &egui::Ui, pitch: f32, line: usize, height: f32) {
    let top = ui.cursor().top() + line as f32 * pitch;
    let rect = egui::Rect::from_min_size(
        egui::pos2(ui.max_rect().left(), top),
        egui::vec2(ui.max_rect().width(), height),
    );
    ui.scroll_to_rect(rect, Some(egui::Align::Center));
}

/// 이 프레임에 스크롤로 보여 줄 항목의 목록 번호.
pub(super) fn scroll_target(view: &ExplorerView) -> Option<usize> {
    let target = view.scroll_to.as_deref()?;
    view.shown().position(|e| e.path == target)
}

/// Grid 칸 치수. 칸 높이는 썸네일 자리와 세 줄 이름 자리로 정해져 모든 칸이 같다.
#[derive(Clone, Copy)]
pub(super) struct GridMetrics {
    pub(super) slot: f32,
    pub(super) label_font: f32,
    pub(super) label_line_h: f32,
    pub(super) cell_h: f32,
}

pub(super) fn grid_metrics(theme: &Theme, font: &EffectiveFont) -> GridMetrics {
    let slot = theme.explorer_grid_thumb_size().value();
    let label_font = font.font_size.max(1.0).min(theme.font_size_caption.value());
    let label_line_h = (label_font * 1.3).round();
    // 고정 3줄 예약 — 짧은 이름도 3줄분 높이를 잡아 그리드 행 정렬을 균일하게 유지.
    let label_h = label_line_h * 3.0;
    let cell_h = theme.spacing_sm.value()
        + slot
        + theme.spacing_xs.value()
        + label_h
        + theme.spacing_sm.value();
    GridMetrics {
        slot,
        label_font,
        label_line_h,
        cell_h,
    }
}
