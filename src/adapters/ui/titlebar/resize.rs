#![cfg(target_os = "windows")]
//! Windows CSD 리사이즈 보더 (egui 오버레이).
//!
//! `with_decorations(false)` 는 OS 비-클라이언트 리사이즈 보더를 없애므로 tasty 가
//! 윈도우 둘레에 얇은 egui 인터랙티브 스트립(4 에지 + 4 코너)을 최상위 레이어로 깔고,
//! 드래그 개시 시 winit `drag_resize_window` 로 OS 리사이즈 루프를 띄운다. 모든 egui
//! 패널(titlebar / sidebar / status bar) 위 레이어라 어느 패널 가장자리든 잡힌다.

use winit::window::{ResizeDirection, Window};

/// 에지 스트립 두께 / 코너 정사각 한 변 (logical points = egui 좌표).
const EDGE: f32 = 6.0;
const CORNER: f32 = 12.0;

/// 윈도우 둘레에 리사이즈 보더를 깐다. `run_egui_frame` 의 가장 마지막에 호출해
/// 다른 모든 레이어 위에 둔다. 최대화 상태에서는 깔지 않는다.
pub fn draw_resize_borders(ctx: &egui::Context, window: &Window) {
    if window.is_maximized() {
        return;
    }
    let r = ctx.screen_rect();
    let span_w = r.width() - 2.0 * CORNER;
    let span_h = r.height() - 2.0 * CORNER;

    // 코너 먼저(겹치는 부분에서 코너 방향이 우선되도록 나중에 그리는 에지보다 위).
    let zones = [
        (
            egui::Rect::from_min_size(r.left_top(), egui::vec2(CORNER, CORNER)),
            ResizeDirection::NorthWest,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.right() - CORNER, r.top()),
                egui::vec2(CORNER, CORNER),
            ),
            ResizeDirection::NorthEast,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.left(), r.bottom() - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            ResizeDirection::SouthWest,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.right() - CORNER, r.bottom() - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            ResizeDirection::SouthEast,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.left() + CORNER, r.top()),
                egui::vec2(span_w, EDGE),
            ),
            ResizeDirection::North,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.left() + CORNER, r.bottom() - EDGE),
                egui::vec2(span_w, EDGE),
            ),
            ResizeDirection::South,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.left(), r.top() + CORNER),
                egui::vec2(EDGE, span_h),
            ),
            ResizeDirection::West,
        ),
        (
            egui::Rect::from_min_size(
                egui::pos2(r.right() - EDGE, r.top() + CORNER),
                egui::vec2(EDGE, span_h),
            ),
            ResizeDirection::East,
        ),
    ];

    for (i, (zone, dir)) in zones.into_iter().enumerate() {
        // 작은 창에서 span 이 음수면 해당 에지 스트립은 건너뛴다.
        if zone.width() <= 0.0 || zone.height() <= 0.0 {
            continue;
        }
        let resp = egui::Area::new(egui::Id::new(("tasty_resize_border", i)))
            .fixed_pos(zone.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.allocate_rect(zone, egui::Sense::click_and_drag())
            })
            .inner;
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(resize_cursor(dir));
        }
        if resp.drag_started()
            && let Err(e) = window.drag_resize_window(dir)
        {
            tracing::warn!("titlebar resize drag failed: {e}");
        }
    }
}

fn resize_cursor(dir: ResizeDirection) -> egui::CursorIcon {
    use ResizeDirection as D;
    match dir {
        D::North => egui::CursorIcon::ResizeNorth,
        D::South => egui::CursorIcon::ResizeSouth,
        D::East => egui::CursorIcon::ResizeEast,
        D::West => egui::CursorIcon::ResizeWest,
        D::NorthEast => egui::CursorIcon::ResizeNorthEast,
        D::NorthWest => egui::CursorIcon::ResizeNorthWest,
        D::SouthEast => egui::CursorIcon::ResizeSouthEast,
        D::SouthWest => egui::CursorIcon::ResizeSouthWest,
    }
}
