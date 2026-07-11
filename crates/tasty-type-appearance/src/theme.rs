// 이 모듈은 색상 const fn (`derive_overlays`) 정의의 본거지다.
// 외부에서는 차단되는 `HexColor::from_rgba` 호출이 여기서는 의도된 사용 —
// 도출 overlay 정의 자체이므로 lint 예외.
#![allow(clippy::disallowed_methods)]

//! Theme schema — UI 시각 표현의 데이터 모델.
//!
//! ```text
//! tasty-themes ──▶ resolve(settings)/mutate ──▶ Theme 인스턴스
//!                                                 │
//!                                                 ▼
//!                                          UI: theme().X
//! ```
//!
//! 이 모듈은 **데이터 구조** 만 정의한다. partial 누적·TOML 로딩·전역 RwLock·
//! 빌트인 mocha fallback 같은 mutation/IO 로직은 `tasty-themes` 가 담당하고,
//! 그 결과를 `Theme` 인스턴스로 빌드하여 themes 의 전역 슬롯에 박아 넣는다.
//!
//! `Theme` 은 평평한 단일 구조체로, UI 코드가 `theme().crust` /
//! `theme().spacing_sm` / `theme().is_light` 처럼 한 단계로 접근한다.
//! 색상 직렬화·partial 표현은 `ThemeColors` / `PartialColors` 에서 분리.

use crate::color::{GpuRgb, HexColor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tasty_type_geometry::length::LogicalPx;

// ============================================================================
//  SurfaceTheme — surface kind 별 focused/unfocused 색 묶음
// ============================================================================

/// 한 surface 종류의 색 묶음. `Theme.surface_themes` 안에 `id -> SurfaceTheme` 으로 보관.
///
/// terminal 특유의 selection / search_match 색은 여기 들지 않는다 — Theme 의 top-level
/// 필드에 남아있고, 다른 surface 가 그 기능을 가질 때 sub-struct 로 흡수한다.
///
/// plugin 이 자기 surface kind 를 등록하면 그 id 로 SurfaceTheme 을 추가할 수 있다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceTheme {
    pub focused_bg: HexColor,
    pub focused_fg: HexColor,
    pub unfocused_bg: HexColor,
    pub unfocused_fg: HexColor,
}

impl Default for SurfaceTheme {
    fn default() -> Self {
        FALLBACK_SURFACE.clone()
    }
}

/// `SurfaceTheme` 의 모든 필드를 `Option` 으로 감싼 partial. theme TOML 의 `[surfaces.<id>]`
/// sub-table 과 `theme_overrides` 양쪽에 쓰인다.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PartialSurfaceTheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_fg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfocused_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfocused_fg: Option<HexColor>,
}

impl PartialSurfaceTheme {
    pub fn is_empty(&self) -> bool {
        self.focused_bg.is_none()
            && self.focused_fg.is_none()
            && self.unfocused_bg.is_none()
            && self.unfocused_fg.is_none()
    }
}

impl SurfaceTheme {
    /// `Some(v)` 인 필드만 자기 자신에 덮어쓴다. (None 은 보존)
    pub fn apply_partial(&mut self, p: &PartialSurfaceTheme) {
        if let Some(v) = p.focused_bg {
            self.focused_bg = v;
        }
        if let Some(v) = p.focused_fg {
            self.focused_fg = v;
        }
        if let Some(v) = p.unfocused_bg {
            self.unfocused_bg = v;
        }
        if let Some(v) = p.unfocused_fg {
            self.unfocused_fg = v;
        }
    }
}

/// surface_themes 에 해당 id 가 없을 때 호출자가 쓸 수 있는 안전한 fallback.
/// 모든 surface 가 검은 배경 + 흰 글자로 동작한다. theme 이 정상 적용된 상태에서는
/// 절대 도달하지 않으며, 부팅 직후 / 잘못된 plugin 등록 케이스의 마지막 보루.
pub const FALLBACK_SURFACE: SurfaceTheme = SurfaceTheme {
    focused_bg: HexColor::from_rgb(0, 0, 0),
    focused_fg: HexColor::from_rgb(0xcd, 0xd6, 0xf4),
    unfocused_bg: HexColor::from_rgb(0x1e, 0x1e, 0x2e),
    unfocused_fg: HexColor::from_rgb(0xa6, 0xad, 0xc8),
};

// ============================================================================
//  Titlebar OS 리터럴 — 테마 불변 (mocha/latte 공통)
// ============================================================================

/// Windows 캡션 close 버튼 hover 시 시스템 red. OS 가 박아둔 리터럴이라 테마와
/// 무관하게 고정 (`--tasty-color-os-windows-close` = `#c42b1c`). primitive 1곳에만.
pub const ACCENT_WINDOW_CLOSE: HexColor = HexColor::from_rgb(0xc4, 0x2b, 0x1c);

/// close 버튼 글리프 — 어두운 red 위 흰 글자라 두 테마 모두 white 고정
/// (`--tasty-text-on-window-close`).
pub const TEXT_ON_WINDOW_CLOSE: HexColor = HexColor::from_rgb(0xff, 0xff, 0xff);

/// light 테마(Latte)에서 accent 위 텍스트색 — DTCG `text-on-accent` 의 Latte remap
/// 은 절대색 white(`--tasty-color-white`). vivid accent(blue 등) 위 white 대비
/// ≈4.9:1 로 4.5:1 충족. Mocha 는 `crust` 를 쓰므로 이 리터럴은 light 전용.
pub const TEXT_ON_ACCENT_LIGHT: HexColor = HexColor::from_rgb(0xff, 0xff, 0xff);

/// macOS 신호등(traffic light) 색. OS 가 인식하는 affordance 라 사용자가 정확한
/// 시스템 red/amber/green 을 기대한다 — Catppuccin accent 가 아니다. Windows close
/// 처럼 테마 불변 OS-system 리터럴 (`--tasty-color-os-macos-*`). mocha/latte 동일값.
pub const OS_MACOS_CLOSE: HexColor = HexColor::from_rgb(0xec, 0x6a, 0x5e);
/// macOS 신호등 — minimize (amber).
pub const OS_MACOS_MIN: HexColor = HexColor::from_rgb(0xf4, 0xbf, 0x4f);
/// macOS 신호등 — zoom (green).
pub const OS_MACOS_ZOOM: HexColor = HexColor::from_rgb(0x61, 0xc5, 0x54);

/// 워터멜론 브랜드(수박) 마크 색. OS 신호등처럼 테마 불변 브랜드 고정 리터럴
/// (`--tasty-color-melon-flesh` = `#f25d6b`, primitives.css). mocha/latte 동일값.
pub const BRAND_MELON_FLESH: HexColor = HexColor::from_rgb(0xf2, 0x5d, 0x6b);

/// disabled 컨트롤 공통 톤 (`--tasty-opacity-disabled` = 0.5). 모든 위젯이 이 값으로
/// 통일한다. LogicalPx 가 아닌 순수 비율이므로 별도 f32 상수.
pub const OPACITY_DISABLED: f32 = 0.5;

/// 뒤로 물러난(recessed) 요소 공통 톤 (`--tasty-opacity-recessed` = 0.4). 상위 스코프
/// 배너 뒤로 디밍되는 하위 스코프 배너가 이 값(≈60% 투명)을 쓴다. OPACITY_DISABLED
/// 와 같은 이유로 순수 비율 f32 상수.
pub const OPACITY_RECESSED: f32 = 0.4;

/// 비-터미널 chrome(배너 등장/소멸 등)의 UI 모션 지속시간 (`--tasty-motion-ui` →
/// `--tasty-duration-120` = 120ms). theme.md 의 "터미널 콘텐츠 애니메이션 0ms" 는
/// 터미널 콘텐츠 한정이라, 알림류 chrome 에는 페이드를 허용한다.
pub const MOTION_UI_MS: f32 = 120.0;

/// 빠른 비-터미널 chrome UI 모션 지속시간 (`--tasty-motion-ui-fast` →
/// `--tasty-duration-90` = 90ms). `MOTION_UI_MS`(120ms) 보다 한 단계 짧은 페이드 —
/// switch-number-overlay 등장처럼 즉각성이 중요한 오버레이용. 터미널 콘텐츠 0ms
/// 불변식은 터미널 grid 한정이라 UI 오버레이 chrome 에는 무관.
pub const MOTION_UI_FAST_MS: f32 = 90.0;

/// status-dot pulse 링 1회 주기 (`--tasty-status-dot-pulse-duration` →
/// `--tasty-duration-1600` = 1600ms). 확장·페이드 링 애니메이션의 주기 — 터미널
/// 콘텐츠가 아닌 상태 표시 chrome 모션이라 토큰화 대상.
pub const STATUS_DOT_PULSE_MS: f32 = 1600.0;

/// modifier-hint 오버레이 홀드→표시 지연 (`--tasty-motion-hold-reveal` →
/// `--tasty-duration-500` = 500ms). **모션이 아니라 지연**이다 — 사용자가 modifier 를
/// 실수로 스쳐도 안 뜨게 하는 의도 게이트라 reduced_motion 여부와 무관하게 유지된다
/// (fade 는 200ms 만 모션). 터미널 콘텐츠 0ms 불변식과 무관한 UI 오버레이 chrome.
pub const MOTION_HOLD_REVEAL_MS: f32 = 500.0;

/// modifier-hint **Shift 단독** 홀드 표시 지연 (`--tasty-motion-hold-reveal-shift` →
/// `--tasty-duration-1200` = 1200ms). Shift 는 대문자·기호 입력에 상시 쓰여 스침이 잦으므로,
/// Shift 만 눌린 경우에 한해 기본 500ms 대신 1.2초를 기다려 타이핑 중 오버레이가 튀는 것을
/// 억제한다(Ctrl+Shift 등 다른 modifier 를 동반한 조합은 의도적 단축키라 기본 500ms 유지).
/// [`MOTION_HOLD_REVEAL_MS`] 와 마찬가지로 **지연이며 모션이 아니라** reduced_motion 무관.
pub const MOTION_HOLD_REVEAL_SHIFT_MS: f32 = 1200.0;

/// 비-터미널 chrome UI 페이드 지속시간 (`--tasty-motion-ui-fade` →
/// `--tasty-duration-200` = 200ms). modifier-hint 오버레이 등장 페이드(opacity 0.2→1.0)에
/// 쓴다. `MOTION_UI_MS`(120ms)보다 한 단계 긴 페이드 — 홀드 게이트를 통과한 뒤라
/// 좀 더 여유 있게 떠오른다. reduced_motion 시 이 페이드는 0ms 로 생략된다.
pub const MOTION_UI_FADE_MS: f32 = 200.0;

/// 떠 있는 패널(popover / banner)의 lift 그림자 토큰. egui 비의존 순수 표현 —
/// egui 변환은 `egui-compat` feature 의 [`ShadowToken::to_egui`] 가 담당한다.
///
/// design `--tasty-shadow-popover` (= 허용된 단 하나의 popover scrim 그림자, 새 그림자
/// 시스템을 만들지 않고 재사용). `alpha` 는 0~255 straight 검정 알파.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowToken {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub alpha: u8,
}

#[cfg(feature = "egui-compat")]
impl ShadowToken {
    /// egui epaint Shadow 로 변환. offset/blur/spread 는 px 정수로 반올림.
    pub fn to_egui(self) -> egui::epaint::Shadow {
        egui::epaint::Shadow {
            offset: [self.offset_x.round() as i8, self.offset_y.round() as i8],
            blur: self.blur.round() as u8,
            spread: self.spread.round() as u8,
            color: egui::Color32::from_black_alpha(self.alpha),
        }
    }
}

/// `--tasty-shadow-popover` 값. 배너/popover 가 떠 있음을 나타내는 단차.
pub const SHADOW_POPOVER: ShadowToken = ShadowToken {
    offset_x: 0.0,
    offset_y: 8.0,
    blur: 24.0,
    spread: 0.0,
    alpha: 90,
};

/// design `--tasty-scrim-bg` — 모달/팝업 뒤 무대를 어둡게 덮는 scrim 알파. black 50%
/// straight (테마 무관 고정 검정). `Theme::scrim()` 이 이 값으로 색을 만든다.
pub const SCRIM_ALPHA: u8 = 128;

/// design `--tasty-preset-split-zone-bg` = accent-primary 22%. 프리셋 편집기 경계
/// hover-split 존 밴드 채움 알파(22%×255≈56). accent 색은 테마 가변 → 알파만 파생.
pub const PRESET_SPLIT_ZONE_BG_ALPHA: u8 = 56;

/// design `--tasty-preset-split-zone-border` = accent-primary 55%. split 존 안쪽 변의
/// 2px 분할선 알파(55%×255≈140).
pub const PRESET_SPLIT_ZONE_BORDER_ALPHA: u8 = 140;

// ============================================================================
//  ThemeSizing — 모든 테마 공통
// ============================================================================

