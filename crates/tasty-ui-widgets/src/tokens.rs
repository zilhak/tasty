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
//
// **`STRUCT_GAP_*` 는 host UI zoom 을 타지 않는다** — 평범한 `const` 라
// `Theme::with_colors_and_zoom` 의 배율 경로에 없다. 의도된 것이다: 승격해봐야
// 얻는 게 없기 때문이다. `zoomed()` 는 `(px * z).round()` 라 지원 배율
// (0.85 / 1.0 / 1.2)에서 1px 는 셋 다 1 로, 2px 는 2/2/2 로 **원값 그대로**
// 되돌아온다. 3px 는 3/3/4, 4px 는 3/4/5 로 ±1px 만 흔들리는데, 이 값들은 요소
// 크기가 아니라 구조 hairline/nudge 라 그 ±1px 가 리듬을 개선하지 않는다. 같은
// 이유로 `Theme` 의 `border_width`(1px)·`tab_indicator_width`(2px)도 `zoomed()`
// 를 거치지 않는다. 크기가 zoom 을 따라가야 하는 값이면 `STRUCT_GAP_*` 가 아니라
// `Theme` 필드로 둔다(예: `icon_glyph_size_row_action`).

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

// ── toast 카드 구조 치수 — 본체와 갤러리 specimen 의 **단일 출처** ──────────────
//
// 이 값들은 본체 `src/adapters/ui/toast.rs` 와 갤러리
// (`catalog/toast_card.rs` · `catalog/components/toast.rs`)에 **각각 정의**돼 있었다.
// 값이 같아 보여도 정의가 둘이면 언제든 갈릴 수 있고, 갈린 뒤에는 어느 쪽이 정본인지
// 알 방법이 없다 — "본체와 동일" 이라고 적힌 주석이 실제로는 자체 사본이었던 사고가
// 같은 라운드의 다른 리뷰에서 나왔다. 그래서 값을 옮기지 않고 **정의를 여기 하나로**
// 모은다(값 무변경).
//
// 아래 넷은 `SIZING` 과 값이 정확히 같으므로 매직넘버로 재정의하지 않고 참조한다 —
// 이름이 "이 위치에서 어떤 토큰을 쓰는지" 를 남긴다(이 파일 상단 규칙).

/// 스코프 가장자리에서의 안쪽 여백. = `SIZING.spacing_md`.
pub const TOAST_SCOPE_MARGIN: f32 = SIZING.spacing_md.0;
/// 본문 텍스트의 좌우 여백. = `SIZING.spacing_md`.
pub const TOAST_PADDING_X: f32 = SIZING.spacing_md.0;
/// 본문 텍스트의 상하 여백. = `SIZING.spacing_sm`.
pub const TOAST_PADDING_Y: f32 = SIZING.spacing_sm.0;
/// 좌측 컬러 바 두께. = `SIZING.spacing_xs`.
pub const TOAST_ACCENT_BAR_WIDTH: f32 = SIZING.spacing_xs.0;

/// 토스트 사이 세로 간격. **4px 그리드 밖(6)** 이다.
///
/// **대응 토큰이 없는 것이 아니다** — `component.toast-gap` 이 `{semantic.space-sm}` = 8 로
/// vendor 되어 있고, 그 값이 여기와 다르다. 그리드 스텝에서 6 을 못 찾은 것과 컴포넌트
/// 토큰이 없는 것은 다른 물음인데 전에는 한 문장이 둘을 합쳐 두고 있었다. 어느 값이
/// 맞는지는 host chrome 토큰 전환 시리즈의 결정이고, 그때 6 → 8 이면 **픽셀이 바뀐다** —
/// `docs/design/systems/token-crosswalk.md` 의 "시리즈 02 착수 전 필독" 표에 등재돼 있다.
pub const TOAST_GAP: f32 = 6.0;

/// 매우 좁은 surface 에서 `max_width` 를 surface 안쪽 폭으로 클램프할 때의 하한.
/// `wrap_width`(= max_width - PADDING_X*2 - ACCENT_BAR_WIDTH)가 음수가 되지 않도록
/// 최소 한 글자 분량의 여유를 보장한다.
pub const TOAST_MIN_INNER_WIDTH: f32 = 48.0;

/// 스코프 폭의 80% 를 쓰되 그 결과가 이 값보다 작아지지 않게 하는 하한.
pub const TOAST_MIN_MAX_WIDTH: f32 = 80.0;

// ── 빈/로딩/오류 중앙 블록 — file_picker · remote_attach 공통 이디엄 ────────────

/// 그 블록 맨 위 스피너·글리프의 한 변. 아이콘 스케일(12·14·15·16) 밖이고 대응
/// `Theme` 토큰이 없다 — 이 블록만의 구조 크기다. 본체 popup 둘과 갤러리 specimen
/// 둘, 네 곳이 같은 값을 각자 들고 있던 것을 여기로 모았다.
pub const CENTER_GLYPH_SIZE: f32 = 22.0;

/// 그 블록의 공칭 높이 — 본체 popup(`file_picker`). 세로 가운데 정렬의 기준이다.
///
/// **갤러리 specimen 은 [`CENTER_BLOCK_H_SPECIMEN`](120)을 쓴다 — 값이 갈려 있다.**
/// 어느 쪽이 맞는지는 디자인 질문이라 값을 맞추지 않고 정의만 한곳에 모았다.
pub const CENTER_BLOCK_H_POPUP: f32 = 100.0;

