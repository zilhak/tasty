//! CSD 공통 titlebar 어댑터 — full-width 상단 바 + 드래그/더블클릭 → winit window 조작.
//!
//! view/wrapper 분리: 순수 [`view`] (props→actions) + 본 wrapper (props 추출 +
//! action → winit window 조작 브리지). OS별 컨트롤(신호등/캡션 버튼)은 P4~P6.

mod caption;
mod resize;
mod view;

use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::AppEvent;
use crate::theme;
use tasty_type_geometry::length::PhysicalPx;

pub use view::{TitlebarAction, TitlebarControls, TitlebarProps, draw_titlebar_view};

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

/// Linux CSD 의 DE 가변 버튼 프리셋을 반환한다.
///
/// 현재는 단일 기본 프리셋(우측 min·max·close, KDE-Breeze 류)으로 시작한다.
/// DE 감지(GNOME=close만/우측 등) 및 사용자 설정 노출은 후속. macOS/Windows 는
/// `None` — macOS 는 네이티브 신호등, Windows 캡션은 전용 caption.rs 로 그린다(P5).
#[cfg(target_os = "linux")]
fn os_controls() -> Option<TitlebarControls> {
    use view::{ControlSide, WindowButton};
    Some(TitlebarControls {
        buttons: vec![
            WindowButton::Minimize,
            WindowButton::Maximize,
            WindowButton::Close,
        ],
        side: ControlSide::Right,
    })
}

#[cfg(not(target_os = "linux"))]
fn os_controls() -> Option<TitlebarControls> {
    None
}

/// 공통 CSD titlebar 를 그리고, view 가 보고한 드래그/더블클릭/버튼 클릭을 winit
/// window 조작 또는 app 이벤트로 브리지한다. `run_egui_frame` 의 egui 클로저
/// 최상단에서 호출한다 — `TopBottomPanel::top` 이 먼저 등록되어야 사이드바
/// `SidePanel` 이 그 아래에서 시작한다.
///
/// Windows 캡션 close 버튼도 `TitlebarAction::Close` 를 보고해 macOS/Linux 와 동일한
/// proxy(`AppEvent::CloseWindow`) 경로로 라우팅된다.
pub fn draw_titlebar(ctx: &egui::Context, window: &Window, proxy: &EventLoopProxy<AppEvent>) {
    let th = theme::theme();
    // macOS 만 네이티브 신호등 폭만큼 좌측 슬롯을 비운다. 그 외 OS 는 0.
    #[cfg(target_os = "macos")]
    let left_inset = MACOS_TRAFFIC_LIGHT_INSET.value();
    #[cfg(not(target_os = "macos"))]
    let left_inset = 0.0;
    let props = TitlebarProps {
        theme: &th,
        active: window.has_focus(),
        height: th.titlebar_height.value(),
        left_inset,
        controls: os_controls(),
        maximized: window.is_maximized(),
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
            TitlebarAction::Minimize => {
                window.set_minimized(true);
            }
            TitlebarAction::Close => {
                // 네이티브 CloseRequested 와 동일 라이프사이클로 라우팅(원칙 1: 사용자
                // 클릭 → app 이벤트, IPC 비노출). App::user_event 가 per-window 처리.
                crate::shortcuts::send_app_event(proxy, AppEvent::CloseWindow(window.id()));
            }
        }
    }
}

/// Windows CSD 리사이즈 보더를 윈도우 둘레에 깐다(데코 off 로 OS 보더가 사라지므로
/// tasty 가 직접 처리). `run_egui_frame` 의 가장 마지막에 호출해 모든 패널 위 레이어에
/// 둔다. Windows 외 OS 에서는 no-op.
/// `sidebar_inset` = 좌측 사이드바 폭(logical px, 없으면 0). 리사이즈 보더가 사이드바
/// 버튼 클릭을 가로채지 않도록 좌측 존을 그만큼 잘라낸다.
#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
pub fn draw_resize_borders(ctx: &egui::Context, window: &Window, sidebar_inset: f32) {
    #[cfg(target_os = "windows")]
    resize::draw_resize_borders(ctx, window, sidebar_inset);
}