/// UI 크기/간격. 모든 테마에서 공통. Theme 인스턴스에도 동일 값이 복사된다.
#[derive(Debug, Clone, Copy)]
pub struct ThemeSizing {
    /// 서브-caption micro-label (kbd / badge / tag / tree·menu meta) — 10px.
    pub font_size_micro: LogicalPx,
    pub font_size_caption: LogicalPx,
    pub font_size_body: LogicalPx,
    pub font_size_heading: LogicalPx,
    pub font_size_max: LogicalPx,
    /// markdown surface heading 앵커 — egui_commonmark 이 `Heading`↔`Body` 사이를 보간하는
    /// 헤딩 사다리의 최상단(H1). 렌더 CONTENT 라 UI 14px 상한 예외 (20px). per-H2 픽셀 토큰
    /// (`prose-h2`)·본문 leading 배수(`line-height-prose`)는 라이브러리가 소유해 은퇴됨.
    pub font_size_prose_h1: LogicalPx,
    /// UI 텍스트(툴팁 등 여러 줄 chrome 문단) 줄간격 배수 (1.4, design
    /// `--tasty-line-height-ui`). prose(1.6)보다 촘촘한 UI 전용 배수 — 무차원 비율이라
    /// `f32`. 폰트 크기에 곱해 줄 높이를 만든다.
    pub line_height_ui: f32,
    /// terminal cell 스케일 — small (12px).
    pub font_size_term_sm: LogicalPx,
    /// terminal cell 스케일 — 기본 (14px).
    pub font_size_term: LogicalPx,
    /// terminal cell 스케일 — large (16px).
    pub font_size_term_lg: LogicalPx,
    pub border_width: LogicalPx,
    /// Focus ring 두께 (2px). accent-primary 색 outline (egui selection.stroke).
    pub focus_ring_width: LogicalPx,
    pub corner_radius: LogicalPx,
    /// 작은 inner element(키캡 등)용 코너 반경 (2px, design `--tasty-radius-sm`).
    pub corner_radius_sm: LogicalPx,
    /// 떠 있는 패널(배너)용 큰 코너 반경 (8px, design `--tasty-radius-8`). 시스템
    /// 기본 4px 의 의도적 2배 — CSD 윈도우 코너 / floating 패널 느낌. 배너 셸이 사용.
    pub corner_radius_lg: LogicalPx,
    pub item_height_tree: LogicalPx,
    pub item_height_interactive: LogicalPx,
    pub item_height_tab: LogicalPx,
    pub tab_width: LogicalPx,
    pub spacing_xs: LogicalPx,
    pub spacing_sm: LogicalPx,
    pub spacing_md: LogicalPx,
    pub spacing_lg: LogicalPx,
    pub spacing_xl: LogicalPx,
    // ── IconButton 글리프 (정사각 프레임 = `item_height_tab`, 코너 = `corner_radius`) ──
    /// 인라인 글리프 (chevron / close 등) 최소 글리프 크기 (12px, design `--tasty-icon-size-xs`).
    pub icon_glyph_size_xs: LogicalPx,
    /// IconButton `sm` 안의 SVG 글리프 크기 (search bar nav/toggle).
    pub icon_glyph_size_sm: LogicalPx,
    /// IconButton `md` 안의 SVG 글리프 크기 (sidebar tools/plugins/settings 등).
    pub icon_glyph_size_md: LogicalPx,
    // ── Sidebar 전용 (host UI zoom 영향 받음) ──
    /// Full sidebar 헤더의 수박 로고 크기.
    pub sidebar_logo_size: LogicalPx,
    /// Collapsed sidebar 헤더의 수박 로고 크기.
    pub sidebar_logo_collapsed_size: LogicalPx,
    /// `tasty.` 워드마크 mono 폰트 크기.
    pub sidebar_wordmark_font_size: LogicalPx,
    /// "WORKSPACES" 섹션 헤딩 mono 폰트 크기.
    pub sidebar_section_heading_font_size: LogicalPx,
    /// Tools / Plugins / Settings / New workspace 등 사이드바 ghost block 버튼 라벨 폰트 크기.
    pub sidebar_button_label_font_size: LogicalPx,
    /// Collapsed sidebar icon / workspace 슬롯의 공통 너비.
    pub sidebar_collapsed_slot_width: LogicalPx,
    /// Collapsed sidebar Tools / Plugins / Settings / Expand 아이콘 슬롯 높이.
    pub sidebar_collapsed_icon_height: LogicalPx,
    /// Collapsed sidebar workspace 슬롯 높이.
    pub sidebar_collapsed_workspace_height: LogicalPx,
    // ── Tab bar 전용 (host UI zoom 영향 받지 않음) ──
    // 사용자 제약 — 탭바는 host UI zoom 제외. with_colors_and_zoom 에서 SIZING 그대로
    // 복사 (border_width / tab_width 와 동일 처리).
    /// 탭바 자체 높이.
    pub tab_bar_height: LogicalPx,
    /// "+" 새 탭 버튼 폰트 크기.
    pub tab_bar_label_font_size: LogicalPx,
    /// 좌/우 스크롤 화살표 폰트 크기.
    pub tab_bar_arrow_font_size: LogicalPx,
    // ── 작업영역 하단 StatusBar 전용 (host UI zoom 영향 받지 않음) ──
    // tab_bar 와 동일하게 px 고정 — with_colors_and_zoom 에서 SIZING 그대로 복사.
    /// 작업영역 하단 StatusBar 높이.
    pub status_bar_height: LogicalPx,
    // ── Titlebar (CSD) 전용 (host UI zoom 영향 받지 않음) ──
    // 디자인 jsx 가 px 고정이고, OS 데코 관습상 고정 px 가 맞다. tab_bar 와 동일하게
    // with_colors_and_zoom 에서 SIZING 그대로 복사 (zoom 미적용).
    /// CSD 타이틀바 높이.
    pub titlebar_height: LogicalPx,
    /// macOS 신호등(traffic light) 점 지름.
    pub traffic_size: LogicalPx,
    /// Windows 캡션 버튼(min·max·close) 폭.
    pub caption_width: LogicalPx,
    /// Linux DE 버튼(min·max·close) 원형 지름.
    pub window_button_size: LogicalPx,
    // ── 가독 폭 (Note / content / 모달 컬럼용) ──
    /// 좁은 가독 폭 (300px).
    pub measure_sm: LogicalPx,
    /// 중간 가독 폭 (400px).
    pub measure_md: LogicalPx,
    /// 넓은 가독 폭 (460px).
    pub measure_lg: LogicalPx,
    /// 최대 가독 폭 (560px).
    pub measure_xl: LogicalPx,
    // ── form-control 폭 ──
    /// 극소 입력 폭 (90px).
    pub field_width_xs: LogicalPx,
    /// 색상 입력 폭 (110px).
    pub field_width_color: LogicalPx,
    /// 중간 입력 폭 (160px).
    pub field_width_md: LogicalPx,
    /// 넓은 입력 폭 (200px).
    pub field_width_lg: LogicalPx,
    // ── 세부 치수 ──
    /// 토스트 최대 폭 (320px).
    pub toast_max_width: LogicalPx,
    /// 툴팁 버블 최대 폭 (240px, design `--tasty-tooltip-max-width` = `--tasty-size-240`).
    /// 초과 시 텍스트 줄바꿈. host UI content 라 zoom 적용.
    pub tooltip_max_width: LogicalPx,
    /// 상태 점(status dot) 지름 (8px).
    pub status_dot_size: LogicalPx,
    /// 스피너 지름 (16px).
    pub spinner_size: LogicalPx,
    /// 토스트 좌측 accent 바 두께 (3px).
    pub toast_accent_width: LogicalPx,
    /// 탭 active indicator 두께 (2px).
    pub tab_indicator_width: LogicalPx,
    /// 상단 정렬 모달(command palette) 상단 gap (88px).
    pub overlay_top_offset: LogicalPx,
}

pub const SIZING: ThemeSizing = ThemeSizing {
    font_size_micro: LogicalPx(10.0),
    font_size_caption: LogicalPx(11.0),
    font_size_body: LogicalPx(13.0),
    font_size_heading: LogicalPx(13.0), // semibold 로 구분, 크기는 같음
    font_size_max: LogicalPx(14.0),
    font_size_prose_h1: LogicalPx(20.0),
    line_height_ui: 1.4,
    font_size_term_sm: LogicalPx(12.0),
    font_size_term: LogicalPx(14.0),
    font_size_term_lg: LogicalPx(16.0),
    border_width: LogicalPx(1.0),
    focus_ring_width: LogicalPx(2.0),
    corner_radius: LogicalPx(4.0),
    corner_radius_sm: LogicalPx(2.0),
    corner_radius_lg: LogicalPx(8.0),
    item_height_tree: LogicalPx(22.0),
    item_height_interactive: LogicalPx(28.0),
    item_height_tab: LogicalPx(24.0),
    tab_width: LogicalPx(150.0),
    spacing_xs: LogicalPx(4.0),
    spacing_sm: LogicalPx(8.0),
    spacing_md: LogicalPx(12.0),
    spacing_lg: LogicalPx(16.0),
    spacing_xl: LogicalPx(24.0),
    icon_glyph_size_xs: LogicalPx(12.0),
    icon_glyph_size_sm: LogicalPx(14.0),
    icon_glyph_size_md: LogicalPx(16.0),
    sidebar_logo_size: LogicalPx(22.0),
    sidebar_logo_collapsed_size: LogicalPx(24.0),
    sidebar_wordmark_font_size: LogicalPx(17.0),
    sidebar_section_heading_font_size: LogicalPx(10.0),
    // 디자인 판정 (2026-07-02 token-coverage): UI 타입 스케일은 10/11/13/14 고정,
    // 12 는 터미널 전용 — `font-size-caption`(11) 으로 스냅.
    sidebar_button_label_font_size: LogicalPx(11.0),
    sidebar_collapsed_slot_width: LogicalPx(32.0),
    sidebar_collapsed_icon_height: LogicalPx(22.0),
    sidebar_collapsed_workspace_height: LogicalPx(28.0),
    tab_bar_height: LogicalPx(24.0),
    tab_bar_label_font_size: LogicalPx(13.0),
    tab_bar_arrow_font_size: LogicalPx(11.0),
    status_bar_height: LogicalPx(24.0),
    titlebar_height: LogicalPx(36.0),
    traffic_size: LogicalPx(12.0),
    caption_width: LogicalPx(46.0),
    window_button_size: LogicalPx(24.0),
    measure_sm: LogicalPx(300.0),
    measure_md: LogicalPx(400.0),
    measure_lg: LogicalPx(460.0),
    measure_xl: LogicalPx(560.0),
    field_width_xs: LogicalPx(90.0),
    field_width_color: LogicalPx(110.0),
    field_width_md: LogicalPx(160.0),
    field_width_lg: LogicalPx(200.0),
    toast_max_width: LogicalPx(320.0),
    tooltip_max_width: LogicalPx(240.0),
    status_dot_size: LogicalPx(8.0),
    spinner_size: LogicalPx(16.0),
    toast_accent_width: LogicalPx(3.0),
    tab_indicator_width: LogicalPx(2.0),
    overlay_top_offset: LogicalPx(88.0),
};

// ============================================================================
//  ThemeColors / PartialColors — 직렬화 표현
// ============================================================================

/// 테마 색상 풀 세트. 직렬화 가능 — `AppearanceSettings.theme_base` 가 이걸 저장.
///
/// `hover_overlay` / `active_overlay` / `separator` 같은 반투명 의미 색은 여기 없다.
/// 그건 `is_light` 에서 자동 도출되므로 `Theme` 인스턴스에서만 보유한다.
///
/// 모든 색 필드가 `HexColor`. ansi 는 hex 로 통일했고, GPU 셰이더에 넘길 때만
/// `.to_float()` 한다. surface 종류별(focused/unfocused × bg/fg) 색은
/// `surface_themes` map 안에 `SurfaceTheme` 으로 담는다 — plugin 이 자기 id 로
/// 추가 가능.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThemeColors {
    // ── Surfaces (low → high elevation) ──
    pub crust: HexColor,
    pub mantle: HexColor,
    pub base: HexColor,
    pub surface0: HexColor,
    pub surface1: HexColor,
    pub surface2: HexColor,

    // ── Overlays ──
    pub overlay0: HexColor,
    pub overlay1: HexColor,
    pub overlay2: HexColor,

    // ── Text ──
    pub text: HexColor,
    pub subtext1: HexColor,
    pub subtext0: HexColor,
    pub placeholder: HexColor,

    // ── Accent ──
    pub blue: HexColor,
    pub green: HexColor,
    pub red: HexColor,
    pub yellow: HexColor,
    pub peach: HexColor,
    pub mauve: HexColor,
    pub teal: HexColor,
    pub sky: HexColor,
    pub lavender: HexColor,
    pub flamingo: HexColor,
    pub pink: HexColor,
    pub maroon: HexColor,
    pub rosewater: HexColor,

    // ── Terminal-specific 색 (다른 surface 가 selection 기능을 가지면 SurfaceTheme 로 흡수) ──
    pub selection_bg: HexColor,
    /// vi copy mode 의 cursor cell highlight. selection_bg 와 시각적으로 구분되어야 한다.
    pub vi_cursor_bg: HexColor,
    pub search_match_bg: HexColor,
    pub search_match_active_bg: HexColor,

    // ── ANSI 16 ──
    pub ansi_black: HexColor,
    pub ansi_red: HexColor,
    pub ansi_green: HexColor,
    pub ansi_yellow: HexColor,
    pub ansi_blue: HexColor,
    pub ansi_magenta: HexColor,
    pub ansi_cyan: HexColor,
    pub ansi_white: HexColor,
    pub ansi_bright_black: HexColor,
    pub ansi_bright_red: HexColor,
    pub ansi_bright_green: HexColor,
    pub ansi_bright_yellow: HexColor,
    pub ansi_bright_blue: HexColor,
    pub ansi_bright_magenta: HexColor,
    pub ansi_bright_cyan: HexColor,
    pub ansi_bright_white: HexColor,

    // ── surface kind 별 색 묶음 ──
    /// surface kind id ("terminal", "markdown", plugin id 등) → `SurfaceTheme`.
    /// theme TOML 의 `[surfaces.<id>]` sub-table 에서 정의. apply_partial 시
    /// entry 단위 merge.
    #[serde(default)]
    pub surface_themes: BTreeMap<String, SurfaceTheme>,
}

