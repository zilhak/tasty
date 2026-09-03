//! 와이어 `Scroll` 의 단위 규약과 그 환산 — egui-mesh 로 가는 휠 입력의 단일 출처.
//!
//! `RawInputEventWire::Scroll` 은 **논리 포인트**를 나른다. plugin SDK 가 받은 값을
//! `MouseWheelUnit::Point` 인 `MouseWheel` 로 그대로 egui 에 넣기 때문이다
//! (`crates/tasty-plugin-sdk/src/egui_surface.rs` 의 `push_scroll_events` / `wheel_event`).
//! 그래서 host 쪽 수집 지점은 자기 입력 소스의 단위를 포인트로 맞춘 뒤 보내야 한다.
//!
//! 수집 지점은 둘이고 입력 소스의 표현이 다르다:
//!
//! - **egui-mesh surface** (`src/view/main/mouse.rs` 의 `handle_mouse_wheel`) — winit
//!   `MouseScrollDelta` 를 직접 받는다. `LineDelta` 는 줄 수, `PixelDelta` 는 물리 픽셀.
//! - **popup / banner** (`popup_render.rs` · `banner_render.rs`) — host egui 가 한 번
//!   가공한 `Event::MouseWheel { unit, delta }` 를 받는다. egui-winit 이 winit 의
//!   `LineDelta` 를 `Line`(줄 수 그대로), `PixelDelta` 를 `Point`(÷ pixels_per_point)
//!   로 옮긴 것이라, 두 소스는 같은 물리 입력의 다른 표현일 뿐이다.
//!
//! 두 경로가 다른 배율을 쓰면 **같은 휠 한 칸이 표면 종류에 따라 다른 거리를 스크롤**한다
//! (`docs/architecture/input-layer.md` 의 입력 계층 일관성). 그래서 줄 → 포인트 배율을
//! [`LINE_SCROLL`] 한 곳에 두고 위 세 지점(surface · popup · banner)이 모두 그것을 읽는다.
//!
//! **다만 이것이 프로세스 전체의 단일 출처는 아니다.** `modifier_hint_overlay` 의
//! `modifier_free_wheel_y` 가 같은 Point/Line/Page 변환을 수행하면서 `Line` 배율만
//! `ctx.options(|o| o.line_scroll_speed)`(native 40pt)에서 가져오고, host egui 위젯의
//! `ScrollArea` 전반도 같은 옵션을 쓴다. 그래서 지금은 plugin 표면이 노치당 50pt,
//! host egui 쪽이 40pt 로 갈려 있다. [`LINE_SCROLL`] 을 고쳐도 그쪽은 따라오지 않는다 —
//! 양쪽을 맞추려면 egui `Options::line_scroll_speed` 자체를 정해야 하는데, 그것은 설정
//! 모달·사이드바를 포함한 host UI 전체의 체감을 바꾸는 별도 결정이다.

use egui::{MouseWheelUnit, Vec2};
use tasty_type_geometry::length::LogicalPx;

/// 휠 1 notch(= `MouseWheelUnit::Line` 1 줄 = winit `LineDelta` 의 1.0)가 옮기는 논리 포인트.
///
/// 값이 50 인 근거는 egui 기본값과 견주어 고른 것이 아니라 **이 코드베이스가 원래 쓰던
/// 값을 보존**하는 것이다 — egui-mesh surface 경로가 winit `LineDelta` 에 곱하던 상수가
/// 50 이었고, 뒤늦게 합류한 popup·banner 를 거기에 맞추는 것이 목표였다.
///
/// egui 자신의 `Options::line_scroll_speed` 를 읽지 않고 상수로 박는 이유는 그와 별개다.
/// 이 값은 plugin 프로세스로 나가는 **와이어 값**이라 host 의 플랫폼 사정에 따라 달라지면
/// 안 되는데, 그 옵션은 native 40pt / web 8pt 로 갈린다.
///
/// 여기를 고치면 surface · popup · banner 세 지점이 함께 따라온다. host egui 위젯 쪽은
/// 따라오지 않는다 — 모듈 문서 참조.
pub(crate) const LINE_SCROLL: LogicalPx = LogicalPx(50.0);

