//! Layout 상수 — `tasty-egui-theme::Theme` 토큰이 아닌 layout-level 값.
//!
//! 폭·패딩·corner·stroke 등 *위젯 구조* 와 직결된 magic number 를 한 곳에 모은다.
//! 색·폰트는 `Theme` 에서 가져가므로 여기엔 두지 않는다.

/// 좌측 sub-menu 패널의 고정 폭 (logical px).
pub const SUB_TAB_PANEL_WIDTH: f32 = 150.0;

/// 좌측 패널 Frame 의 inner margin (px, symmetric).
pub const PANEL_INNER_MARGIN: i8 = 6;

/// 좌측 패널 Frame 의 corner radius.
pub const PANEL_CORNER_RADIUS: f32 = 4.0;

/// 좌측 패널 Frame 의 stroke 굵기.
pub const PANEL_STROKE_WIDTH: f32 = 1.0;

/// 좌·우 패널 사이 horizontal spacing.
pub const PANEL_SPACING: f32 = 8.0;

/// 탭 컨텐츠 영역의 inner padding (px, 4 면 동일).
/// settings 모달 본체와 갤러리의 layout idiom 공통 표준.
pub const TAB_CONTENT_PADDING: i8 = 16;