/// `ThemeColors` 의 모든 필드를 `Option<HexColor>` 로 감싼 표현.
///
/// - 사용자가 settings UI 픽커로 손댄 흔적(`AppearanceSettings.theme_overrides`)
/// - 외부 TOML 의 partial 테마 정의 (`ThemeFile` 에서 변환)
///
/// `ThemeColors::apply_partial()` 로 `Some` 필드만 base 에 덮어쓴다.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PartialColors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crust: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mantle: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface0: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface1: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface2: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay0: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay1: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay2: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtext1: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtext0: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blue: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub green: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub red: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yellow: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peach: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mauve: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teal: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lavender: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flamingo: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pink: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maroon: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rosewater: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vi_cursor_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_match_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_match_active_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_black: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_red: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_green: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_yellow: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_blue: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_magenta: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_cyan: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_white: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_black: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_red: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_green: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_yellow: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_blue: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_magenta: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_cyan: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ansi_bright_white: Option<HexColor>,

    /// surface kind 별 partial 색. entry 단위로 base 의 `SurfaceTheme` 위에 덮어쓴다.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub surface_themes: BTreeMap<String, PartialSurfaceTheme>,
}

impl PartialColors {
    /// 모든 필드를 None 으로 리셋.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// 단일 None 인지 (= 사용자 흔적 없음).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl ThemeColors {
    /// `Some(v)` 인 필드만 자기 자신에 덮어쓴다. (None 은 보존)
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 평면 per-필드 Option 병합 — 색 필드 수만큼 동일 if-let 반복, 중첩 없음(clippy 과대계상)
    pub fn apply_partial(&mut self, p: &PartialColors) {
        if let Some(v) = p.crust {
            self.crust = v;
        }
        if let Some(v) = p.mantle {
            self.mantle = v;
        }
        if let Some(v) = p.base {
            self.base = v;
        }
        if let Some(v) = p.surface0 {
            self.surface0 = v;
        }
        if let Some(v) = p.surface1 {
            self.surface1 = v;
        }
        if let Some(v) = p.surface2 {
            self.surface2 = v;
        }
        if let Some(v) = p.overlay0 {
            self.overlay0 = v;
        }
        if let Some(v) = p.overlay1 {
            self.overlay1 = v;
        }
        if let Some(v) = p.overlay2 {
            self.overlay2 = v;
        }
        if let Some(v) = p.text {
            self.text = v;
        }
        if let Some(v) = p.subtext1 {
            self.subtext1 = v;
        }
        if let Some(v) = p.subtext0 {
            self.subtext0 = v;
        }
        if let Some(v) = p.placeholder {
            self.placeholder = v;
        }
        if let Some(v) = p.blue {
            self.blue = v;
        }
        if let Some(v) = p.green {
            self.green = v;
        }
        if let Some(v) = p.red {
            self.red = v;
        }
        if let Some(v) = p.yellow {
            self.yellow = v;
        }
        if let Some(v) = p.peach {
            self.peach = v;
        }
        if let Some(v) = p.mauve {
            self.mauve = v;
        }
        if let Some(v) = p.teal {
            self.teal = v;
        }
        if let Some(v) = p.sky {
            self.sky = v;
        }
        if let Some(v) = p.lavender {
            self.lavender = v;
        }
        if let Some(v) = p.flamingo {
            self.flamingo = v;
        }
        if let Some(v) = p.pink {
            self.pink = v;
        }
        if let Some(v) = p.maroon {
            self.maroon = v;
        }
        if let Some(v) = p.rosewater {
            self.rosewater = v;
        }
        if let Some(v) = p.selection_bg {
            self.selection_bg = v;
        }
        if let Some(v) = p.vi_cursor_bg {
            self.vi_cursor_bg = v;
        }
        if let Some(v) = p.search_match_bg {
            self.search_match_bg = v;
        }
        if let Some(v) = p.search_match_active_bg {
            self.search_match_active_bg = v;
        }
        if let Some(v) = p.ansi_black {
            self.ansi_black = v;
        }
        if let Some(v) = p.ansi_red {
            self.ansi_red = v;
        }
        if let Some(v) = p.ansi_green {
            self.ansi_green = v;
        }
        if let Some(v) = p.ansi_yellow {
            self.ansi_yellow = v;
        }
        if let Some(v) = p.ansi_blue {
            self.ansi_blue = v;
        }
        if let Some(v) = p.ansi_magenta {
            self.ansi_magenta = v;
        }
        if let Some(v) = p.ansi_cyan {
            self.ansi_cyan = v;
        }
        if let Some(v) = p.ansi_white {
            self.ansi_white = v;
        }
        if let Some(v) = p.ansi_bright_black {
            self.ansi_bright_black = v;
        }
        if let Some(v) = p.ansi_bright_red {
            self.ansi_bright_red = v;
        }
        if let Some(v) = p.ansi_bright_green {
            self.ansi_bright_green = v;
        }
        if let Some(v) = p.ansi_bright_yellow {
            self.ansi_bright_yellow = v;
        }
        if let Some(v) = p.ansi_bright_blue {
            self.ansi_bright_blue = v;
        }
        if let Some(v) = p.ansi_bright_magenta {
            self.ansi_bright_magenta = v;
        }
        if let Some(v) = p.ansi_bright_cyan {
            self.ansi_bright_cyan = v;
        }
        if let Some(v) = p.ansi_bright_white {
            self.ansi_bright_white = v;
        }
        // surface_themes: entry 단위 merge. base 에 없는 id 면 default 위에 partial 적용.
        for (id, partial_st) in &p.surface_themes {
            self.surface_themes
                .entry(id.clone())
                .or_default()
                .apply_partial(partial_st);
        }
    }
}

// ============================================================================
//  Theme — 실제 적용된 인스턴스 (평평한 구조)
// ============================================================================

/// 현재 적용된 테마 인스턴스. **UI 코드는 `theme()` 으로 받아 평평하게 접근**한다
/// (예: `theme().crust`, `theme().spacing_sm`, `theme().is_light`).
///
/// `ThemeColors` 의 모든 필드를 펼쳐 담고, sizing/플래그/도출 색상을 함께 보유.
/// surface kind 별 색은 `surface_themes` map — `surface(id)` 헬퍼로 접근 권장.
#[derive(Debug, Clone)]
pub struct Theme {
    // ── ThemeColors 와 동일 필드 (펼친 형태) ──
    pub crust: HexColor,
    pub mantle: HexColor,
    pub base: HexColor,
    pub surface0: HexColor,
    pub surface1: HexColor,
    pub surface2: HexColor,
    pub overlay0: HexColor,
    pub overlay1: HexColor,
    pub overlay2: HexColor,
    pub text: HexColor,
    pub subtext1: HexColor,
    pub subtext0: HexColor,
    pub placeholder: HexColor,
    pub blue: HexColor,
    pub green: HexColor,
    pub red: HexColor,
    pub yellow: HexColor,
    pub peach: HexColor,
    pub mauve: HexColor,
    pub teal: HexColor,
    pub sky: HexColor,
    pub lavender: HexColor,
    pub flamingo: HexColor,
    pub pink: HexColor,
    pub maroon: HexColor,
    pub rosewater: HexColor,
    pub selection_bg: HexColor,
    pub vi_cursor_bg: HexColor,
    pub search_match_bg: HexColor,
    pub search_match_active_bg: HexColor,
    pub ansi_black: HexColor,
    pub ansi_red: HexColor,
    pub ansi_green: HexColor,
    pub ansi_yellow: HexColor,
    pub ansi_blue: HexColor,
    pub ansi_magenta: HexColor,
    pub ansi_cyan: HexColor,
    pub ansi_white: HexColor,
    pub ansi_bright_black: HexColor,
    pub ansi_bright_red: HexColor,
    pub ansi_bright_green: HexColor,
    pub ansi_bright_yellow: HexColor,
    pub ansi_bright_blue: HexColor,
    pub ansi_bright_magenta: HexColor,
    pub ansi_bright_cyan: HexColor,
    pub ansi_bright_white: HexColor,

    // ── `is_light` 에서 자동 도출 (premultiplied 바이트) ──
    /// 호버 시 배경 오버레이 (~8%). `to_egui_premultiplied()` 로 변환할 것.
    pub hover_overlay: HexColor,
    /// 눌림 시 배경 오버레이 (~12%). `to_egui_premultiplied()` 로 변환할 것.
    pub active_overlay: HexColor,
    /// 구분선 (~8%). `to_egui_premultiplied()` 로 변환할 것.
    pub separator: HexColor,

    // ── 모든 테마 공통 sizing (SIZING 에서 복사) ──
    /// 서브-caption micro-label (kbd / badge / tag / tree·menu meta) — 10px.
    pub font_size_micro: LogicalPx,
    pub font_size_caption: LogicalPx,
    pub font_size_body: LogicalPx,
    pub font_size_heading: LogicalPx,
    pub font_size_max: LogicalPx,
    /// markdown surface heading 앵커 — egui_commonmark 헤딩 사다리 최상단(H1). 렌더 CONTENT 라
    /// UI 14px 상한 예외 (20px). per-H2·본문 leading 은 라이브러리 소유로 은퇴됨.
    pub font_size_prose_h1: LogicalPx,
    /// UI 텍스트(툴팁 등) 줄간격 배수 (1.4, design `--tasty-line-height-ui`). 무차원 비율.
    pub line_height_ui: f32,
    /// terminal cell 스케일 — small (12px).
    pub font_size_term_sm: LogicalPx,
    /// terminal cell 스케일 — 기본 (14px).
    pub font_size_term: LogicalPx,
    /// terminal cell 스케일 — large (16px).
    pub font_size_term_lg: LogicalPx,
    pub border_width: LogicalPx,
    /// Focus ring 두께 (2px). accent-primary 색 outline (egui selection.stroke).
    pub focus_ring_width: LogicalPx,
    pub corner_radius: LogicalPx,
    /// 작은 inner element(키캡 등)용 코너 반경 (2px, design `--tasty-radius-sm`).
    pub corner_radius_sm: LogicalPx,
    /// 떠 있는 패널(배너)용 큰 코너 반경 (8px, design `--tasty-radius-8`).
    pub corner_radius_lg: LogicalPx,
    pub item_height_tree: LogicalPx,
    pub item_height_interactive: LogicalPx,
    pub item_height_tab: LogicalPx,
    pub tab_width: LogicalPx,
    pub spacing_xs: LogicalPx,
    pub spacing_sm: LogicalPx,
    pub spacing_md: LogicalPx,
    pub spacing_lg: LogicalPx,
    pub spacing_xl: LogicalPx,
    // ── IconButton 글리프 (정사각 프레임 = `item_height_tab`, 코너 = `corner_radius`) ──
    /// 인라인 글리프 (chevron / close 등) 최소 글리프 크기 (12px, design `--tasty-icon-size-xs`).
    pub icon_glyph_size_xs: LogicalPx,
    /// IconButton `sm` 안의 SVG 글리프 크기 (search bar nav/toggle).
    pub icon_glyph_size_sm: LogicalPx,
    /// IconButton `md` 안의 SVG 글리프 크기 (sidebar tools/plugins/settings 등).
    pub icon_glyph_size_md: LogicalPx,
    // ── Sidebar 전용 (host UI zoom 영향 받음) ──
    pub sidebar_logo_size: LogicalPx,
    pub sidebar_logo_collapsed_size: LogicalPx,
    pub sidebar_wordmark_font_size: LogicalPx,
    pub sidebar_section_heading_font_size: LogicalPx,
    pub sidebar_button_label_font_size: LogicalPx,
    pub sidebar_collapsed_slot_width: LogicalPx,
    pub sidebar_collapsed_icon_height: LogicalPx,
    pub sidebar_collapsed_workspace_height: LogicalPx,
    // ── Tab bar 전용 (host UI zoom 영향 받지 않음) ──
    pub tab_bar_height: LogicalPx,
    pub tab_bar_label_font_size: LogicalPx,
    pub tab_bar_arrow_font_size: LogicalPx,
    // ── 작업영역 하단 StatusBar 전용 (host UI zoom 영향 받지 않음) ──
    pub status_bar_height: LogicalPx,
    // ── Titlebar (CSD) 전용 (host UI zoom 영향 받지 않음) ──
    pub titlebar_height: LogicalPx,
    pub traffic_size: LogicalPx,
    pub caption_width: LogicalPx,
    pub window_button_size: LogicalPx,
    // ── 가독 폭 (Note / content / 모달 컬럼용) ──
    pub measure_sm: LogicalPx,
    pub measure_md: LogicalPx,
    pub measure_lg: LogicalPx,
    pub measure_xl: LogicalPx,
    // ── form-control 폭 ──
    pub field_width_xs: LogicalPx,
    pub field_width_color: LogicalPx,
    pub field_width_md: LogicalPx,
    pub field_width_lg: LogicalPx,
    // ── 세부 치수 ──
    pub toast_max_width: LogicalPx,
    /// 툴팁 버블 최대 폭 (240px, design `--tasty-tooltip-max-width`).
    pub tooltip_max_width: LogicalPx,
    pub status_dot_size: LogicalPx,
    pub spinner_size: LogicalPx,
    pub toast_accent_width: LogicalPx,
    pub tab_indicator_width: LogicalPx,
    pub overlay_top_offset: LogicalPx,

    /// host UI zoom 배율 (`with_colors_and_zoom` 에 전달된 값 그대로, 기본 1.0).
    /// component 접근자가 primitive 직접 alias 치수에 곱하는 용도 — 이미 `zoomed()`
    /// 로 resolve 된 필드에는 재적용하지 않는다.
    pub ui_zoom: f32,

    // ── 라이트/다크 플래그 ──
    pub is_light: bool,

    // ── surface kind 별 색 묶음 ──
    /// `"terminal"`, `"markdown"`, 또는 plugin 등록 id → `SurfaceTheme`.
    /// 호출자는 보통 [`Theme::surface`] 헬퍼를 통해 접근 (없는 id 는 `FALLBACK_SURFACE`).
    pub surface_themes: BTreeMap<String, SurfaceTheme>,
}

