//! macOS는 네이티브 창 버튼을 유지한 채 콘텐츠를 타이틀바까지 확장한다.
//! Windows/Linux는 OS 데코레이션을 끄고 Tasty가 타이틀바와 버튼을 그린다.

use winit::window::{ResizeDirection, WindowAttributes};

/// 데코레이션 없는 Windows/Linux 창의 가장자리 리사이즈 영역 두께(물리 px).
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

/// OS별 창 데코레이션 속성. macOS는 투명 타이틀바·전체 콘텐츠 영역과 네이티브 버튼을 쓴다.
/// Windows/Linux는 OS 데코레이션을 끄며 Windows에서는 undecorated shadow를 켠다.
/// 버튼·드래그·리사이즈 입력은 호스트가 처리한다. 다른 OS는 기존 속성을 유지한다.
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
