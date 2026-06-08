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
//  ThemeSizing — 모든 테마 공통
// ============================================================================

/// UI 크기/간격. 모든 테마에서 공통. Theme 인스턴스에도 동일 값이 복사된다.
#[derive(Debug, Clone, Copy)]
pub struct ThemeSizing {
    pub font_size_caption: LogicalPx,
    pub font_size_body: LogicalPx,
    pub font_size_heading: LogicalPx,
    pub font_size_max: LogicalPx,
    pub border_width: LogicalPx,
    pub corner_radius: LogicalPx,
    pub item_height_tree: LogicalPx,
    pub item_height_interactive: LogicalPx,
    pub item_height_tab: LogicalPx,
    pub tab_width: LogicalPx,
    pub spacing_xs: LogicalPx,
    pub spacing_sm: LogicalPx,
    pub spacing_md: LogicalPx,
    pub spacing_lg: LogicalPx,
    pub spacing_xl: LogicalPx,
}

pub const SIZING: ThemeSizing = ThemeSizing {
    font_size_caption: LogicalPx(11.0),
    font_size_body: LogicalPx(13.0),
    font_size_heading: LogicalPx(13.0), // semibold 로 구분, 크기는 같음
    font_size_max: LogicalPx(14.0),
    border_width: LogicalPx(1.0),
    corner_radius: LogicalPx(4.0),
    item_height_tree: LogicalPx(22.0),
    item_height_interactive: LogicalPx(28.0),
    item_height_tab: LogicalPx(24.0),
    tab_width: LogicalPx(150.0),
    spacing_xs: LogicalPx(4.0),
    spacing_sm: LogicalPx(8.0),
    spacing_md: LogicalPx(12.0),
    spacing_lg: LogicalPx(16.0),
    spacing_xl: LogicalPx(24.0),
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
    pub font_size_caption: LogicalPx,
    pub font_size_body: LogicalPx,
    pub font_size_heading: LogicalPx,
    pub font_size_max: LogicalPx,
    pub border_width: LogicalPx,
    pub corner_radius: LogicalPx,
    pub item_height_tree: LogicalPx,
    pub item_height_interactive: LogicalPx,
    pub item_height_tab: LogicalPx,
    pub tab_width: LogicalPx,
    pub spacing_xs: LogicalPx,
    pub spacing_sm: LogicalPx,
    pub spacing_md: LogicalPx,
    pub spacing_lg: LogicalPx,
    pub spacing_xl: LogicalPx,

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
        let (hover_overlay, active_overlay, separator) = derive_overlays(is_light);
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
            font_size_caption: SIZING.font_size_caption,
            font_size_body: SIZING.font_size_body,
            font_size_heading: SIZING.font_size_heading,
            font_size_max: SIZING.font_size_max,
            border_width: SIZING.border_width,
            corner_radius: SIZING.corner_radius,
            item_height_tree: SIZING.item_height_tree,
            item_height_interactive: SIZING.item_height_interactive,
            item_height_tab: SIZING.item_height_tab,
            tab_width: SIZING.tab_width,
            spacing_xs: SIZING.spacing_xs,
            spacing_sm: SIZING.spacing_sm,
            spacing_md: SIZING.spacing_md,
            spacing_lg: SIZING.spacing_lg,
            spacing_xl: SIZING.spacing_xl,
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
