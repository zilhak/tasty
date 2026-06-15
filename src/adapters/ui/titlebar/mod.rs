//! CSD 공통 titlebar 어댑터 — full-width 상단 바 + 드래그/더블클릭 → winit window 조작.
//!
//! view/wrapper 분리: 순수 [`view`] (props→actions) + 본 wrapper (props 추출 +
//! action → winit window 조작 브리지). OS별 컨트롤(신호등/캡션 버튼)은 P4~P6.

mod view;

use winit::window::Window;

use crate::theme;
use tasty_type_geometry::length::PhysicalPx;

pub use view::{TitlebarAction, TitlebarProps, draw_titlebar_view};

/// macOS 네이티브 신호등(standardWindowButton) 클러스터가 차지하는 좌측 폭
/// (logical points). fullsize-content-view 에서 신호등은 OS 가 좌상단 고정 위치에
/// 그리므로, egui titlebar 의 드래그 영역은 이 폭만큼 비운다(carve-out). 디자인
/// inset(padding 12 + 점 3×12 + gap 2×8) 기준 + 네이티브 클러스터 여유.
/// OS 가 고정하는 geometry 라 테마 토큰이 아니다.
#[cfg(target_os = "macos")]
const MACOS_TRAFFIC_LIGHT_INSET: tasty_type_geometry::length::LogicalPx =
    tasty_type_geometry::length::LogicalPx(78.0);

/// titlebar 가 차지하는 상단 inset (physical px) — `compute_terminal_rect` 의
/// `top_inset` 인자 + egui SidePanel 시작 오프셋의 단일 진실원.
///
/// P3 에서 titlebar 는 항상 그려지므로 항상 실제 높이를 반환한다.
pub fn top_inset(scale_factor: f32) -> PhysicalPx {
    theme::theme().titlebar_height.to_physical(scale_factor)
}

/// 공통 CSD titlebar 를 그리고, view 가 보고한 드래그/더블클릭을 winit window
/// 조작으로 브리지한다. `run_egui_frame` 의 egui 클로저 최상단에서 호출한다 —
/// `TopBottomPanel::top` 이 먼저 등록되어야 사이드바 `SidePanel` 이 그 아래에서
/// 시작한다.
pub fn draw_titlebar(ctx: &egui::Context, window: &Window) {
    let th = theme::theme();
    // macOS 만 네이티브 신호등 폭만큼 좌측 슬롯을 비운다. 그 외 OS 는 0(P5/P6 후속).
    #[cfg(target_os = "macos")]
    let left_inset = MACOS_TRAFFIC_LIGHT_INSET.value();
    #[cfg(not(target_os = "macos"))]
    let left_inset = 0.0;
    let props = TitlebarProps {
        theme: &th,
        active: window.has_focus(),
        height: th.titlebar_height.value(),
        left_inset,
    };

    for action in draw_titlebar_view(ctx, &props) {
        match action {
            TitlebarAction::StartDrag => {
                // 드래그 시작 시점(마우스 눌린 상태)에 호출해야 OS 가 윈도우 이동을
                // 받는다. 실패(예: 일부 플랫폼/상태)는 치명적이지 않으므로 로그만.
                if let Err(e) = window.drag_window() {
                    tracing::warn!("titlebar drag_window failed: {e}");
                }
            }
            TitlebarAction::ToggleMaximize => {
                window.set_maximized(!window.is_maximized());
            }
        }
    }
}