/// `is_light` 에 따른 hover/active/separator 도출.
/// premultiplied sRGB 바이트로 저장 — 변환 시 `to_egui_premultiplied()` 사용.
const fn derive_overlays(is_light: bool) -> (HexColor, HexColor, HexColor) {
    if is_light {
        (
            HexColor::from_rgba(0, 0, 0, 20), // black ~8%
            HexColor::from_rgba(0, 0, 0, 31), // black ~12%
            HexColor::from_rgba(0, 0, 0, 20), // black ~8%
        )
    } else {
        (
            HexColor::from_rgba(20, 20, 20, 20), // white-ish ~8%
            HexColor::from_rgba(31, 31, 31, 31), // white-ish ~12%
            HexColor::from_rgba(20, 20, 20, 20), // white-ish ~8%
        )
    }
}

impl Theme {
    /// `ThemeColors` + `is_light` 로 풀 인스턴스 빌드. SIZING 은 const 에서 복사.
    /// `surface_themes` 가 `BTreeMap` 이라 더 이상 `const fn` 일 수 없다.
    pub fn with_colors(c: ThemeColors, is_light: bool) -> Self {
        Self::with_colors_and_zoom(c, is_light, 1.0)
    }

    /// `with_colors` 의 일반화 — host UI zoom 배율을 sizing token 자체에 곱해서
    /// 박는다. `ui_zoom == 1.0` 이면 `with_colors` 와 결과 동일.
    ///
    /// 미적용 토큰 (디자인 정책상 zoom 영향 받지 않음):
    /// - `border_width` (1px 보더는 zoom 무관)
    /// - `tab_width` (탭바는 host UI zoom 제외)
    pub fn with_colors_and_zoom(c: ThemeColors, is_light: bool, ui_zoom: f32) -> Self {
        let (hover_overlay, active_overlay, separator) = derive_overlays(is_light);
        let zoomed = |px: LogicalPx| LogicalPx((px.value() * ui_zoom).round());
        Self {
            crust: c.crust,
            mantle: c.mantle,
            base: c.base,
            surface0: c.surface0,
            surface1: c.surface1,
            surface2: c.surface2,
            overlay0: c.overlay0,
            overlay1: c.overlay1,
            overlay2: c.overlay2,
            text: c.text,
            subtext1: c.subtext1,
            subtext0: c.subtext0,
            placeholder: c.placeholder,
            blue: c.blue,
            green: c.green,
            red: c.red,
            yellow: c.yellow,
            peach: c.peach,
            mauve: c.mauve,
            teal: c.teal,
            sky: c.sky,
            lavender: c.lavender,
            flamingo: c.flamingo,
            pink: c.pink,
            maroon: c.maroon,
            rosewater: c.rosewater,
            selection_bg: c.selection_bg,
            vi_cursor_bg: c.vi_cursor_bg,
            search_match_bg: c.search_match_bg,
            search_match_active_bg: c.search_match_active_bg,
            ansi_black: c.ansi_black,
            ansi_red: c.ansi_red,
            ansi_green: c.ansi_green,
            ansi_yellow: c.ansi_yellow,
            ansi_blue: c.ansi_blue,
            ansi_magenta: c.ansi_magenta,
            ansi_cyan: c.ansi_cyan,
            ansi_white: c.ansi_white,
            ansi_bright_black: c.ansi_bright_black,
            ansi_bright_red: c.ansi_bright_red,
            ansi_bright_green: c.ansi_bright_green,
            ansi_bright_yellow: c.ansi_bright_yellow,
            ansi_bright_blue: c.ansi_bright_blue,
            ansi_bright_magenta: c.ansi_bright_magenta,
            ansi_bright_cyan: c.ansi_bright_cyan,
            ansi_bright_white: c.ansi_bright_white,
            hover_overlay,
            active_overlay,
            separator,
            font_size_micro: zoomed(SIZING.font_size_micro),
            font_size_caption: zoomed(SIZING.font_size_caption),
            font_size_body: zoomed(SIZING.font_size_body),
            font_size_heading: zoomed(SIZING.font_size_heading),
            font_size_max: zoomed(SIZING.font_size_max),
            // prose / term 스케일 = surface CONTENT 폰트 (markdown/terminal). UI zoom
            // 영향 받지 않는다 — 터미널/마크다운 셀 폰트는 자체 설정 경로를 따른다.
            font_size_prose_h1: SIZING.font_size_prose_h1,
            // 줄간격 배수 — 무차원 비율, zoom 무관 (폰트 크기 자체가 스케일을 담당).
            line_height_ui: SIZING.line_height_ui,
            font_size_term_sm: SIZING.font_size_term_sm,
            font_size_term: SIZING.font_size_term,
            font_size_term_lg: SIZING.font_size_term_lg,
            border_width: SIZING.border_width,
            focus_ring_width: zoomed(SIZING.focus_ring_width),
            corner_radius: zoomed(SIZING.corner_radius),
            corner_radius_sm: zoomed(SIZING.corner_radius_sm),
            corner_radius_lg: zoomed(SIZING.corner_radius_lg),
            item_height_tree: zoomed(SIZING.item_height_tree),
            item_height_interactive: zoomed(SIZING.item_height_interactive),
            item_height_tab: zoomed(SIZING.item_height_tab),
            tab_width: SIZING.tab_width,
            spacing_xs: zoomed(SIZING.spacing_xs),
            spacing_sm: zoomed(SIZING.spacing_sm),
            spacing_md: zoomed(SIZING.spacing_md),
            spacing_lg: zoomed(SIZING.spacing_lg),
            spacing_xl: zoomed(SIZING.spacing_xl),
            icon_glyph_size_xs: zoomed(SIZING.icon_glyph_size_xs),
            icon_glyph_size_sm: zoomed(SIZING.icon_glyph_size_sm),
            icon_glyph_size_md: zoomed(SIZING.icon_glyph_size_md),
            sidebar_logo_size: zoomed(SIZING.sidebar_logo_size),
            sidebar_logo_collapsed_size: zoomed(SIZING.sidebar_logo_collapsed_size),
            sidebar_wordmark_font_size: zoomed(SIZING.sidebar_wordmark_font_size),
            sidebar_section_heading_font_size: zoomed(SIZING.sidebar_section_heading_font_size),
            sidebar_button_label_font_size: zoomed(SIZING.sidebar_button_label_font_size),
            sidebar_collapsed_slot_width: zoomed(SIZING.sidebar_collapsed_slot_width),
            sidebar_collapsed_icon_height: zoomed(SIZING.sidebar_collapsed_icon_height),
            sidebar_collapsed_workspace_height: zoomed(SIZING.sidebar_collapsed_workspace_height),
            // Tab bar 전용 — 사용자 제약 "탭바 zoom 제외" 에 따라 SIZING 그대로 (zoom 미적용).
            tab_bar_height: SIZING.tab_bar_height,
            tab_bar_label_font_size: SIZING.tab_bar_label_font_size,
            tab_bar_arrow_font_size: SIZING.tab_bar_arrow_font_size,
            // 작업영역 하단 StatusBar — tab_bar 와 동일하게 zoom 미적용.
            status_bar_height: SIZING.status_bar_height,
            // Titlebar (CSD) 전용 — px 고정 디자인, tab_bar 와 동일하게 zoom 미적용.
            titlebar_height: SIZING.titlebar_height,
            traffic_size: SIZING.traffic_size,
            caption_width: SIZING.caption_width,
            window_button_size: SIZING.window_button_size,
            // 가독 폭 / form-control 폭 / 세부 치수 — host UI content, zoom 적용.
            measure_sm: zoomed(SIZING.measure_sm),
            measure_md: zoomed(SIZING.measure_md),
            measure_lg: zoomed(SIZING.measure_lg),
            measure_xl: zoomed(SIZING.measure_xl),
            field_width_xs: zoomed(SIZING.field_width_xs),
            field_width_color: zoomed(SIZING.field_width_color),
            field_width_md: zoomed(SIZING.field_width_md),
            field_width_lg: zoomed(SIZING.field_width_lg),
            toast_max_width: zoomed(SIZING.toast_max_width),
            tooltip_max_width: zoomed(SIZING.tooltip_max_width),
            status_dot_size: zoomed(SIZING.status_dot_size),
            spinner_size: zoomed(SIZING.spinner_size),
            toast_accent_width: zoomed(SIZING.toast_accent_width),
            tab_indicator_width: SIZING.tab_indicator_width,
            overlay_top_offset: zoomed(SIZING.overlay_top_offset),
            ui_zoom,
            is_light,
            surface_themes: c.surface_themes,
        }
    }

    /// surface kind id 로 SurfaceTheme 조회. 없으면 [`FALLBACK_SURFACE`] 를 가리킨다.
    /// theme 이 정상 적용된 상태에서는 항상 빌트인 entry (`"terminal"`, `"markdown"`) 가
    /// 존재한다.
    pub fn surface(&self, id: &str) -> &SurfaceTheme {
        self.surface_themes.get(id).unwrap_or(&FALLBACK_SURFACE)
    }

    /// 현재 색상 스냅샷을 `ThemeColors` 로 추출.
    pub fn extract_colors(&self) -> ThemeColors {
        ThemeColors {
            crust: self.crust,
            mantle: self.mantle,
            base: self.base,
            surface0: self.surface0,
            surface1: self.surface1,
            surface2: self.surface2,
            overlay0: self.overlay0,
            overlay1: self.overlay1,
            overlay2: self.overlay2,
            text: self.text,
            subtext1: self.subtext1,
            subtext0: self.subtext0,
            placeholder: self.placeholder,
            blue: self.blue,
            green: self.green,
            red: self.red,
            yellow: self.yellow,
            peach: self.peach,
            mauve: self.mauve,
            teal: self.teal,
            sky: self.sky,
            lavender: self.lavender,
            flamingo: self.flamingo,
            pink: self.pink,
            maroon: self.maroon,
            rosewater: self.rosewater,
            selection_bg: self.selection_bg,
            vi_cursor_bg: self.vi_cursor_bg,
            search_match_bg: self.search_match_bg,
            search_match_active_bg: self.search_match_active_bg,
            ansi_black: self.ansi_black,
            ansi_red: self.ansi_red,
            ansi_green: self.ansi_green,
            ansi_yellow: self.ansi_yellow,
            ansi_blue: self.ansi_blue,
            ansi_magenta: self.ansi_magenta,
            ansi_cyan: self.ansi_cyan,
            ansi_white: self.ansi_white,
            ansi_bright_black: self.ansi_bright_black,
            ansi_bright_red: self.ansi_bright_red,
            ansi_bright_green: self.ansi_bright_green,
            ansi_bright_yellow: self.ansi_bright_yellow,
            ansi_bright_blue: self.ansi_bright_blue,
            ansi_bright_magenta: self.ansi_bright_magenta,
            ansi_bright_cyan: self.ansi_bright_cyan,
            ansi_bright_white: self.ansi_bright_white,
            surface_themes: self.surface_themes.clone(),
        }
    }

    /// `ThemeColors` 의 색상 필드만 자신에게 덮어쓴다 (sizing / is_light / 도출 색상은 보존).
    /// is_light 변경이 필요하면 `set_is_light()` 도 호출할 것.
    pub fn apply_colors(&mut self, c: &ThemeColors) {
        let next = Self::with_colors(c.clone(), self.is_light);
        // 색상 + 도출 overlay 만 갱신 (is_light/sizing 은 보존되지만 next 와 동일).
        *self = next;
    }

    /// is_light 플래그를 바꾸고 hover/active/separator 를 재도출.
    pub fn set_is_light(&mut self, is_light: bool) {
        let (h, a, s) = derive_overlays(is_light);
        self.is_light = is_light;
        self.hover_overlay = h;
        self.active_overlay = a;
        self.separator = s;
    }

    /// 평면 색 필드를 다시 [`ThemeColors`] 로 모은다 (`apply_colors`/`with_colors`
    /// 의 역방향). resolved Theme 을 (zoom 독립적인) 색 집합으로 직렬화해 프로세스
    /// 경계 너머로 보내고 [`Theme::with_colors_and_zoom`] 으로 재구성할 때 쓴다
    /// (egui-mesh plugin 의 Theme parity — ADR-0028).
    pub fn to_colors(&self) -> ThemeColors {
        ThemeColors {
            crust: self.crust,
            mantle: self.mantle,
            base: self.base,
            surface0: self.surface0,
            surface1: self.surface1,
            surface2: self.surface2,
            overlay0: self.overlay0,
            overlay1: self.overlay1,
            overlay2: self.overlay2,
            text: self.text,
            subtext1: self.subtext1,
            subtext0: self.subtext0,
            placeholder: self.placeholder,
            blue: self.blue,
            green: self.green,
            red: self.red,
            yellow: self.yellow,
            peach: self.peach,
            mauve: self.mauve,
            teal: self.teal,
            sky: self.sky,
            lavender: self.lavender,
            flamingo: self.flamingo,
            pink: self.pink,
            maroon: self.maroon,
            rosewater: self.rosewater,
            selection_bg: self.selection_bg,
            vi_cursor_bg: self.vi_cursor_bg,
            search_match_bg: self.search_match_bg,
            search_match_active_bg: self.search_match_active_bg,
            ansi_black: self.ansi_black,
            ansi_red: self.ansi_red,
            ansi_green: self.ansi_green,
            ansi_yellow: self.ansi_yellow,
            ansi_blue: self.ansi_blue,
            ansi_magenta: self.ansi_magenta,
            ansi_cyan: self.ansi_cyan,
            ansi_white: self.ansi_white,
            ansi_bright_black: self.ansi_bright_black,
            ansi_bright_red: self.ansi_bright_red,
            ansi_bright_green: self.ansi_bright_green,
            ansi_bright_yellow: self.ansi_bright_yellow,
            ansi_bright_blue: self.ansi_bright_blue,
            ansi_bright_magenta: self.ansi_bright_magenta,
            ansi_bright_cyan: self.ansi_bright_cyan,
            ansi_bright_white: self.ansi_bright_white,
            surface_themes: self.surface_themes.clone(),
        }
    }

