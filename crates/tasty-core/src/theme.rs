//! Live theme state — the single source of truth UI code reads from.
//!
//! ```text
//! tasty-themes ──▶ resolve(settings)/mutate ──▶ Theme (here)
//!                                                 │
//!                                                 ▼
//!                                          UI: theme().X
//! ```
//!
//! 이 모듈은 **현재 적용된 값** 의 인스턴스만 관리한다. partial 누적·TOML 로딩·
//! fallback 같은 mutation 로직은 `tasty-themes` 가 담당하고, 결과를
//! `set_theme()` 으로 여기에 박아 넣는다.
//!
//! `Theme` 은 평평한 단일 구조체로, UI 코드가 `theme().crust` /
//! `theme().spacing_sm` / `theme().is_light` 처럼 한 단계로 접근한다.
//! 색상 직렬화·partial 표현은 `ThemeColors` / `PartialColors` 에서 분리.

use crate::color::HexColor;
use crate::model::LogicalPx;
use serde::{Deserialize, Serialize};

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
/// 모든 필드가 `HexColor`. 터미널 fg/bg 나 ansi 도 hex 로 통일했고, GPU 셰이더에
/// 넘길 때만 `.to_float()` 한다.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

    // ── Terminal surface ──
    pub terminal_fg: HexColor,
    pub terminal_bg: HexColor,
    pub selection_bg: HexColor,
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
}

/// `ThemeColors` 의 모든 필드를 `Option<HexColor>` 로 감싼 표현.
///
/// - 사용자가 settings UI 픽커로 손댄 흔적(`AppearanceSettings.theme_overrides`)
/// - 외부 TOML 의 partial 테마 정의 (`ThemeFile` 에서 변환)
///
/// `ThemeColors::apply_partial()` 로 `Some` 필드만 base 에 덮어쓴다.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    pub terminal_fg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_bg: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_bg: Option<HexColor>,
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
        if let Some(v) = p.terminal_fg {
            self.terminal_fg = v;
        }
        if let Some(v) = p.terminal_bg {
            self.terminal_bg = v;
        }
        if let Some(v) = p.selection_bg {
            self.selection_bg = v;
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
    }
}

// ============================================================================
//  Theme — 실제 적용된 인스턴스 (평평한 구조)
// ============================================================================

/// 현재 적용된 테마 인스턴스. **UI 코드는 `theme()` 으로 받아 평평하게 접근**한다
/// (예: `theme().crust`, `theme().spacing_sm`, `theme().is_light`).
///
/// `ThemeColors` 의 모든 필드를 펼쳐 담고, sizing/플래그/도출 색상을 함께 보유.
#[derive(Debug, Clone, Copy)]
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
    pub terminal_fg: HexColor,
    pub terminal_bg: HexColor,
    pub selection_bg: HexColor,
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
    pub const fn with_colors(c: ThemeColors, is_light: bool) -> Self {
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
            terminal_fg: c.terminal_fg,
            terminal_bg: c.terminal_bg,
            selection_bg: c.selection_bg,
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
        }
    }

    /// 현재 색상 스냅샷을 `ThemeColors` 로 추출.
    pub const fn extract_colors(&self) -> ThemeColors {
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
            terminal_fg: self.terminal_fg,
            terminal_bg: self.terminal_bg,
            selection_bg: self.selection_bg,
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
        }
    }

    /// `ThemeColors` 의 색상 필드만 자신에게 덮어쓴다 (sizing / is_light / 도출 색상은 보존).
    /// is_light 변경이 필요하면 `set_is_light()` 도 호출할 것.
    pub fn apply_colors(&mut self, c: &ThemeColors) {
        let next = Self::with_colors(*c, self.is_light);
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

    /// GPU 렌더러용 ANSI 16색 팔레트 (`[r, g, b]` floats in `0..=1`).
    /// 인덱스 순서: black, red, green, yellow, blue, magenta, cyan, white,
    /// bright_black, bright_red, bright_green, bright_yellow, bright_blue,
    /// bright_magenta, bright_cyan, bright_white.
    pub fn ansi_palette(&self) -> [[f32; 3]; 16] {
        let rgb = |c: HexColor| [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0];
        [
            rgb(self.ansi_black),
            rgb(self.ansi_red),
            rgb(self.ansi_green),
            rgb(self.ansi_yellow),
            rgb(self.ansi_blue),
            rgb(self.ansi_magenta),
            rgb(self.ansi_cyan),
            rgb(self.ansi_white),
            rgb(self.ansi_bright_black),
            rgb(self.ansi_bright_red),
            rgb(self.ansi_bright_green),
            rgb(self.ansi_bright_yellow),
            rgb(self.ansi_bright_blue),
            rgb(self.ansi_bright_magenta),
            rgb(self.ansi_bright_cyan),
            rgb(self.ansi_bright_white),
        ]
    }
}

