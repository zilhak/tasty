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

/// macOS 신호등(traffic light) 색. OS 가 인식하는 affordance 라 사용자가 정확한
/// 시스템 red/amber/green 을 기대한다 — Catppuccin accent 가 아니다. Windows close
/// 처럼 테마 불변 OS-system 리터럴 (`--tasty-color-os-macos-*`). mocha/latte 동일값.
pub const OS_MACOS_CLOSE: HexColor = HexColor::from_rgb(0xec, 0x6a, 0x5e);
/// macOS 신호등 — minimize (amber).
pub const OS_MACOS_MIN: HexColor = HexColor::from_rgb(0xf4, 0xbf, 0x4f);
/// macOS 신호등 — zoom (green).
pub const OS_MACOS_ZOOM: HexColor = HexColor::from_rgb(0x61, 0xc5, 0x54);

/// disabled 컨트롤 공통 톤 (`--tasty-opacity-disabled` = 0.5). 모든 위젯이 이 값으로
/// 통일한다. LogicalPx 가 아닌 순수 비율이므로 별도 f32 상수.
pub const OPACITY_DISABLED: f32 = 0.5;

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
    /// markdown surface H1 — 렌더 CONTENT 라 UI 14px 상한 예외 (20px).
    pub font_size_prose_h1: LogicalPx,
    /// markdown surface H2 — 14px (= font_size_max).
    pub font_size_prose_h2: LogicalPx,
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
}