    /// GPU 렌더러용 ANSI 16색 팔레트.
    /// 인덱스 순서: black, red, green, yellow, blue, magenta, cyan, white,
    /// bright_black, bright_red, bright_green, bright_yellow, bright_blue,
    /// bright_magenta, bright_cyan, bright_white.
    pub fn ansi_palette(&self) -> [GpuRgb; 16] {
        [
            self.ansi_black.to_gpu_rgb(),
            self.ansi_red.to_gpu_rgb(),
            self.ansi_green.to_gpu_rgb(),
            self.ansi_yellow.to_gpu_rgb(),
            self.ansi_blue.to_gpu_rgb(),
            self.ansi_magenta.to_gpu_rgb(),
            self.ansi_cyan.to_gpu_rgb(),
            self.ansi_white.to_gpu_rgb(),
            self.ansi_bright_black.to_gpu_rgb(),
            self.ansi_bright_red.to_gpu_rgb(),
            self.ansi_bright_green.to_gpu_rgb(),
            self.ansi_bright_yellow.to_gpu_rgb(),
            self.ansi_bright_blue.to_gpu_rgb(),
            self.ansi_bright_magenta.to_gpu_rgb(),
            self.ansi_bright_cyan.to_gpu_rgb(),
            self.ansi_bright_white.to_gpu_rgb(),
        ]
    }
}

// ============================================================================
//  Semantic 접근자 — A1 token-crosswalk 의 의미 라벨 기준
// ============================================================================
//
// `Theme` 은 평면 primitive 필드를 노출하고, 의미(accent-primary 등) 매핑을 semantic
// 접근자로 끌어올린다. bg-*/surface-*/text-*(placeholder 까지)/accent-*/border-* 의
// **단순 primitive 필드 alias** 접근자는 `semantic_color_generated.rs` 로 이관됐다
// (DTCG semantic 색 토큰에서 생성 — SSoT 는 디자인 파일). 아래 `impl Theme` 에는
// codegen 불가라 수기로 남는 접근자만 둔다: is_light 분기(text-on-accent), derive_overlays
// 도출(overlay-*), 합성색(scrim), OS/brand 리터럴, component tier 조합(titlebar/banner) 등.
//
// 매핑 근거: `docs/design/systems/token-crosswalk.md` (semantic ↔ primitive ↔ 필드).
// 같은 primitive 가 여러 role 로 갈리는 다의성(crosswalk §4)은 호출처가 어느 접근자를
// 쓰는지로 표현된다 (예: blue → `accent_primary` / `border_focus` / ansi-blue).
impl Theme {
    /// accent 위 텍스트색 (DTCG `text-on-accent`). 테마별 role-remap: Mocha(dark)
    /// 는 neutral-0(=`crust`), Latte(light)는 절대색 white. vivid accent 위 대비
    /// (4.5:1) 를 양 테마에서 충족시키기 위한 분기 — `is_light` 로 식별.
    #[inline]
    pub fn text_on_accent(&self) -> HexColor {
        if self.is_light {
            TEXT_ON_ACCENT_LIGHT
        } else {
            self.crust
        }
    }

    // ── 오버레이 (overlay-*) — is_light 에서 도출된 필드를 semantic 이름으로 ──
    #[inline]
    pub fn overlay_hover(&self) -> HexColor {
        self.hover_overlay
    }
    #[inline]
    pub fn overlay_active(&self) -> HexColor {
        self.active_overlay
    }

    /// 모달/팝업 뒤 무대를 덮는 scrim 색. design `--tasty-scrim-bg`(black 50%) — 테마
    /// 무관 고정 검정 + [`SCRIM_ALPHA`]. 갤러리 dialog 레시피와 동일 토큰.
    #[inline]
    pub fn scrim(&self) -> HexColor {
        HexColor::from_rgba(0, 0, 0, SCRIM_ALPHA)
    }

    /// 프리셋 편집기 경계 hover-split 존의 밴드 채움색. design
    /// `--tasty-preset-split-zone-bg` = accent-primary 22% — accent 색(테마 가변)은
    /// 유지하고 알파만 파생한다([`PRESET_SPLIT_ZONE_BG_ALPHA`]). split-zone overlay 는
    /// 향후 drag-drop drop-zone 과 토큰을 공유할 의도로 명명(계획 §rationale).
    #[inline]
    pub fn preset_split_zone_bg(&self) -> HexColor {
        self.accent_primary().with_alpha(PRESET_SPLIT_ZONE_BG_ALPHA)
    }

    /// split 존의 안쪽 변에 그리는 2px 분할선 색. design
    /// `--tasty-preset-split-zone-border` = accent-primary 55%
    /// ([`PRESET_SPLIT_ZONE_BORDER_ALPHA`]).
    #[inline]
    pub fn preset_split_zone_border(&self) -> HexColor {
        self.accent_primary()
            .with_alpha(PRESET_SPLIT_ZONE_BORDER_ALPHA)
    }

    /// 프리셋 편집기 leaf 미리보기 값 요약의 라벨(소문자 필드 키) 색. design
    /// `--tasty-preset-leaf-label-fg` → `text-muted`.
    #[inline]
    pub fn preset_leaf_label_fg(&self) -> HexColor {
        self.text_muted()
    }

    /// 프리셋 편집기 leaf 미리보기 값 요약의 값 색. design
    /// `--tasty-preset-leaf-value-fg` → `text-secondary`.
    #[inline]
    pub fn preset_leaf_value_fg(&self) -> HexColor {
        self.text_secondary()
    }

    // ── Titlebar (CSD) 컴포넌트 색 — 기존 semantic 접근자 조합 (changelog §Tokens) ──
    /// 타이틀바 배경 (active/focused). `--tasty-titlebar-bg` → `bg-app`.
    #[inline]
    pub fn titlebar_bg(&self) -> HexColor {
        self.bg_app()
    }
    /// 타이틀바 배경 (inactive/unfocused). `--tasty-titlebar-bg-inactive` → `bg-sidebar`.
    #[inline]
    pub fn titlebar_bg_inactive(&self) -> HexColor {
        self.bg_sidebar()
    }
    /// 타이틀바 하단 1px 보더. `--tasty-titlebar-border` → `separator`.
    #[inline]
    pub fn titlebar_border(&self) -> HexColor {
        self.separator
    }
    /// 타이틀바 전경 (active/focused). `--tasty-titlebar-fg` → `text-secondary`.
    #[inline]
    pub fn titlebar_fg(&self) -> HexColor {
        self.text_secondary()
    }
    /// 타이틀바 전경 (inactive/unfocused). `--tasty-titlebar-fg-inactive` → `text-muted`.
    #[inline]
    pub fn titlebar_fg_inactive(&self) -> HexColor {
        self.text_muted()
    }
    /// Windows close 버튼 hover 의 시스템 red. 테마 불변 OS 리터럴.
    #[inline]
    pub fn accent_window_close(&self) -> HexColor {
        ACCENT_WINDOW_CLOSE
    }
    /// close 버튼 글리프 — 두 테마 모두 white 고정.
    #[inline]
    pub fn text_on_window_close(&self) -> HexColor {
        TEXT_ON_WINDOW_CLOSE
    }

    /// macOS 신호등 close 색 (테마 불변 OS 리터럴). macOS 네이티브 신호등이 OS 렌더되는
    /// 경로에서는 미사용 — tasty 가 자체 신호등을 그리게 되는 경우의 색 소스.
    #[inline]
    pub fn accent_macos_close(&self) -> HexColor {
        OS_MACOS_CLOSE
    }
    /// macOS 신호등 minimize 색 (테마 불변 OS 리터럴).
    #[inline]
    pub fn accent_macos_min(&self) -> HexColor {
        OS_MACOS_MIN
    }
    /// macOS 신호등 zoom 색 (테마 불변 OS 리터럴).
    #[inline]
    pub fn accent_macos_zoom(&self) -> HexColor {
        OS_MACOS_ZOOM
    }

    /// 워터멜론 브랜드(수박) 마크 색 (테마 불변 브랜드 리터럴).
    #[inline]
    pub fn brand_melon_flesh(&self) -> HexColor {
        BRAND_MELON_FLESH
    }

    /// disabled 컨트롤 공통 opacity (0.5). 모든 위젯이 disabled 디밍에 이 값을 쓴다.
    #[inline]
    pub fn opacity_disabled(&self) -> f32 {
        OPACITY_DISABLED
    }

    /// recessed(뒤로 물러난) 요소 공통 opacity (0.4). 상위 스코프 배너 뒤로 디밍되는
    /// 하위 스코프 배너가 이 값(≈60% 투명)을 쓴다. `--tasty-opacity-recessed`.
    #[inline]
    pub fn opacity_recessed(&self) -> f32 {
        OPACITY_RECESSED
    }

    /// cut-pending(잘라내기 대기) explorer 셀 전경(아이콘+라벨) 디밍 opacity (0.5).
    /// 디자인 explorer cell-state matrix "cut (50% opacity) until paste". 디자인 토큰에
    /// cut 전용 primitive 가 없고 값이 disabled 와 동일한 0.5 이므로 같은 primitive 를
    /// semantic 으로 재사용한다(새 primitive 값 미도입 — 디자인 토큰과 drift 없음).
    #[inline]
    pub fn opacity_cut(&self) -> f32 {
        OPACITY_DISABLED
    }

    /// 비-터미널 chrome UI 모션 지속시간 (120ms). 배너 등장/소멸 알파 페이드 등.
    /// `--tasty-motion-ui` → `--tasty-duration-120`.
    #[inline]
    pub fn motion_ui_ms(&self) -> f32 {
        MOTION_UI_MS
    }

    /// 빠른 비-터미널 chrome UI 모션 지속시간 (90ms). switch-number-overlay 등장 페이드 등.
    /// `--tasty-motion-ui-fast` → `--tasty-duration-90`.
    #[inline]
    pub fn motion_ui_fast_ms(&self) -> f32 {
        MOTION_UI_FAST_MS
    }

    /// status-dot pulse 링 1회 주기 (1600ms). `--tasty-status-dot-pulse-duration`
    /// → `--tasty-duration-1600`.
    #[inline]
    pub fn status_dot_pulse_ms(&self) -> f32 {
        STATUS_DOT_PULSE_MS
    }

    // ── 컴포넌트 토큰 (banner) — 기존 semantic 접근자 / 신규 primitive 조합 ──
    /// 배너 셸 배경. `--tasty-banner-bg` → `surface-raised` (surface0).
    #[inline]
    pub fn banner_bg(&self) -> HexColor {
        self.surface_raised()
    }
    /// 배너 셸 보더. `--tasty-banner-border` → `border-strong`.
    #[inline]
    pub fn banner_border(&self) -> HexColor {
        self.border_strong()
    }
    /// 배너 전경(본문). `--tasty-banner-fg` → `text-primary`.
    #[inline]
    pub fn banner_fg(&self) -> HexColor {
        self.text_primary()
    }
    /// 배너 leading 글리프 기본색. `--tasty-banner-icon-fg` → `text-muted`
    /// (배너별 심각도 표현이 override 가능).
    #[inline]
    pub fn banner_icon_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// 배너 우상단 TTL 카운트다운 색. `--tasty-banner-countdown-fg` → `text-muted`.
    #[inline]
    pub fn banner_countdown_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// 떠 있는 패널 그림자. `--tasty-banner-shadow` → `--tasty-shadow-popover`.
    #[inline]
    pub fn shadow_popover(&self) -> ShadowToken {
        SHADOW_POPOVER
    }

    // ── 모션 (modifier-hint) ──
    /// modifier-hint 홀드→표시 지연 (500ms). `--tasty-motion-hold-reveal`. **지연이며
    /// 모션이 아니라** reduced_motion 여부와 무관하게 유지된다.
    #[inline]
    pub fn motion_hold_reveal_ms(&self) -> f32 {
        MOTION_HOLD_REVEAL_MS
    }
    /// modifier-hint **Shift 단독** 홀드 표시 지연 (1200ms). `--tasty-motion-hold-reveal-shift`.
    /// 타이핑 중 Shift 스침으로 오버레이가 튀는 것을 억제한다. **지연이며 모션이 아니라**
    /// reduced_motion 무관.
    #[inline]
    pub fn motion_hold_reveal_shift_ms(&self) -> f32 {
        MOTION_HOLD_REVEAL_SHIFT_MS
    }
    /// UI chrome 페이드 (200ms). `--tasty-motion-ui-fade`. modifier-hint 등장 페이드.
    /// reduced_motion 시 0ms 로 생략.
    #[inline]
    pub fn motion_ui_fade_ms(&self) -> f32 {
        MOTION_UI_FADE_MS
    }