/// 같은 블록의 갤러리 specimen 값. [`CENTER_BLOCK_H_POPUP`] 참고 — 불일치는 의도가
/// 아니라 미해결 상태다.
pub const CENTER_BLOCK_H_SPECIMEN: f32 = 120.0;

// ── 본체 ↔ 갤러리 specimen 이중 정의였던 나머지 ─────────────────────────────────
//
// toast 상수와 같은 형태로 발견된 것들이다. 값이 같아 보여도 정의가 둘이면 갈릴 수
// 있고, 이 둘은 실제로 주석까지 서로 다르게 적혀 있었다(같은 값에 다른 근거).

/// 빈 상태 글리프 크기 — 아이콘 스케일(12·14·15·16) 밖의 일회성 값. 설정
/// Misc › Scripts 와 그 갤러리 specimen 이 같은 상수를 읽는다.
pub const EMPTY_STATE_GLYPH_SIZE: f32 = 26.0;

/// 클립보드 뷰어 CenterState 아이콘 크기 — 아이콘 글리프 토큰 상한(16) 밖의 화면
/// 전용 고정값. plugin 본체와 갤러리 specimen 이 같은 상수를 읽는다.
pub const CLIPBOARD_CENTER_ICON_SIZE: f32 = 28.0;

// ── 본체 ↔ 갤러리 specimen 공용 — 4px 그리드 밖 구조값 ─────────────────────────

/// 튜토리얼 스텝 행의 인덱스 캡 ↔ 본문 가로 간격. 디자인 전사값 10 으로 그리드
/// 밖이고, 같은 프레임의 `inner_margin` 이 쓰는 10(= `spacing_xs * 2.5`)과 짝이다.
/// 본체 `adapters/ui/tutorial/topic_popup.rs` 와 갤러리 specimen 이 같이 읽는다.
pub const TUTORIAL_STEP_GAP_X: f32 = 10.0;

/// 전송(transfer) 카드의 좌우 안쪽 여백. 디자인 전사값 10 으로 그리드 밖이다.
/// 본체 `adapters/ui/popup/transfer.rs` 와 갤러리 specimen 이 같이 읽는다.
/// `egui::Margin` 필드가 `i8` 이라 타입을 맞춰 둔다.
pub const TRANSFER_CARD_PAD_X: i8 = 10;

// ── 스케일 밖 코너 반경 — DTCG radius 스케일에 대응이 없는 값 (ADR-0126) ──
//
// DTCG radius 스케일은 `radius-2` · `radius-4` · `radius-8` · `radius-full` 뿐이고
// `Theme` 의 `corner_radius_sm`(2) · `corner_radius`(4) · `corner_radius_lg`(8) 가 그
// 셋을 그대로 노출한다. 아래 값들은 어디에도 없다. ADR-0126 대로 **가까운 토큰으로
// 스냅하지 않는다** — 스냅은 픽셀을 바꾸는 디자인 결정이고, 리터럴 정리가 곁다리로
// 할 일이 아니다. 이름과 사유를 붙여 드리프트를 눈에 보이게 두고, 수렴 여부는 디자인
// 판단으로 넘긴다.
//
// **대가는 폰트 축과 같다**: 명명 const 는 `Theme::with_colors_and_zoom` 의 `zoomed()`
// 경로 밖이라 `ui_scale` 을 타지 않는다. 그리고 `corner_radius*` 토큰은 **탄다**
// (`zoomed(SIZING.corner_radius)`) — 그래서 이 자리들만 배율 0.85 / 1.2 에서 고정
// 반경으로 남는다. 굵기 쪽(`border_width` · `icon_stroke_width`)이 애초에 zoom 을 타지
// 않는 것과 다르다. 반경은 대가가 실재한다.
//
// `.corner_radius()` 는 `impl Into<CornerRadius>` 를 받고 `From<f32>` 가
// `same(radius.round() as u8)` 이라, f32 로 넘기는 것은 `CornerRadius::same(n)` 과
// 값이 같다.

/// 부팅 화면 chrome(버튼 · 인셋 프레임)의 코너 반경. **스케일 밖 6px.**
/// 디자인의 `component.button-radius` 는 `semantic.radius`(4)인데 부팅 전 셸
/// (`shell_setup` · `boot_error`)만 6 을 쓴다. 의도인지 표류인지 소스에 신호가 없어
/// 값을 그대로 두고 이름만 붙였다.
pub const BOOT_CHROME_CORNER_RADIUS: f32 = 6.0;

/// 부팅 셸 카드의 코너 반경. **스케일 밖 12px.** 떠 있는 패널용 토큰
/// `corner_radius_lg`(8 = `primitive.radius-8`)보다 크다.
pub const BOOT_CARD_CORNER_RADIUS: f32 = 12.0;

/// accent tag pill 의 코너 반경. **스케일 밖 3px.** 디자인의
/// `component.badge-radius` 는 `semantic.radius-sm`(2)다.
pub const TAG_PILL_CORNER_RADIUS: f32 = 3.0;
