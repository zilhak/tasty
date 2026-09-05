//! CSD(Client-Side Decorations) 윈도우 속성 — OS별 데코레이션 전략 적용.
//!
//! 원칙 1(사용자/에이전트 분리)·원칙 4(크로스플랫폼). macOS 는 fullsize-content-view
//! 패턴으로 **네이티브 신호등을 유지**하면서 콘텐츠를 타이틀바 영역(y=0)까지 확장한다.
//! `with_decorations(false)` 는 신호등까지 없애므로 (a) 결정에서 쓰지 않는다.
//! Linux 는 네이티브 데코를 끄고(`with_decorations(false)`) tasty 가 DE 가변 버튼을
//! CSD titlebar 에 직접 그린다(P6). Windows 는 `with_decorations(false)` 로 OS 캡션을
//! 제거하고 tasty 가 우측 캡션 버튼(min/max/restore/close)을 직접 그린다(P5).

use winit::window::{ResizeDirection, WindowAttributes};

/// CSD 창의 가장자리 리사이즈 핸들 두께 (physical px). 네이티브 데코가 없는
/// Linux 창에서 이 폭 안쪽을 가리키면 리사이즈 엣지로 친다.
// 이유: macOS 는 네이티브 데코라 호출부(mouse.rs)가 `#[cfg(not(target_os = "macos"))]`로
// 빠진다 — macOS 빌드에서는 dead_code 로 잡히므로 해당 타겟에서만 allow.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const RESIZE_EDGE_MARGIN: f64 = 8.0;

/// 커서가 창 가장자리 리사이즈 존에 있으면 해당 8방향 [`ResizeDirection`] 을 돌려준다.
/// 좌표·크기 모두 physical px. 모서리(코너)가 변보다 우선한다. 순수 함수라 OS 무관
/// 하게 컴파일·테스트된다. 데코 없는 Windows/Linux 창의 단일 MainView 리사이즈 경로가
/// 공유 호출한다(macOS 는 네이티브 데코라 호출하지 않음).
// 이유: 호출부가 데코 없는 창의 리사이즈 경로뿐이라 macOS 빌드엔 호출자가 없다(위).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn resize_direction_at(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    margin: f64,
) -> Option<ResizeDirection> {
    let left = x <= margin;
    let right = x >= width - margin;
    let top = y <= margin;
    let bottom = y >= height - margin;
    Some(match (top, bottom, left, right) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return None,
    })
}

/// 윈도우 생성부(첫 윈도우 + 추가 윈도우 공통)에서 호출해 OS별 CSD 속성을 적용한다.
///
/// - **macOS**: `titlebar_transparent` + `fullsize_content_view` + `title_hidden` 조합.
///   타이틀바를 투명화하고 콘텐츠를 y=0 까지 확장하되 OS 신호등(standardWindowButton:
///   close/min/zoom)은 그대로 둔다. 신호등의 클릭동작·hover글리프·풀스크린·접근성·
///   다크모드 디밍은 모두 OS 가 처리한다.
/// - **Linux**: `with_decorations(false)`. WM/컴포지터 데코를 끄고 tasty 가 CSD
///   titlebar(DE 가변 버튼)를 직접 그린다. Wayland 의 리사이즈 엣지는
///   `window.drag_resize_window` 로, 윈도우 이동은 `drag_window` 로 처리한다
///   (둘 다 winit 0.30 표준). 둥근 모서리/그림자 프레이밍은 윈도우 투명화 +
///   GPU 컴포지팅이 필요해 별도 후속.
/// - **Windows**: `with_decorations(false)` 로 OS 캡션/보더를 제거한다. tasty 가
///   우측 캡션 버튼을 직접 그리고(P5), 드래그/더블클릭 maximize 는 공통 어댑터가
///   처리한다. `with_undecorated_shadow(true)` 로 데코 제거 후에도 드롭 섀도를 복원해
///   창 경계가 보이게 한다. 가장자리 리사이즈는 Linux 와 동일한 단일 MainView 경로
///   (raw hit-test + `resize_direction_at` + `drag_resize_window`)로 처리한다 —
///   별도 egui 오버레이 레이어 없음(위젯 우선순위 입력모델: `egui_consumed` 은 패널/Area
///   전체의 bounding rect 단위라 리사이즈 게이트에 쓰지 않고, 타이틀바/상태바의 실제
///   인터랙티브 버튼 hover(`AppState.resize_edge_widget_hovered`)만 리사이즈보다
///   우선한다 — 빈 여백은 항상 리사이즈).
/// - **그 외 OS**: 변경 없음(네이티브 데코 유지).
pub fn apply_csd_attributes(attrs: WindowAttributes) -> WindowAttributes {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attrs
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
    }
    #[cfg(target_os = "linux")]
    {
        attrs.with_decorations(false)
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs.with_decorations(false).with_undecorated_shadow(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        attrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: f64 = 8.0;
    const W: f64 = 200.0;
    const H: f64 = 100.0;

    #[test]
    fn center_is_no_resize() {
        assert_eq!(resize_direction_at(100.0, 50.0, W, H, M), None);
    }

    #[test]
    fn edges_map_to_directions() {
        assert_eq!(
            resize_direction_at(0.0, 50.0, W, H, M),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            resize_direction_at(W, 50.0, W, H, M),
            Some(ResizeDirection::East)
        );
        assert_eq!(
            resize_direction_at(100.0, 0.0, W, H, M),
            Some(ResizeDirection::North)
        );
        assert_eq!(
            resize_direction_at(100.0, H, W, H, M),
            Some(ResizeDirection::South)
        );
    }

    #[test]
    fn corners_take_priority_over_edges() {
        assert_eq!(
            resize_direction_at(1.0, 1.0, W, H, M),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction_at(W - 1.0, 1.0, W, H, M),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            resize_direction_at(1.0, H - 1.0, W, H, M),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(
            resize_direction_at(W - 1.0, H - 1.0, W, H, M),
            Some(ResizeDirection::SouthEast)
        );
    }
}