    // ── 컴포넌트 토큰 (modifier-hint 오버레이) — `--tasty-modhint-*` ──
    // 4분류(Popup/Toast/Banner/Modal) 밖의 신규 요소: 키보드 포커스 없음 + 마우스
    // 인터랙티브 + 홀드 수명. 치수는 LogicalPx(DPI 자연대응), 색은 semantic 재사용.
    // raw px 하드코딩 금지 — 본체 draw 는 전부 이 접근자를 경유한다.
    /// 기본 너비 (180px). `--tasty-modhint-width` → `--tasty-size-180`.
    /// 열린 사이드바 폭(`AppearanceSettings.sidebar_width` 기본 180)과 정렬.
    #[inline]
    pub fn modhint_width(&self) -> LogicalPx {
        LogicalPx((180.0 * self.ui_zoom).round())
    }
    /// 기본 높이 (400px). `--tasty-modhint-height`. 펼친 사이드바 하단을 채우는 세로 패널.
    #[inline]
    pub fn modhint_height(&self) -> LogicalPx {
        LogicalPx((400.0 * self.ui_zoom).round())
    }
    /// 리사이즈 최소 너비 (180px = 기본 너비). `--tasty-modhint-min-width`.
    #[inline]
    pub fn modhint_min_width(&self) -> LogicalPx {
        LogicalPx((180.0 * self.ui_zoom).round())
    }
    /// 리사이즈 최소 높이 (240px). `--tasty-modhint-min-height`.
    #[inline]
    pub fn modhint_min_height(&self) -> LogicalPx {
        LogicalPx((240.0 * self.ui_zoom).round())
    }
    /// 드래그 스트립 높이 (28px). `--tasty-modhint-strip-height` → `--tasty-size-28`
    /// (= `item-height-interactive`).
    #[inline]
    pub fn modhint_strip_height(&self) -> LogicalPx {
        self.item_height_interactive
    }
    /// 스크롤 리스트 안쪽 패딩 (10px). `--tasty-modhint-pad`.
    #[inline]
    pub fn modhint_pad(&self) -> LogicalPx {
        LogicalPx((10.0 * self.ui_zoom).round())
    }
    /// 섹션 사이 간격 (12px). `--tasty-modhint-section-gap` → `--tasty-space-md`.
    #[inline]
    pub fn modhint_section_gap(&self) -> LogicalPx {
        self.spacing_md
    }
    /// 섹션 내부 행 사이 간격 (6px). `--tasty-modhint-row-gap`.
    #[inline]
    pub fn modhint_row_gap(&self) -> LogicalPx {
        LogicalPx((6.0 * self.ui_zoom).round())
    }
    /// 빈 조합 섹션의 내부 간격 (3px) — 채워진 섹션(6px)보다 좁게 잡아, 항상 표시되는
    /// 빈 섹션이 리스트를 과하게 늘어뜨리지 않게 한다. `--tasty-modhint-empty-row-gap`
    /// (디자인 `explorations/modifier-hint-empty-section.html` §6-5, `.mh-section--empty { gap: 3px }`).
    #[inline]
    pub fn modhint_empty_row_gap(&self) -> LogicalPx {
        LogicalPx((3.0 * self.ui_zoom).round())
    }
    /// 빈 조합 플레이스홀더 행의 최소 높이 (20px) — 키캡 행(24px)보다 타이트.
    /// `--tasty-modhint-empty-row-min-height` (디자인 시안 §6-5, `.mh-empty { min-height: 20px }`).
    #[inline]
    pub fn modhint_empty_row_min_height(&self) -> LogicalPx {
        LogicalPx((20.0 * self.ui_zoom).round())
    }
    /// 코너 리사이즈 그립 크기 (12px). `--tasty-modhint-grip-size` → `--tasty-icon-size-xs`.
    #[inline]
    pub fn modhint_grip_size(&self) -> LogicalPx {
        self.icon_glyph_size_xs
    }
    /// 패널 배경 (불투명 — 라이브 출력 위). `--tasty-modhint-bg` → `bg-panel`.
    #[inline]
    pub fn modhint_bg(&self) -> HexColor {
        self.bg_panel()
    }
    /// 패널 보더. `--tasty-modhint-border` → `border-strong`.
    #[inline]
    pub fn modhint_border(&self) -> HexColor {
        self.border_strong()
    }
    /// 드래그 스트립 배경. `--tasty-modhint-strip-bg` → `bg-sidebar`.
    #[inline]
    pub fn modhint_strip_bg(&self) -> HexColor {
        self.bg_sidebar()
    }
    /// 스트립 하단 / 조합 헤더 하단 구분선. `--tasty-modhint-separator` → `separator`.
    #[inline]
    pub fn modhint_separator(&self) -> HexColor {
        self.separator
    }
    /// 드래그 스트립 "held" 라벨 색. `--tasty-modhint-held-fg` → `text-muted`.
    #[inline]
    pub fn modhint_held_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// 특수 역할 행 배경 (washed). `--tasty-modhint-role-bg` → `surface-active`.
    #[inline]
    pub fn modhint_role_bg(&self) -> HexColor {
        self.surface_active()
    }
    /// 특수 역할 행 leading 글리프 색. `--tasty-modhint-role-fg` → `accent-primary`.
    #[inline]
    pub fn modhint_role_fg(&self) -> HexColor {
        self.accent_primary()
    }
    /// 액션/역할 행 텍스트 색. `--tasty-modhint-row-fg` → `text-secondary`.
    #[inline]
    pub fn modhint_row_fg(&self) -> HexColor {
        self.text_secondary()
    }
    /// 빈 조합 플레이스홀더("바인딩 없음") 텍스트 색. `--tasty-modhint-empty-fg` → `text-muted`.
    /// 키캡 행의 `text-secondary` 보다 한 단계 절제된 톤 — 실제 항목이 아니라 부재 신호라
    /// 리스트에서 가장 조용하다(wash·글리프·키캡 없음).
    #[inline]
    pub fn modhint_empty_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// plugin 행 leading agent dot 색. `--tasty-modhint-agent-dot` → `accent-agent`.
    #[inline]
    pub fn modhint_agent_dot(&self) -> HexColor {
        self.accent_agent()
    }

    // ── 컴포넌트 토큰 (AutoComplete 후보 드롭다운) — `--tasty-autocomplete-*` ──
    // 자유입력 트리거 + 후보 드롭다운(typeahead). 색·간격·행높이는 전부 기존
    // Input/menu-item/semantic 접근자를 그대로 재사용하므로 여기엔 드롭다운 최대
    // 높이 하나만 둔다(신규 primitive 없음 — 220 은 primitive `size-220`).
    /// 드롭다운 최대 높이 (220px, ≈7행). 초과 시 리스트 내부 스크롤 + shrink-to-fit.
    /// `--tasty-autocomplete-max-height` → `--tasty-size-220`.
    #[inline]
    pub fn autocomplete_max_height(&self) -> LogicalPx {
        LogicalPx((220.0 * self.ui_zoom).round())
    }

    // ── 컴포넌트 토큰 (markdown surface 인라인 표 — grid + zebra) ──
    // Markdown surface 의 GFM 표 전용 tier-3 토큰. 공용 `Table` 위젯과 별개(읽기 전용·정적).
    // 모두 기존 semantic 접근자를 가리키는 포인터 — 신규 primitive/hex 없음. 값 사다리
    // (어두움→밝음): mantle(zebra) < base(행) < surface0(헤더) < surface1(격자선).
    /// 외곽 + 가로 + 세로 격자선. `--tasty-md-table-border` → `border-strong` (surface1).
    #[inline]
    pub fn md_table_border(&self) -> HexColor {
        self.border_strong()
    }
    /// 헤더 밴드 배경 (가장 밝은 채움). `--tasty-md-table-header-bg` → `surface-raised` (surface0).
    #[inline]
    pub fn md_table_header_bg(&self) -> HexColor {
        self.surface_raised()
    }
    /// 헤더 텍스트 — 헤더 신호(색·배경, weight 아님). `--tasty-md-table-header-fg` → `text-primary`.
    #[inline]
    pub fn md_table_header_fg(&self) -> HexColor {
        self.text_primary()
    }
    /// 홀수 행 + 표 base 채움 (불투명). `--tasty-md-table-row-bg` → `bg-panel` (base).
    #[inline]
    pub fn md_table_row_bg(&self) -> HexColor {
        self.bg_panel()
    }
    /// 짝수 행 stripe (미세하게 어둡게). `--tasty-md-table-row-bg-zebra` → `bg-sidebar` (mantle).
    #[inline]
    pub fn md_table_row_bg_zebra(&self) -> HexColor {
        self.bg_sidebar()
    }
    /// 셀 본문 텍스트. `--tasty-md-table-cell-fg` → `text-secondary` (subtext1).
    #[inline]
    pub fn md_table_cell_fg(&self) -> HexColor {
        self.text_secondary()
    }
    /// 셀 좌우 패딩 (8px). `--tasty-md-table-cell-padding-x` → `space-sm`.
    #[inline]
    pub fn md_table_cell_padding_x(&self) -> LogicalPx {
        self.spacing_sm
    }
    /// 셀 상하 패딩 (4px). `--tasty-md-table-cell-padding-y` → `space-xs`.
    #[inline]
    pub fn md_table_cell_padding_y(&self) -> LogicalPx {
        self.spacing_xs
    }

    // ── 컴포넌트 토큰 (DrillDown — master→detail content-swap) — `--tasty-drilldown-*` ──
    // 리스트 뷰 ⇄ 디테일 뷰 전면 교체 레이아웃. 디테일 상단에 back bar(← + 제목 +
    // 우측 actions 슬롯) 밴드. 값은 디자인 `tokens/components.css` 의 alias 체인 그대로.
    // (title-font-weight semibold 는 egui 폰트 weight 한계로 색 강조 관례로 대체 —
    // `button.rs` 참조.)
    /// back bar 밴드 높이 (36px). `--tasty-drilldown-backbar-height` → `size-36`.
    #[inline]
    pub fn drilldown_backbar_height(&self) -> LogicalPx {
        LogicalPx((36.0 * self.ui_zoom).round())
    }
    /// back bar 좌우 패딩 (8px) — ← 버튼을 콘텐츠 좌단에 정렬.
    /// `--tasty-drilldown-backbar-padding-x` → `space-sm`.
    #[inline]
    pub fn drilldown_backbar_padding_x(&self) -> LogicalPx {
        self.spacing_sm
    }
    /// back bar 상하 패딩 (4px). `--tasty-drilldown-backbar-padding-y` → `space-xs`.
    #[inline]
    pub fn drilldown_backbar_padding_y(&self) -> LogicalPx {
        self.spacing_xs
    }
    /// ← ↔ 제목 ↔ actions 간격 (8px). `--tasty-drilldown-backbar-gap` → `space-sm`.
    #[inline]
    pub fn drilldown_backbar_gap(&self) -> LogicalPx {
        self.spacing_sm
    }
    /// back bar 하단 헤어라인. `--tasty-drilldown-backbar-border` → `separator`.
    #[inline]
    pub fn drilldown_backbar_border(&self) -> HexColor {
        self.separator
    }
    /// 디테일 제목 폰트 (13px). `--tasty-drilldown-title-font-size` → `font-size-body`.
    #[inline]
    pub fn drilldown_title_font_size(&self) -> LogicalPx {
        self.font_size_body
    }
    /// 디테일 제목 색. `--tasty-drilldown-title-fg` → `text-primary`.
    #[inline]
    pub fn drilldown_title_fg(&self) -> HexColor {
        self.text_primary()
    }

    // ── 컴포넌트 토큰 (ListCtrl — 행 선택형 내비게이션 리스트) — `--tasty-listctrl-*` ──
    // "하나 골라 진입하는" 풀폭 리스트 (데이터 그리드는 Table). 행 상태 팔레트는
    // TreeRow/MenuItem/Table 과 동일한 list idiom (hover = overlay-hover, selected =
    // surface-active + 2px accent 좌측 바). 값은 디자인 `tokens/components.css` 그대로.
    /// 행 최소 높이 (36px — label + description 수용).
    /// `--tasty-listctrl-row-min-height` → `size-36`.
    #[inline]
    pub fn listctrl_row_min_height(&self) -> LogicalPx {
        LogicalPx((36.0 * self.ui_zoom).round())
    }
    /// 행 좌우 패딩 (12px). `--tasty-listctrl-row-padding-x` → `space-md`.
    #[inline]
    pub fn listctrl_row_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }
    /// 행 상하 패딩 (8px). `--tasty-listctrl-row-padding-y` → `space-sm`.
    #[inline]
    pub fn listctrl_row_padding_y(&self) -> LogicalPx {
        self.spacing_sm
    }
    /// icon ↔ text ↔ trailing 간격 (8px). `--tasty-listctrl-row-gap` → `space-sm`.
    #[inline]
    pub fn listctrl_row_gap(&self) -> LogicalPx {
        self.spacing_sm
    }
    /// 행 corner radius (divided 모드에선 0). `--tasty-listctrl-radius` → `radius-sm`.
    #[inline]
    pub fn listctrl_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }
    /// 라벨 폰트 (13px). `--tasty-listctrl-font-size` → `font-size-body`.
    #[inline]
    pub fn listctrl_font_size(&self) -> LogicalPx {
        self.font_size_body
    }
    /// 라벨 기본 색. `--tasty-listctrl-label-fg` → `text-secondary`.
    #[inline]
    pub fn listctrl_label_fg(&self) -> HexColor {
        self.text_secondary()
    }
    /// hover/selected 라벨 색. `--tasty-listctrl-label-fg-active` → `text-primary`.
    #[inline]
    pub fn listctrl_label_fg_active(&self) -> HexColor {
        self.text_primary()
    }
    /// description(보조 줄) 색. `--tasty-listctrl-desc-fg` → `text-muted`.
    #[inline]
    pub fn listctrl_desc_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// description 폰트 (11px). `--tasty-listctrl-desc-font-size` → `font-size-caption`.
    #[inline]
    pub fn listctrl_desc_font_size(&self) -> LogicalPx {
        self.font_size_caption
    }
    /// leading 아이콘 색. `--tasty-listctrl-icon-fg` → `text-muted`.
    #[inline]
    pub fn listctrl_icon_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// trailing drill-in chevron 색. `--tasty-listctrl-chevron-fg` → `text-muted`.
    #[inline]
    pub fn listctrl_chevron_fg(&self) -> HexColor {
        self.text_muted()
    }
    /// hover 행 워시. `--tasty-listctrl-row-bg-hover` → `overlay-hover`.
    #[inline]
    pub fn listctrl_row_bg_hover(&self) -> HexColor {
        self.overlay_hover()
    }
    /// selected 행 배경. `--tasty-listctrl-row-bg-selected` → `surface-active`.
    #[inline]
    pub fn listctrl_row_bg_selected(&self) -> HexColor {
        self.surface_active()
    }
    /// selected 행 좌측 accent 바 색. `--tasty-listctrl-selected-bar` → `accent-primary`.
    #[inline]
    pub fn listctrl_selected_bar(&self) -> HexColor {
        self.accent_primary()
    }
    /// selected 좌측 바 굵기 (2px). `--tasty-listctrl-selected-bar-width` → `size-2`.
    #[inline]
    pub fn listctrl_selected_bar_width(&self) -> LogicalPx {
        LogicalPx((2.0 * self.ui_zoom).round())
    }
    /// 행 사이 헤어라인. `--tasty-listctrl-divider` → `separator`.
    #[inline]
    pub fn listctrl_divider(&self) -> HexColor {
        self.separator
    }
}

