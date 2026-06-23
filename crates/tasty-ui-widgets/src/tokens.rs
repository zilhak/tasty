//! Layout 상수 — `tasty-egui-theme::Theme` 토큰이 아닌 layout-level 값.
//!
//! 폭·패딩·corner·stroke 등 *위젯 구조* 와 직결된 값을 한 곳에 모은다.
//! 색·폰트는 `Theme` 에서 가져가므로 여기엔 두지 않는다.
//!
//! SIZING 과 의미가 겹치는 값은 매직넘버로 재정의하지 않고 `SIZING` 을 단일
//! 소스로 참조한다 (이름은 "이 위치에서 어떤 토큰을 쓰는지" 의미론을 보존).
//! `LogicalPx(pub f32)` 의 `.0` 필드는 const 컨텍스트에서 접근 가능하다.

use tasty_type_appearance::theme::SIZING;

/// 좌측 sub-menu 패널의 고정 폭 (logical px). = `SIZING.tab_width`.
pub const SUB_TAB_PANEL_WIDTH: f32 = SIZING.tab_width.0;

/// 좌측 패널 Frame 의 inner margin (px, symmetric). = `SIZING.spacing_sm`.
/// (이전 6 은 4px 그리드 위반이었다 → spacing_sm 으로 정합.)
pub const PANEL_INNER_MARGIN: i8 = SIZING.spacing_sm.0 as i8;

/// 좌측 패널 Frame 의 corner radius. = `SIZING.corner_radius`.
pub const PANEL_CORNER_RADIUS: f32 = SIZING.corner_radius.0;

/// 좌측 패널 Frame 의 stroke 굵기. = `SIZING.border_width`.
pub const PANEL_STROKE_WIDTH: f32 = SIZING.border_width.0;

/// 좌·우 패널 사이 horizontal spacing. = `SIZING.spacing_sm`.
pub const PANEL_SPACING: f32 = SIZING.spacing_sm.0;

/// 탭 컨텐츠 영역의 inner padding (px, 4 면 동일). = `SIZING.spacing_lg`.
/// settings 모달 본체와 갤러리의 layout idiom 공통 표준.
pub const TAB_CONTENT_PADDING: i8 = SIZING.spacing_lg.0 as i8;

// ── 디자인 컴포넌트 const — Theme 토큰에 대응값이 없어 위젯 레벨로 둔다 ──

/// Button lg 높이 = 디자인 control-height-lg(32). Theme 에 32 토큰 없음.
pub const CONTROL_HEIGHT_LG: f32 = 32.0;

/// IconButton md/lg 글리프 = 디자인 icon scale 기본 16. (sm 은 `theme.icon_glyph_size_sm`=14.)
pub const ICON_GLYPH_MD: f32 = 16.0;

/// Input leading 아이콘 글리프 = 디자인 icon-size-md(16). (token-policy: 15 → 16 snap.)
/// = `SIZING.icon_glyph_size_md`.
pub const INPUT_ICON_GLYPH: f32 = SIZING.icon_glyph_size_md.0;

/// TreeRow 높이 = 디자인 control-height-tree(22). Theme 에 대응 토큰 없음.
pub const TREE_ROW_HEIGHT: f32 = 22.0;