pub const SIZING: ThemeSizing = ThemeSizing {
    font_size_micro: LogicalPx(10.0),
    font_size_caption: LogicalPx(11.0),
    font_size_body: LogicalPx(13.0),
    font_size_heading: LogicalPx(13.0), // semibold 로 구분, 크기는 같음
    font_size_max: LogicalPx(14.0),
    font_size_prose_h1: LogicalPx(20.0),
    font_size_prose_h2: LogicalPx(14.0),
    font_size_term_sm: LogicalPx(12.0),
    font_size_term: LogicalPx(14.0),
    font_size_term_lg: LogicalPx(16.0),
    border_width: LogicalPx(1.0),
    focus_ring_width: LogicalPx(2.0),
    corner_radius: LogicalPx(4.0),
    corner_radius_sm: LogicalPx(2.0),
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
    sidebar_button_label_font_size: LogicalPx(12.0),
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
    /// markdown surface H1 — 렌더 CONTENT 라 UI 14px 상한 예외 (20px).
    pub font_size_prose_h1: LogicalPx,
    /// markdown surface H2 — 14px (= font_size_max).
    pub font_size_prose_h2: LogicalPx,
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
            font_size_prose_h2: SIZING.font_size_prose_h2,
            font_size_term_sm: SIZING.font_size_term_sm,
            font_size_term: SIZING.font_size_term,
            font_size_term_lg: SIZING.font_size_term_lg,
            border_width: SIZING.border_width,
            focus_ring_width: zoomed(SIZING.focus_ring_width),
            corner_radius: zoomed(SIZING.corner_radius),
            corner_radius_sm: zoomed(SIZING.corner_radius_sm),
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
//  Semantic 접근자 (additive) — A1 token-crosswalk 의 의미 라벨 기준
// ============================================================================
//
// 현재 `Theme` 은 평면 primitive 필드만 노출하고, 의미(accent-primary 등) 매핑을
// UI 호출처가 암묵적으로 들고 있다. 아래 접근자는 그 암묵 매핑을 **이름 있는
// semantic 표면**으로 끌어올린다. primitive 필드를 대체하지 않는 additive 레이어 —
// 기존 `theme().blue` 직접 접근은 그대로 유효하며, 호출처 이식이 끝날 때까지 공존한다.
//
// 매핑 근거: `docs/design/systems/token-crosswalk.md` (semantic 96 ↔ primitive ↔ 필드).
// 같은 primitive 가 여러 role 로 갈리는 다의성(crosswalk §4)은 호출처가 어느 접근자를
// 쓰는지로 표현된다 (예: blue → `accent_primary` / `border_focus` / ansi-blue).
impl Theme {
    // ── 배경 (bg-*) ──
    #[inline]
    pub fn bg_app(&self) -> HexColor {
        self.crust
    }
    #[inline]
    pub fn bg_sidebar(&self) -> HexColor {
        self.mantle
    }
    #[inline]
    pub fn bg_panel(&self) -> HexColor {
        self.base
    }

    // ── 표면 (surface-*) ──
    #[inline]
    pub fn surface_raised(&self) -> HexColor {
        self.surface0
    }
    #[inline]
    pub fn surface_hover(&self) -> HexColor {
        self.surface1
    }
    #[inline]
    pub fn surface_active(&self) -> HexColor {
        self.surface2
    }

    // ── 텍스트 (text-*) ──
    #[inline]
    pub fn text_primary(&self) -> HexColor {
        self.text
    }
    #[inline]
    pub fn text_secondary(&self) -> HexColor {
        self.subtext1
    }
    #[inline]
    pub fn text_muted(&self) -> HexColor {
        self.subtext0
    }
    #[inline]
    pub fn text_disabled(&self) -> HexColor {
        self.overlay1
    }
    #[inline]
    pub fn text_placeholder(&self) -> HexColor {
        self.placeholder
    }
    /// semantic role-remap 미대응 — **잠정 매핑**. Mocha 에선 neutral-0(=`crust`) 와
    /// 동일값이지만, Latte 에선 white 여야 한다(DTCG `text-on-accent`). 전용 필드가
    /// 없어 현재는 mocha 기준 `crust` 를 리턴 — 후속에서 role-remap 필드 신설 후보.
    #[inline]
    pub fn text_on_accent(&self) -> HexColor {
        self.crust
    }

    // ── accent (의미색) ──
    #[inline]
    pub fn accent_primary(&self) -> HexColor {
        self.blue
    }
    /// **잠정 매핑** — DTCG `accent-info` → `color-sky`. 현재 실 UI 직접 사용처가
    /// 없다(crosswalk §3.3 "확인 필요"). 매핑 자체는 sky 로 확정.
    #[inline]
    pub fn accent_info(&self) -> HexColor {
        self.sky
    }
    #[inline]
    pub fn accent_success(&self) -> HexColor {
        self.green
    }
    #[inline]
    pub fn accent_warning(&self) -> HexColor {
        self.yellow
    }
    #[inline]
    pub fn accent_danger(&self) -> HexColor {
        self.red
    }
    #[inline]
    pub fn accent_agent(&self) -> HexColor {
        self.mauve
    }

    // ── 보더 (border-*) ──
    #[inline]
    pub fn border_default(&self) -> HexColor {
        self.surface0
    }
    #[inline]
    pub fn border_strong(&self) -> HexColor {
        self.surface1
    }
    #[inline]
    pub fn border_focus(&self) -> HexColor {
        self.blue
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

    /// disabled 컨트롤 공통 opacity (0.5). 모든 위젯이 disabled 디밍에 이 값을 쓴다.
    #[inline]
    pub fn opacity_disabled(&self) -> f32 {
        OPACITY_DISABLED
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
        assert_eq!(th.text_on_accent(), th.crust); // 잠정(mocha 기준)

        // 보더
        assert_eq!(th.border_default(), th.surface0);
        assert_eq!(th.border_strong(), th.surface1);
        assert_eq!(th.border_focus(), th.blue);

        // 오버레이 (도출 필드)
        assert_eq!(th.overlay_hover(), th.hover_overlay);
        assert_eq!(th.overlay_active(), th.active_overlay);

        // 다의성: 같은 primitive 로 수렴하는 role 들이 동일값인지 확인
        assert_eq!(th.accent_primary(), th.border_focus()); // 둘 다 blue
        assert_eq!(th.surface_raised(), th.border_default()); // 둘 다 surface0
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
        assert_eq!(t.font_size_prose_h2.value(), 14.0);
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
        assert_eq!(dark.accent_macos_close(), HexColor::from_rgb(0xec, 0x6a, 0x5e));
        assert_eq!(dark.accent_macos_min(), HexColor::from_rgb(0xf4, 0xbf, 0x4f));
        assert_eq!(dark.accent_macos_zoom(), HexColor::from_rgb(0x61, 0xc5, 0x54));
        assert_eq!(dark.accent_macos_close(), light.accent_macos_close());
        assert_eq!(dark.accent_macos_min(), light.accent_macos_min());
        assert_eq!(dark.accent_macos_zoom(), light.accent_macos_zoom());
        assert_eq!(dark.opacity_disabled(), 0.5);
    }

    /// prose / term 폰트는 surface CONTENT 라 UI zoom 미적용 (micro 는 적용).
    #[test]
    fn prose_term_fonts_unaffected_by_zoom() {
        let t = Theme::with_colors_and_zoom(dummy_colors(), false, 1.5);
        assert_eq!(t.font_size_prose_h1, SIZING.font_size_prose_h1);
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
}