// ============================================================================
//  Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 모든 필드가 같은 색인 schema-level dummy. 빌트인 테마 색 의존 없이
    /// apply_partial / with_colors / extract_colors 동작만 검증.
    /// `surface_themes` 가 BTreeMap 이라 `const fn` 일 수 없다.
    fn dummy_colors() -> ThemeColors {
        let c = HexColor::from_rgb(0x12, 0x34, 0x56);
        ThemeColors {
            crust: c,
            mantle: c,
            base: c,
            surface0: c,
            surface1: c,
            surface2: c,
            overlay0: c,
            overlay1: c,
            overlay2: c,
            text: c,
            subtext1: c,
            subtext0: c,
            placeholder: c,
            blue: c,
            green: c,
            red: c,
            yellow: c,
            peach: c,
            mauve: c,
            teal: c,
            sky: c,
            lavender: c,
            flamingo: c,
            pink: c,
            maroon: c,
            rosewater: c,
            selection_bg: c,
            vi_cursor_bg: c,
            search_match_bg: c,
            search_match_active_bg: c,
            ansi_black: c,
            ansi_red: c,
            ansi_green: c,
            ansi_yellow: c,
            ansi_blue: c,
            ansi_magenta: c,
            ansi_cyan: c,
            ansi_white: c,
            ansi_bright_black: c,
            ansi_bright_red: c,
            ansi_bright_green: c,
            ansi_bright_yellow: c,
            ansi_bright_blue: c,
            ansi_bright_magenta: c,
            ansi_bright_cyan: c,
            ansi_bright_white: c,
            surface_themes: BTreeMap::new(),
        }
    }

    /// dummy 와 달리 semantic 접근자 매핑 검증용 — 비교 대상 필드마다 **고유 색**을
    /// 줘서 `accent_primary()==blue` 가 `==green` 으로 잘못 매핑돼도 잡히게 한다.
    fn distinct_colors() -> ThemeColors {
        let mut c = dummy_colors();
        c.crust = HexColor::from_rgb(1, 0, 0);
        c.mantle = HexColor::from_rgb(2, 0, 0);
        c.base = HexColor::from_rgb(3, 0, 0);
        c.surface0 = HexColor::from_rgb(4, 0, 0);
        c.surface1 = HexColor::from_rgb(5, 0, 0);
        c.surface2 = HexColor::from_rgb(6, 0, 0);
        c.overlay1 = HexColor::from_rgb(7, 0, 0);
        c.subtext0 = HexColor::from_rgb(8, 0, 0);
        c.subtext1 = HexColor::from_rgb(9, 0, 0);
        c.text = HexColor::from_rgb(10, 0, 0);
        c.placeholder = HexColor::from_rgb(11, 0, 0);
        c.blue = HexColor::from_rgb(12, 0, 0);
        c.green = HexColor::from_rgb(13, 0, 0);
        c.red = HexColor::from_rgb(14, 0, 0);
        c.yellow = HexColor::from_rgb(15, 0, 0);
        c.sky = HexColor::from_rgb(16, 0, 0);
        c.mauve = HexColor::from_rgb(17, 0, 0);
        c
    }

    /// A2: semantic 접근자가 A1 크로스워크대로 primitive 필드에 매핑되는지 고정.
    #[test]
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 반복 assert_eq 테스트 — clippy 과대계상, rca cognitive 0
    fn semantic_accessors_map_to_primitives() {
        let th = Theme::with_colors(distinct_colors(), false);

        // accent (의미색)
        assert_eq!(th.accent_primary(), th.blue);
        assert_eq!(th.accent_info(), th.sky);
        assert_eq!(th.accent_success(), th.green);
        assert_eq!(th.accent_warning(), th.yellow);
        assert_eq!(th.accent_danger(), th.red);
        assert_eq!(th.accent_agent(), th.mauve);

        // 배경 / 표면
        assert_eq!(th.bg_app(), th.crust);
        assert_eq!(th.bg_sidebar(), th.mantle);
        assert_eq!(th.bg_panel(), th.base);
        assert_eq!(th.surface_raised(), th.surface0);
        assert_eq!(th.surface_hover(), th.surface1);
        assert_eq!(th.surface_active(), th.surface2);

        // 텍스트
        assert_eq!(th.text_primary(), th.text);
        assert_eq!(th.text_secondary(), th.subtext1);
        assert_eq!(th.text_muted(), th.subtext0);
        assert_eq!(th.text_disabled(), th.overlay1);
        assert_eq!(th.text_placeholder(), th.placeholder);
        assert_eq!(th.text_on_accent(), th.crust); // dark(Mocha): neutral-0=crust
        // light(Latte): accent 위 텍스트는 절대색 white 로 role-remap.
        let light = Theme::with_colors(distinct_colors(), true);
        assert_eq!(light.text_on_accent(), TEXT_ON_ACCENT_LIGHT);
        assert_ne!(light.text_on_accent(), light.crust);

        // 보더
        assert_eq!(th.border_default(), th.surface0);
        assert_eq!(th.border_strong(), th.surface1);
        assert_eq!(th.border_focus(), th.blue);
        assert_eq!(th.border_attached(), th.lavender); // accent-attached(이름≠토큰)

        // 오버레이 (도출 필드)
        assert_eq!(th.overlay_hover(), th.hover_overlay);
        assert_eq!(th.overlay_active(), th.active_overlay);

        // 다의성: 같은 primitive 로 수렴하는 role 들이 동일값인지 확인
        assert_eq!(th.accent_primary(), th.border_focus()); // 둘 다 blue
        assert_eq!(th.surface_raised(), th.border_default()); // 둘 다 surface0
    }

    /// markdown surface 인라인 표 토큰이 semantic 접근자 포인터로 매핑되고,
    /// 채움 값 사다리(mantle < base < surface0 < surface1)가 유지되는지 고정.
    #[test]
    fn md_table_tokens_map_to_semantics_and_keep_ladder() {
        let th = Theme::with_colors(distinct_colors(), false);

        assert_eq!(th.md_table_border(), th.border_strong()); // surface1
        assert_eq!(th.md_table_header_bg(), th.surface_raised()); // surface0
        assert_eq!(th.md_table_header_fg(), th.text_primary());
        assert_eq!(th.md_table_row_bg(), th.bg_panel()); // base
        assert_eq!(th.md_table_row_bg_zebra(), th.bg_sidebar()); // mantle
        assert_eq!(th.md_table_cell_fg(), th.text_secondary());
        assert_eq!(th.md_table_cell_padding_x(), th.spacing_sm);
        assert_eq!(th.md_table_cell_padding_y(), th.spacing_xs);

        // 값 사다리: zebra(mantle) < 행(base) < 헤더(surface0) < 격자선(surface1).
        assert_eq!(th.md_table_row_bg_zebra(), th.mantle);
        assert_eq!(th.md_table_row_bg(), th.base);
        assert_eq!(th.md_table_header_bg(), th.surface0);
        assert_eq!(th.md_table_border(), th.surface1);
    }

    #[test]
    fn apply_partial_overwrites_only_some_fields() {
        let mut base = dummy_colors();
        let original_red = base.red;
        let partial = PartialColors {
            blue: Some(HexColor::from_rgb(0x00, 0xff, 0x00)),
            ..Default::default()
        };
        base.apply_partial(&partial);
        assert_eq!(base.blue, HexColor::from_rgb(0x00, 0xff, 0x00));
        // red 는 건드리지 않음
        assert_eq!(base.red, original_red);
    }

    #[test]
    fn partial_default_is_empty() {
        let p = PartialColors::default();
        assert!(p.is_empty());
    }

    #[test]
    fn theme_with_colors_propagates_sizing() {
        let t = Theme::with_colors(dummy_colors(), false);
        assert_eq!(t.spacing_sm, SIZING.spacing_sm);
        assert_eq!(t.tab_width, SIZING.tab_width);
        assert!(!t.is_light);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 반복 assert_eq 테스트 — clippy 과대계상, rca cognitive 0
    fn zoom_one_preserves_sizing() {
        let base = Theme::with_colors(dummy_colors(), false);
        let zoomed = Theme::with_colors_and_zoom(dummy_colors(), false, 1.0);
        assert_eq!(base.spacing_xs, zoomed.spacing_xs);
        assert_eq!(base.spacing_sm, zoomed.spacing_sm);
        assert_eq!(base.spacing_md, zoomed.spacing_md);
        assert_eq!(base.spacing_lg, zoomed.spacing_lg);
        assert_eq!(base.spacing_xl, zoomed.spacing_xl);
        assert_eq!(base.font_size_caption, zoomed.font_size_caption);
        assert_eq!(base.font_size_body, zoomed.font_size_body);
        assert_eq!(base.font_size_heading, zoomed.font_size_heading);
        assert_eq!(base.font_size_max, zoomed.font_size_max);
        assert_eq!(base.border_width, zoomed.border_width);
        assert_eq!(base.focus_ring_width, zoomed.focus_ring_width);
        assert_eq!(base.corner_radius, zoomed.corner_radius);
        assert_eq!(base.item_height_tree, zoomed.item_height_tree);
        assert_eq!(base.item_height_interactive, zoomed.item_height_interactive);
        assert_eq!(base.item_height_tab, zoomed.item_height_tab);
        assert_eq!(base.tab_width, zoomed.tab_width);
        assert_eq!(base.sidebar_logo_size, zoomed.sidebar_logo_size);
        assert_eq!(
            base.sidebar_logo_collapsed_size,
            zoomed.sidebar_logo_collapsed_size
        );
        assert_eq!(
            base.sidebar_wordmark_font_size,
            zoomed.sidebar_wordmark_font_size
        );
        assert_eq!(
            base.sidebar_section_heading_font_size,
            zoomed.sidebar_section_heading_font_size
        );
        assert_eq!(
            base.sidebar_button_label_font_size,
            zoomed.sidebar_button_label_font_size
        );
        assert_eq!(
            base.sidebar_collapsed_slot_width,
            zoomed.sidebar_collapsed_slot_width
        );
        assert_eq!(
            base.sidebar_collapsed_icon_height,
            zoomed.sidebar_collapsed_icon_height
        );
        assert_eq!(
            base.sidebar_collapsed_workspace_height,
            zoomed.sidebar_collapsed_workspace_height
        );
        assert_eq!(base.tab_bar_height, zoomed.tab_bar_height);
        assert_eq!(base.tab_bar_label_font_size, zoomed.tab_bar_label_font_size);
        assert_eq!(base.tab_bar_arrow_font_size, zoomed.tab_bar_arrow_font_size);
        assert_eq!(base.titlebar_height, zoomed.titlebar_height);
        assert_eq!(base.traffic_size, zoomed.traffic_size);
        assert_eq!(base.caption_width, zoomed.caption_width);
        assert_eq!(base.window_button_size, zoomed.window_button_size);
    }

    #[test]
    fn ui_zoom_field_stores_value() {
        let default = Theme::with_colors(dummy_colors(), false);
        assert_eq!(default.ui_zoom, 1.0);
        let custom = Theme::with_colors_and_zoom(dummy_colors(), false, 1.5);
        assert_eq!(custom.ui_zoom, 1.5);
    }

    #[test]
    fn sidebar_tokens_scale_with_zoom() {
        // 22 * 1.5 = 33.0, 17 * 1.5 = 25.5 → round → 26.
        let t = Theme::with_colors_and_zoom(dummy_colors(), false, 1.5);
        assert_eq!(t.sidebar_logo_size.value(), 33.0);
        assert_eq!(t.sidebar_wordmark_font_size.value(), 26.0);
        // 32 * 1.5 = 48.0
        assert_eq!(t.sidebar_collapsed_slot_width.value(), 48.0);
    }

    #[test]
    fn zoom_one_point_five_doubles_almost_spacing() {
        // spacing_md=12 → 12 * 1.5 = 18.0 (정수). spacing_sm=8 → 12.
        let t = Theme::with_colors_and_zoom(dummy_colors(), false, 1.5);
        assert_eq!(t.spacing_md.value(), 18.0);
        assert_eq!(t.spacing_sm.value(), 12.0);
        assert_eq!(t.spacing_lg.value(), 24.0);
        // font_size_body=13 → 13 * 1.5 = 19.5 → round → 20.0
        assert_eq!(t.font_size_body.value(), 20.0);
    }

    #[test]
    fn border_width_unaffected_by_zoom() {
        let t_small = Theme::with_colors_and_zoom(dummy_colors(), false, 0.85);
        let t_large = Theme::with_colors_and_zoom(dummy_colors(), false, 1.2);
        assert_eq!(t_small.border_width.value(), 1.0);
        assert_eq!(t_large.border_width.value(), 1.0);
        // tab_width 도 zoom 무관 (탭바 제외 정책).
        assert_eq!(t_small.tab_width, SIZING.tab_width);
        assert_eq!(t_large.tab_width, SIZING.tab_width);
        // tab_bar_* 도 zoom 무관 (탭바 제외 정책).
        assert_eq!(t_small.tab_bar_height, SIZING.tab_bar_height);
        assert_eq!(t_large.tab_bar_height, SIZING.tab_bar_height);
        assert_eq!(
            t_small.tab_bar_label_font_size,
            SIZING.tab_bar_label_font_size
        );
        assert_eq!(
            t_large.tab_bar_label_font_size,
            SIZING.tab_bar_label_font_size
        );
        assert_eq!(
            t_small.tab_bar_arrow_font_size,
            SIZING.tab_bar_arrow_font_size
        );
        assert_eq!(
            t_large.tab_bar_arrow_font_size,
            SIZING.tab_bar_arrow_font_size
        );
        // titlebar(CSD) 토큰도 zoom 무관 (px 고정 정책).
        assert_eq!(t_small.titlebar_height, SIZING.titlebar_height);
        assert_eq!(t_large.titlebar_height, SIZING.titlebar_height);
        assert_eq!(t_small.traffic_size, SIZING.traffic_size);
        assert_eq!(t_large.traffic_size, SIZING.traffic_size);
        assert_eq!(t_small.caption_width, SIZING.caption_width);
        assert_eq!(t_large.caption_width, SIZING.caption_width);
        assert_eq!(t_small.window_button_size, SIZING.window_button_size);
        assert_eq!(t_large.window_button_size, SIZING.window_button_size);
    }

    /// P1/P6: CSD 타이틀바 길이 토큰 값 고정 (디자인 jsx px).
    #[test]
    fn titlebar_sizing_tokens_fixed() {
        let t = Theme::with_colors(dummy_colors(), false);
        assert_eq!(t.titlebar_height.value(), 36.0);
        assert_eq!(t.traffic_size.value(), 12.0);
        assert_eq!(t.caption_width.value(), 46.0);
        assert_eq!(t.window_button_size.value(), 24.0);
    }

    /// P1: titlebar 컴포넌트 색 접근자가 changelog 매핑대로 semantic 에 묶이는지 고정.
    #[test]
    fn titlebar_color_accessors_map_to_semantics() {
        let th = Theme::with_colors(distinct_colors(), false);
        assert_eq!(th.titlebar_bg(), th.bg_app());
        assert_eq!(th.titlebar_bg_inactive(), th.bg_sidebar());
        assert_eq!(th.titlebar_border(), th.separator);
        assert_eq!(th.titlebar_fg(), th.text_secondary());
        assert_eq!(th.titlebar_fg_inactive(), th.text_muted());
    }

    /// P1: OS 리터럴(테마 불변) close red / white 글리프 값 고정.
    #[test]
    fn window_close_literals_are_theme_invariant() {
        let dark = Theme::with_colors(distinct_colors(), false);
        let light = Theme::with_colors(distinct_colors(), true);
        assert_eq!(
            dark.accent_window_close(),
            HexColor::from_rgb(0xc4, 0x2b, 0x1c)
        );
        assert_eq!(dark.accent_window_close(), light.accent_window_close());
        assert_eq!(
            dark.text_on_window_close(),
            HexColor::from_rgb(0xff, 0xff, 0xff)
        );
        assert_eq!(dark.text_on_window_close(), light.text_on_window_close());
    }

    /// token-policy 신설 토큰 값 고정 (semantic.css 의 role 값).
    #[test]
    fn token_policy_new_sizing_values() {
        let t = Theme::with_colors(dummy_colors(), false);
        assert_eq!(t.font_size_micro.value(), 10.0);
        assert_eq!(t.font_size_prose_h1.value(), 20.0);
        assert_eq!(t.line_height_ui, 1.4);
        assert_eq!(t.tooltip_max_width.value(), 240.0);
        assert_eq!(t.font_size_term_sm.value(), 12.0);
        assert_eq!(t.font_size_term.value(), 14.0);
        assert_eq!(t.font_size_term_lg.value(), 16.0);
        assert_eq!(t.icon_glyph_size_xs.value(), 12.0);
        // icon sm/md 정합 확인
        assert_eq!(t.icon_glyph_size_sm.value(), 14.0);
        assert_eq!(t.icon_glyph_size_md.value(), 16.0);
    }

    /// macOS 신호등 색 + disabled opacity 는 테마 불변 OS/정책 리터럴.
    #[test]
    fn macos_traffic_and_disabled_opacity_are_invariant() {
        let dark = Theme::with_colors(distinct_colors(), false);
        let light = Theme::with_colors(distinct_colors(), true);
        assert_eq!(
            dark.accent_macos_close(),
            HexColor::from_rgb(0xec, 0x6a, 0x5e)
        );
        assert_eq!(
            dark.accent_macos_min(),
            HexColor::from_rgb(0xf4, 0xbf, 0x4f)
        );
        assert_eq!(
            dark.accent_macos_zoom(),
            HexColor::from_rgb(0x61, 0xc5, 0x54)
        );
        assert_eq!(dark.accent_macos_close(), light.accent_macos_close());
        assert_eq!(dark.accent_macos_min(), light.accent_macos_min());
        assert_eq!(dark.accent_macos_zoom(), light.accent_macos_zoom());
        assert_eq!(
            dark.brand_melon_flesh(),
            HexColor::from_rgb(0xf2, 0x5d, 0x6b)
        );
        assert_eq!(dark.brand_melon_flesh(), light.brand_melon_flesh());
        assert_eq!(dark.opacity_disabled(), 0.5);
    }

    /// prose / term 폰트는 surface CONTENT 라 UI zoom 미적용 (micro 는 적용).
    #[test]
    fn prose_term_fonts_unaffected_by_zoom() {
        let t = Theme::with_colors_and_zoom(dummy_colors(), false, 1.5);
        assert_eq!(t.font_size_prose_h1, SIZING.font_size_prose_h1);
        assert_eq!(t.line_height_ui, SIZING.line_height_ui);
        assert_eq!(t.font_size_term, SIZING.font_size_term);
        // micro 는 caption 처럼 zoom 적용: 10 * 1.5 = 15.
        assert_eq!(t.font_size_micro.value(), 15.0);
    }

    #[test]
    fn set_is_light_swaps_overlays() {
        let mut t = Theme::with_colors(dummy_colors(), false);
        let dark_hover = t.hover_overlay;
        t.set_is_light(true);
        assert!(t.is_light);
        assert_ne!(t.hover_overlay, dark_hover);
        // 라이트 오버레이는 RGB 가 0
        assert_eq!(t.hover_overlay.r, 0);
    }

    #[test]
    fn extract_apply_round_trip() {
        // dummy 와 두 필드만 다른 변형으로 extract/apply round-trip 검증.
        let mut variant = dummy_colors();
        variant.blue = HexColor::from_rgb(0x00, 0xff, 0x00);
        variant.red = HexColor::from_rgb(0xff, 0x00, 0xff);

        let t = Theme::with_colors(variant.clone(), true);
        let c = t.extract_colors();
        assert_eq!(c, variant);

        let mut t2 = Theme::with_colors(dummy_colors(), false);
        t2.apply_colors(&c);
        // apply_colors 는 with_colors(clone, self.is_light) 이므로 is_light 보존.
        assert!(!t2.is_light);
        assert_eq!(t2.extract_colors(), variant);
    }

    #[test]
    fn apply_partial_merges_surface_themes_entry_wise() {
        let mut base = dummy_colors();
        base.surface_themes
            .insert("terminal".to_string(), FALLBACK_SURFACE.clone());

        let mut partial = PartialColors::default();
        let p_st = PartialSurfaceTheme {
            focused_bg: Some(HexColor::from_rgb(0x11, 0x22, 0x33)),
            ..Default::default()
        };
        partial.surface_themes.insert("terminal".to_string(), p_st);
        // 또 base 에 없는 id 도 partial 만으로 등장 가능 — default 위에 입혀짐
        let p_md = PartialSurfaceTheme {
            focused_fg: Some(HexColor::from_rgb(0xaa, 0xbb, 0xcc)),
            ..Default::default()
        };
        partial.surface_themes.insert("markdown".to_string(), p_md);

        base.apply_partial(&partial);

        let term = base
            .surface_themes
            .get("terminal")
            .expect("terminal exists");
        assert_eq!(term.focused_bg, HexColor::from_rgb(0x11, 0x22, 0x33));
        // partial 이 안 건드린 focused_fg 는 base (FALLBACK_SURFACE) 값
        assert_eq!(term.focused_fg, FALLBACK_SURFACE.focused_fg);

        let md = base
            .surface_themes
            .get("markdown")
            .expect("markdown created");
        assert_eq!(md.focused_fg, HexColor::from_rgb(0xaa, 0xbb, 0xcc));
        // base 에 없던 id 라 default(FALLBACK_SURFACE) 위에 입혔으니 그 외 필드는 fallback
        assert_eq!(md.focused_bg, FALLBACK_SURFACE.focused_bg);
    }

    #[test]
    fn surface_theme_apply_partial_overwrites_only_some_fields() {
        let mut base = FALLBACK_SURFACE.clone();
        let original_fg = base.focused_fg;
        let partial = PartialSurfaceTheme {
            focused_bg: Some(HexColor::from_rgb(0xff, 0x00, 0x00)),
            ..Default::default()
        };
        base.apply_partial(&partial);
        assert_eq!(base.focused_bg, HexColor::from_rgb(0xff, 0x00, 0x00));
        // focused_fg 는 건드리지 않음
        assert_eq!(base.focused_fg, original_fg);
    }

    #[test]
    fn partial_surface_theme_default_is_empty() {
        let p = PartialSurfaceTheme::default();
        assert!(p.is_empty());
    }

    #[test]
    fn fallback_surface_is_dark_friendly() {
        // 모든 surface 가 안전하게 동작하려면 fallback 이 검은 배경 + 가독성 있는 fg 여야 한다.
        assert_eq!(FALLBACK_SURFACE.focused_bg.r, 0);
        assert_eq!(FALLBACK_SURFACE.focused_bg.g, 0);
        assert_eq!(FALLBACK_SURFACE.focused_bg.b, 0);
    }

    // ── design-tokens 04 (F2b): component tier 접근자 zoom 회귀 + 값 불변 ──

    /// zoom 1.0 에서 component 접근자가 tasty-ui-widgets 이식 전 위젯이 쓰던 값과
    /// 정확히 일치함을 대표 속성으로 실측 대조 (갤러리 픽셀 diff 0 의 근거).
    #[test]
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 반복 assert_eq 테스트 — clippy 과대계상, rca cognitive 0
    fn component_accessors_invariant_at_zoom_one() {
        let t = Theme::with_colors(dummy_colors(), false);
        // primitive-직접 종착 — 구 매직넘버/파일 const 값이 zoom 1.0 에서 고정.
        assert_eq!(t.button_height_lg().value(), 32.0); // control.rs CONTROL_HEIGHT_LG
        assert_eq!(t.checkbox_size().value(), 16.0); // toggle.rs BOX
        assert_eq!(t.switch_track_width().value(), 28.0); // toggle.rs SWITCH_W
        assert_eq!(t.switch_track_height().value(), 16.0); // toggle.rs SWITCH_H
        assert_eq!(t.switch_thumb_size().value(), 12.0); // toggle.rs SWITCH_THUMB
        assert_eq!(t.switch_thumb_inset().value(), 2.0); // toggle.rs SWITCH_INSET
        assert_eq!(t.tag_size().value(), 16.0); // chip.rs TAG_HEIGHT
        assert_eq!(t.tag_dot_size().value(), 8.0); // chip.rs TAG_DOT
        assert_eq!(t.kbd_gap().value(), 3.0); // chip.rs KBD_GAP
        assert_eq!(t.kbd_shadow_depth().value(), 2.0); // chip.rs KBD_BOTTOM_BORDER
        assert_eq!(t.select_chevron_room().value(), 28.0); // select.rs CHEVRON_PAD
        // semantic-종착 — 이식 전 위젯이 읽던 바로 그 zoomed 필드와 동일 값.
        assert_eq!(t.button_gap().value(), t.spacing_sm.value()); // button gap
        assert_eq!(t.button_radius().value(), t.corner_radius.value());
        assert_eq!(t.input_height().value(), t.item_height_interactive.value());
        assert_eq!(t.tree_row_height().value(), t.item_height_tree.value()); // =22
        assert_eq!(
            t.menu_item_height().value(),
            t.item_height_interactive.value()
        );
        assert_eq!(t.status_dot_size().value(), 8.0);
        // 색: component 접근자 == 이식 전 semantic 접근자.
        assert_eq!(t.button_primary_bg(), t.accent_primary());
        assert_eq!(t.input_border_focus(), t.border_focus());
        assert_eq!(t.status_dot_success(), t.accent_success());
        assert_eq!(t.tree_row_fg_active(), t.text_primary());
        assert_eq!(t.table_header_fg(), t.text_muted());
    }

    /// zoom≠1.0 에서 이식으로 스케일이 생긴 치수 접근자가 with_colors_and_zoom
    /// resolve(= value*zoom 반올림)를 따라감을 검증 (완료조건 3).
    #[test]
    fn component_dim_accessors_scale_with_zoom() {
        let t = Theme::with_colors_and_zoom(dummy_colors(), false, 1.5);
        // primitive-직접: 구 고정 매직넘버가 이제 zoom 스케일한다.
        assert_eq!(t.button_height_lg().value(), 48.0); // 32 * 1.5
        assert_eq!(t.checkbox_size().value(), 24.0); // 16 * 1.5
        assert_eq!(t.switch_track_width().value(), 42.0); // 28 * 1.5
        assert_eq!(t.tag_size().value(), 24.0); // 16 * 1.5
        assert_eq!(t.kbd_gap().value(), 5.0); // 3 * 1.5 = 4.5 → round 5
        assert_eq!(t.select_chevron_room().value(), 42.0); // 28 * 1.5
        // semantic-종착: 대응 zoomed 필드를 그대로 반영한다.
        assert_eq!(t.button_gap().value(), t.spacing_sm.value()); // 12
        assert_eq!(t.tree_row_height().value(), t.item_height_tree.value()); // 33
        assert_eq!(t.input_height().value(), 42.0); // control-height 28 * 1.5
    }
}