// ============================================================================
//  MOCHA_FALLBACK — Catppuccin Mocha 의 in-memory const fallback
// ============================================================================

/// 최후의 fallback 색상 세트. `tasty-themes` 가 mocha.toml 로드에 실패하면 이걸 쓴다.
pub const MOCHA_FALLBACK_COLORS: ThemeColors = ThemeColors {
    // Surfaces
    crust: HexColor::from_rgb(0x11, 0x11, 0x1b),
    mantle: HexColor::from_rgb(0x18, 0x18, 0x25),
    base: HexColor::from_rgb(0x1e, 0x1e, 0x2e),
    surface0: HexColor::from_rgb(0x31, 0x32, 0x44),
    surface1: HexColor::from_rgb(0x45, 0x47, 0x5a),
    surface2: HexColor::from_rgb(0x58, 0x5b, 0x70),
    // Overlays
    overlay0: HexColor::from_rgb(0x6c, 0x70, 0x86),
    overlay1: HexColor::from_rgb(0x7f, 0x84, 0x9c),
    overlay2: HexColor::from_rgb(0x93, 0x99, 0xb2),
    // Text
    text: HexColor::from_rgb(0xcd, 0xd6, 0xf4),
    subtext1: HexColor::from_rgb(0xba, 0xc2, 0xde),
    subtext0: HexColor::from_rgb(0xa6, 0xad, 0xc8),
    placeholder: HexColor::from_rgb(0x6c, 0x70, 0x86), // = overlay0
    // Accent
    blue: HexColor::from_rgb(0x89, 0xb4, 0xfa),
    green: HexColor::from_rgb(0xa6, 0xe3, 0xa1),
    red: HexColor::from_rgb(0xf3, 0x8b, 0xa8),
    yellow: HexColor::from_rgb(0xf9, 0xe2, 0xaf),
    peach: HexColor::from_rgb(0xfa, 0xb3, 0x87),
    mauve: HexColor::from_rgb(0xcb, 0xa6, 0xf7),
    teal: HexColor::from_rgb(0x94, 0xe2, 0xd5),
    sky: HexColor::from_rgb(0x89, 0xdc, 0xeb),
    lavender: HexColor::from_rgb(0xb4, 0xbe, 0xfe),
    flamingo: HexColor::from_rgb(0xf2, 0xcd, 0xcd),
    pink: HexColor::from_rgb(0xf5, 0xc2, 0xe7),
    maroon: HexColor::from_rgb(0xeb, 0xa0, 0xac),
    rosewater: HexColor::from_rgb(0xf5, 0xe0, 0xdc),
    // Terminal
    terminal_fg: HexColor::from_rgb(0xcd, 0xd6, 0xf4), // = text
    terminal_bg: HexColor::from_rgb(0x1e, 0x1e, 0x2e), // = base
    selection_bg: HexColor::from_rgb(0x58, 0x5b, 0x70), // = surface2
    search_match_bg: HexColor::from_rgba(0xf9, 0xe2, 0xaf, 0x4d), // yellow @ ~30%
    search_match_active_bg: HexColor::from_rgba(0xf9, 0xe2, 0xaf, 0xb3), // yellow @ ~70%
    // ANSI 16
    ansi_black: HexColor::from_rgb(0x45, 0x47, 0x5a), // surface1
    ansi_red: HexColor::from_rgb(0xf3, 0x8b, 0xa8),
    ansi_green: HexColor::from_rgb(0xa6, 0xe3, 0xa1),
    ansi_yellow: HexColor::from_rgb(0xf9, 0xe2, 0xaf),
    ansi_blue: HexColor::from_rgb(0x89, 0xb4, 0xfa),
    ansi_magenta: HexColor::from_rgb(0xcb, 0xa6, 0xf7),
    ansi_cyan: HexColor::from_rgb(0x94, 0xe2, 0xd5),
    ansi_white: HexColor::from_rgb(0xba, 0xc2, 0xde), // subtext1
    ansi_bright_black: HexColor::from_rgb(0x6c, 0x70, 0x86), // overlay0
    ansi_bright_red: HexColor::from_rgb(0xf3, 0x8b, 0xa8),
    ansi_bright_green: HexColor::from_rgb(0xa6, 0xe3, 0xa1),
    ansi_bright_yellow: HexColor::from_rgb(0xf9, 0xe2, 0xaf),
    ansi_bright_blue: HexColor::from_rgb(0x89, 0xb4, 0xfa),
    ansi_bright_magenta: HexColor::from_rgb(0xcb, 0xa6, 0xf7),
    ansi_bright_cyan: HexColor::from_rgb(0x89, 0xdc, 0xeb), // sky
    ansi_bright_white: HexColor::from_rgb(0xcd, 0xd6, 0xf4), // text
};

