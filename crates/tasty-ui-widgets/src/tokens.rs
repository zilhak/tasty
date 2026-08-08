//! Layout 상수 — `tasty-egui-theme::Theme` 토큰이 아닌 layout-level 값.
//!
//! 폭·패딩·corner·stroke 등 *위젯 구조* 와 직결된 값을 한 곳에 모은다.
//! 색·폰트는 `Theme` 에서 가져가므로 여기엔 두지 않는다.
//!
//! SIZING 과 의미가 겹치는 값은 매직넘버로 재정의하지 않고 `SIZING` 을 단일
//! 소스로 참조한다 (이름은 "이 위치에서 어떤 토큰을 쓰는지" 의미론을 보존).
//! `LogicalPx(pub f32)` 의 `.0` 필드는 const 컨텍스트에서 접근 가능하다.

use tasty_type_appearance::theme::SIZING;
use tasty_type_geometry::length::LogicalPx;

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

// ── 구조 간격 상수 — DTCG primitive 직접 대응 (semantic 부재, Rust-only) ──
//
// 디자인은 4px 그리드 spacing 스텝 밖의 미세 구조 간격에 primitive `size-1/2/3`
// 을 직접 쓴다 — 요소 간 간격 리듬이 아니라 컴포넌트 내부 구조를 맞추는 값이라
// spacing 스텝 체계 밖에 둔 것. Rust 에는 대응 semantic 이 없고 `tasty-design-tokens` 의
// `generated::primitive` 는 pub(crate) 라, 위젯 레벨 상수로 둔다 (crate 정책:
// SIZING 에 없는 디자인 값의 단일 위치). `vspace`/`hspace` 헬퍼와 함께 사용.

/// 구조 간격 1px = DTCG `primitive.size-1`.
/// 예: 사이드바 WorkspaceRow subtitle 의 margin-top (디자인 chrome.jsx
/// `marginTop: var(--tasty-size-1)`).
pub const STRUCT_GAP_1: LogicalPx = LogicalPx(1.0);

/// 구조 간격 2px = DTCG `primitive.size-2`.
/// 예: 사이드바 도구 리스트/collapsed rail 의 gap (디자인 chrome.jsx
/// `gap: var(--tasty-size-2)`).
pub const STRUCT_GAP_2: LogicalPx = LogicalPx(2.0);

/// 구조 간격 3px = DTCG `primitive.size-3`.
/// 예: 사이드바 WorkspaceRow description 의 margin-top (디자인 chrome.jsx
/// `marginTop: var(--tasty-size-3)`).
pub const STRUCT_GAP_3: LogicalPx = LogicalPx(3.0);

/// 구조 간격 4px = DTCG `primitive.size-4`.
/// control-internal nudge (spacing 리듬 아님) — 예: 다이얼로그 close 버튼 마진 x,
/// 포트스캐너 검색줄 x nudge. 값 자체는 그리드 상(4px)이지만, 다른 `STRUCT_GAP_*`
/// 처럼 요소 간 간격 리듬으로 반복 사용되는 스케일이 아니라 컨트롤 내부 위치를
/// 맞추는 1회성 보정값이라 별도 상수로 구분한다.
pub const STRUCT_GAP_4: LogicalPx = LogicalPx(4.0);
