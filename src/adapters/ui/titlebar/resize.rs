#![cfg(target_os = "windows")]
//! Windows CSD 리사이즈 보더 (egui 오버레이).
//!
//! `with_decorations(false)` 는 OS 비-클라이언트 리사이즈 보더를 없애므로 tasty 가
//! 윈도우 둘레에 얇은 egui 인터랙티브 스트립(에지 + 코너)을 최상위 레이어로 깔고,
//! 드래그 개시 시 winit `drag_resize_window` 로 OS 리사이즈 루프를 띄운다. 모든 egui
//! 패널(titlebar / sidebar / status bar) 위 레이어라 어느 패널 가장자리든 잡힌다.
//!
//! **캡션 클러스터 carve-out**: 우상단 캡션 버튼(min/max/close)은 타이틀바 우측을
//! `cluster_width × titlebar_height` 만큼 차지한다. 리사이즈 스트립도 같은
//! `Order::Foreground` 라 늦게 그려지는 리사이즈가 위에 깔려 캡션 클릭을 가로챌 수
//! 있다. 그래서 caption rect 와 겹치는 리사이즈 zone(NorthEast 코너, North 에지의
//! 우측 끝, East 에지의 상단)을 캡션 영역만큼 잘라낸다 — Win11 도 캡션 버튼 위는
//! 리사이즈존이 아니다(HTCLOSE/HTMAXBUTTON > HTTOP).

use winit::window::{ResizeDirection, Window};

/// 에지 스트립 두께 / 코너 정사각 한 변 (logical points = egui 좌표).
const EDGE: f32 = 6.0;
const CORNER: f32 = 12.0;

/// 윈도우 둘레에 리사이즈 보더를 깐다. `run_egui_frame` 의 가장 마지막에 호출해
/// 다른 모든 레이어 위에 둔다. 최대화 상태에서는 깔지 않는다.
/// `sidebar_inset` = 좌측 사이드바가 차지하는 폭(logical px, 없으면 0). 사이드바는
/// 좌변 전체를 인터랙티브하게 덮으므로, 그 위에 리사이즈 존을 깔면 사이드바 버튼
/// 클릭이 리사이즈로 흡수된다(좌하단 도구/플러그인/설정 버튼이 먹히는 버그). 따라서
/// 좌측 존(West / NorthWest / SouthWest)과 North/South 의 좌측부를 사이드바 폭만큼
/// 잘라낸다(우상단 캡션 carve-out 과 동일 사상).
pub fn draw_resize_borders(ctx: &egui::Context, window: &Window, sidebar_inset: f32) {
    if window.is_maximized() {
        return;
    }
    let r = ctx.screen_rect();
    let span_h = r.height() - 2.0 * CORNER;
    // 좌측 존이 시작할 수 있는 x(사이드바 우측). 사이드바가 EDGE 보다 넓으면 좌변 존은
    // 전부 사이드바 아래라 생략한다.
    let inset = sidebar_inset.max(0.0);
    let left_bound = r.left() + inset;
    let sidebar_covers_left = inset >= EDGE;

    // 우상단 캡션 클러스터가 차지하는 영역. 이 rect 와 겹치는 리사이즈 zone 은
    // 캡션 클릭을 가리지 않도록 잘라낸다(carve-out).
    let th = crate::theme::theme();
    let caption_left = r.right() - super::caption::cluster_width(&th);
    let caption_bottom = r.top() + th.titlebar_height.value();

    // 코너 먼저(겹치는 부분에서 코너 방향이 우선되도록 나중에 그리는 에지보다 위).
    // NorthEast 코너는 캡션 클러스터(close 버튼 우상단)와 정면 충돌하므로 제외한다 —
    // 우상단 리사이즈는 East 에지 하단부로 대신한다.
    // North/South 의 좌측 시작점은 코너 끝과 사이드바 우측 중 더 오른쪽.
    let hpad_left = (r.left() + CORNER).max(left_bound);
    let mut zones: Vec<(egui::Rect, ResizeDirection)> = Vec::new();

    // 좌측 존(NW/SW 코너 + West)은 사이드바가 덮지 않을 때만.
    if !sidebar_covers_left {
        zones.push((
            egui::Rect::from_min_size(r.left_top(), egui::vec2(CORNER, CORNER)),
            ResizeDirection::NorthWest,
        ));
        zones.push((
            egui::Rect::from_min_size(
                egui::pos2(r.left(), r.bottom() - CORNER),
                egui::vec2(CORNER, CORNER),
            ),
            ResizeDirection::SouthWest,
        ));
        zones.push((
            egui::Rect::from_min_size(
                egui::pos2(r.left(), r.top() + CORNER),
                egui::vec2(EDGE, span_h),
            ),
            ResizeDirection::West,
        ));
    }

    // 우하단 코너 — 항상.
    zones.push((
        egui::Rect::from_min_size(
            egui::pos2(r.right() - CORNER, r.bottom() - CORNER),
            egui::vec2(CORNER, CORNER),
        ),
        ResizeDirection::SouthEast,
    ));
    // North 에지: 사이드바 우측(또는 좌측 코너 끝) ~ 캡션 클러스터 좌단.
    zones.push((
        egui::Rect::from_min_max(
            egui::pos2(hpad_left, r.top()),
            egui::pos2(caption_left, r.top() + EDGE),
        ),
        ResizeDirection::North,
    ));
    // South 에지: 사이드바 우측(또는 좌측 코너 끝) ~ 우하단 코너 위.
    zones.push((
        egui::Rect::from_min_max(
            egui::pos2(hpad_left, r.bottom() - EDGE),
            egui::pos2(r.right() - CORNER, r.bottom()),
        ),
        ResizeDirection::South,
    ));
    // East 에지: 캡션 클러스터 하단 ~ 우하단 코너 위(상단은 캡션 영역).
    zones.push((
        egui::Rect::from_min_max(
            egui::pos2(r.right() - EDGE, caption_bottom),
            egui::pos2(r.right(), r.bottom() - CORNER),
        ),
        ResizeDirection::East,
    ));

    for (i, (zone, dir)) in zones.into_iter().enumerate() {
        // 작은 창에서 span/carve-out 결과가 음수면 해당 스트립은 건너뛴다.
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