/// host egui 의 `Event::MouseWheel` 델타를 와이어 `Scroll` 이 요구하는 논리 포인트로 환산한다.
///
/// `page` 는 `Page` 단위 1 페이지의 길이다. egui 자신(`InputState::begin_pass`)과
/// `modifier_hint_overlay::modifier_free_wheel_y` 가 모두 **화면 높이**를 쓰고 가로·세로
/// 양축에 같은 값을 곱하므로 여기서도 같은 규칙을 따른다. 다만 egui-winit 은 데스크톱에서
/// `Line` 과 `Point` 만 만들어 내므로(`LineDelta` / `PixelDelta` 두 갈래) 이 갈래는 실제로
/// 도달하지 않는다 — 단위를 빠짐없이 다루기 위한 것이다.
pub(crate) fn wheel_delta_to_points(
    unit: MouseWheelUnit,
    delta: Vec2,
    page: LogicalPx,
) -> (LogicalPx, LogicalPx) {
    // 단위 1.0 당 논리 포인트. `Point` 는 이미 포인트라 1:1.
    let per_unit = match unit {
        MouseWheelUnit::Point => LogicalPx(1.0),
        MouseWheelUnit::Line => LINE_SCROLL,
        MouseWheelUnit::Page => page,
    };
    (per_unit * delta.x, per_unit * delta.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 마우스 휠 1 notch 는 어느 경로로 들어오든 같은 거리를 옮겨야 한다.
    ///
    /// - surface 경로: winit `LineDelta(0, ±1)` × [`LINE_SCROLL`]
    /// - popup / banner 경로: egui `Line` 단위 델타 ±1 → 이 함수
    ///
    /// egui-winit 이 winit `LineDelta` 를 `Line` 단위로 값 그대로 옮기므로(줄 수 보존),
    /// 두 식의 입력은 같은 수이고 결과도 같아야 한다.
    #[test]
    fn one_wheel_notch_moves_the_same_distance_on_both_paths() {
        let surface_dy = LINE_SCROLL * 1.0;
        let (_, popup_dy) =
            wheel_delta_to_points(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), LogicalPx(800.0));
        assert_eq!(popup_dy, surface_dy);
        assert_eq!(popup_dy, LogicalPx(50.0));

        // 아래로 굴리는 방향(음수)도 부호까지 같다.
        let (_, popup_up) =
            wheel_delta_to_points(MouseWheelUnit::Line, Vec2::new(0.0, -1.0), LogicalPx(800.0));
        assert_eq!(popup_up, LINE_SCROLL * -1.0);
    }

    /// 트랙패드(winit `PixelDelta` → egui `Point`)는 이미 논리 포인트라 그대로 통과한다 —
    /// surface 경로의 `PixelDelta` ÷ pixels_per_point 와 같은 값이다(egui-winit 이 그
    /// 나눗셈을 이미 했다).
    #[test]
    fn point_unit_passes_through_unscaled() {
        let (dx, dy) = wheel_delta_to_points(
            MouseWheelUnit::Point,
            Vec2::new(3.5, -12.0),
            LogicalPx(800.0),
        );
        assert_eq!(dx, LogicalPx(3.5));
        assert_eq!(dy, LogicalPx(-12.0));
    }

    /// `Page` 는 양축 모두 화면 높이를 곱한다(egui `InputState::begin_pass` 와 같은 규칙).
    #[test]
    fn page_unit_scales_both_axes_by_the_page_length() {
        let (dx, dy) =
            wheel_delta_to_points(MouseWheelUnit::Page, Vec2::new(1.0, -2.0), LogicalPx(600.0));
        assert_eq!(dx, LogicalPx(600.0));
        assert_eq!(dy, LogicalPx(-1200.0));
    }

    /// 환산 전 동작(단위를 버리고 델타를 그대로 전달)과의 차이를 수치로 고정한다 —
    /// 물리 마우스 휠 1 notch 가 1pt 에서 [`LINE_SCROLL`] 로 바뀐다.
    #[test]
    fn line_unit_is_no_longer_delivered_as_one_point() {
        let raw_delta = Vec2::new(0.0, 1.0); // 환산 전에는 이 값이 그대로 와이어에 실렸다.
        let (_, converted) =
            wheel_delta_to_points(MouseWheelUnit::Line, raw_delta, LogicalPx(800.0));
        assert_eq!(LogicalPx(raw_delta.y), LogicalPx(1.0));
        assert_eq!(converted, LogicalPx(50.0));
    }
}
