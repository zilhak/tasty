//! CSD(Client-Side Decorations) 윈도우 속성 — OS별 데코레이션 전략 적용.
//!
//! 원칙 1(사용자/에이전트 분리)·원칙 4(크로스플랫폼). macOS 는 fullsize-content-view
//! 패턴으로 **네이티브 신호등을 유지**하면서 콘텐츠를 타이틀바 영역(y=0)까지 확장한다.
//! `with_decorations(false)` 는 신호등까지 없애므로 (a) 결정에서 쓰지 않는다.
//! Linux 는 네이티브 데코를 끄고(`with_decorations(false)`) tasty 가 DE 가변 버튼을
//! CSD titlebar 에 직접 그린다(P6). Windows 의 캡션 버튼/Snap 은 P5 후속이라 no-op.

use winit::window::{ResizeDirection, WindowAttributes};

/// CSD 창의 가장자리 리사이즈 핸들 두께 (physical px). 네이티브 데코가 없는
/// Linux 창에서 이 폭 안쪽을 가리키면 리사이즈 엣지로 친다.
pub const RESIZE_EDGE_MARGIN: f64 = 8.0;

/// 커서가 창 가장자리 리사이즈 존에 있으면 해당 8방향 [`ResizeDirection`] 을 돌려준다.
/// 좌표·크기 모두 physical px. 모서리(코너)가 변보다 우선한다. 순수 함수라 OS 무관
/// 하게 컴파일·테스트된다(실제 `drag_resize_window` 호출만 Linux 배선).
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
/// - **그 외 OS(Windows)**: 변경 없음(네이티브 데코 유지, P5 후속).
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
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