/// 최후의 fallback Theme 인스턴스. 전역 RwLock 초기값으로도 사용.
pub const MOCHA_FALLBACK: Theme = Theme::with_colors(MOCHA_FALLBACK_COLORS, false);

// ============================================================================
//  전역 인스턴스 (read/write API)
// ============================================================================

/// Global theme instance. Mutable at runtime via `set_theme()`.
static THEME: std::sync::RwLock<Theme> = std::sync::RwLock::new(MOCHA_FALLBACK);

/// Get the current theme (read lock).
pub fn theme() -> std::sync::RwLockReadGuard<'static, Theme> {
    THEME
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the current theme at runtime.
pub fn set_theme(new_theme: Theme) {
    let mut guard = THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = new_theme;
}

/// `tasty-themes` 전용 — 전역 인스턴스를 in-place 로 mutate 한다.
/// 예: `apply_colors` 후 `set_is_light` 같은 두 단계 변경을 락 한 번에 묶을 때.
pub fn mutate_theme(f: impl FnOnce(&mut Theme)) {
    let mut guard = THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard);
}

// ============================================================================
//  ThemeApplyContext — settings ↔ themes 어댑터 인터페이스
// ============================================================================

/// 설정값에서 테마 두 레이어 (`theme_base`, `theme_overrides`) 와 메타데이터
/// (`theme` id, `is_light`) 에 접근하는 trait. `tasty-themes` 의 `apply_theme()`
/// / `resolve()` 가 이 trait 만 받기 때문에, `tasty-settings::AppearanceSettings`
/// 가 직접 구현하면 두 crate 사이의 의존이 필요 없다.
pub trait ThemeApplyContext {
    fn theme_id(&self) -> &str;
    fn set_theme_id(&mut self, id: &str);

    fn theme_base(&self) -> &ThemeColors;
    fn theme_base_mut(&mut self) -> &mut ThemeColors;

    fn theme_overrides(&self) -> &PartialColors;
    fn theme_overrides_mut(&mut self) -> &mut PartialColors;

    fn theme_is_light(&self) -> bool;
    fn set_theme_is_light(&mut self, v: bool);
}

// ============================================================================
//  Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_partial_overwrites_only_some_fields() {
        let mut base = MOCHA_FALLBACK_COLORS;
        let original_red = base.red;
        let mut partial = PartialColors::default();
        partial.blue = Some(HexColor::from_rgb(0x00, 0xff, 0x00));
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
        let t = Theme::with_colors(MOCHA_FALLBACK_COLORS, false);
        assert_eq!(t.spacing_sm, SIZING.spacing_sm);
        assert_eq!(t.tab_width, SIZING.tab_width);
        assert!(!t.is_light);
    }

    #[test]
    fn set_is_light_swaps_overlays() {
        let mut t = Theme::with_colors(MOCHA_FALLBACK_COLORS, false);
        let dark_hover = t.hover_overlay;
        t.set_is_light(true);
        assert!(t.is_light);
        assert_ne!(t.hover_overlay, dark_hover);
        // 라이트 오버레이는 RGB 가 0
        assert_eq!(t.hover_overlay.r, 0);
    }

    #[test]
    fn extract_apply_round_trip() {
        // mocha 와 한 필드만 다른 변형으로 extract/apply round-trip 검증.
        let mut variant = MOCHA_FALLBACK_COLORS;
        variant.blue = HexColor::from_rgb(0x00, 0xff, 0x00);
        variant.red = HexColor::from_rgb(0xff, 0x00, 0xff);

        let t = Theme::with_colors(variant, true);
        let c = t.extract_colors();
        assert_eq!(c, variant);

        let mut t2 = Theme::with_colors(MOCHA_FALLBACK_COLORS, false);
        t2.apply_colors(&c);
        // apply_colors 는 `*self = Self::with_colors(*c, self.is_light)` 이므로 is_light 보존.
        assert!(!t2.is_light);
        assert_eq!(t2.extract_colors(), variant);
    }
}
