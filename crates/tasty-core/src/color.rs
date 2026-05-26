//! Color types shared between theme and settings.
//!
//! `HexColor`는 `#RRGGBB` 문자열로 직렬화되는 색상 래퍼.
//! 내부 표현은 **straight (unmultiplied) RGBA u8**이며, `to_egui()`로 egui가
//! 기대하는 premultiplied `Color32`로 변환된다.
//! `SurfaceColors`는 surface 종류별 focused/unfocused 배경·전경 색상 묶음.

use serde::{Deserialize, Serialize};

/// Straight (unmultiplied) RGBA color stored as u8 components.
///
/// alpha 채널은 0(투명) ~ 255(불투명) 사이의 straight 값이며, GPU/egui 등으로 보낼 때
/// 변환 헬퍼(`to_egui`, `to_float`)를 사용한다. egui는 내부적으로 premultiplied
/// 표현을 쓰므로 `to_egui()`는 `Color32::from_rgba_unmultiplied`를 호출한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HexColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl HexColor {
    /// Opaque RGB.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Straight RGBA (alpha is *not* premultiplied into RGB).
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// 채널 접근자 (egui::Color32 호환을 위한 method-style getter).
    #[inline]
    pub const fn r(self) -> u8 {
        self.r
    }
    #[inline]
    pub const fn g(self) -> u8 {
        self.g
    }
    #[inline]
    pub const fn b(self) -> u8 {
        self.b
    }
    #[inline]
    pub const fn a(self) -> u8 {
        self.a
    }

    /// Replace alpha channel (straight).
    #[inline]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    /// Multiply alpha by `factor` (saturating). RGB는 보존되며, premultiplied로
    /// 변환된 시점의 효과는 egui::Color32::gamma_multiply과 시각적으로 동등하다.
    #[inline]
    pub fn gamma_multiply(self, factor: f32) -> Self {
        let a = ((self.a as f32) * factor).clamp(0.0, 255.0) as u8;
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    /// Convert to GPU-friendly `[r, g, b, a]` floats in `0..=1`.
    /// Components are returned **straight** (not premultiplied).
    pub fn to_float(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    /// Convert to `egui::Color32` (gamma-aware premultiplication via
    /// `Color32::from_rgba_unmultiplied`). 일반적인 변환은 이 메서드를 쓴다.
    ///
    /// alpha < 255인 경우 egui가 sRGB → linear → premultiply → sRGB 순서로
    /// 변환하므로, RGB 채널이 단순히 `r * a / 255`가 아니라 감마 보정된 값으로
    /// 저장된다.
    ///
    /// `egui-compat` 기능이 켜져 있을 때만 노출된다. 헤드리스 플러그인 프로세스는
    /// `default-features = false`로 컴파일하면 이 변환 헬퍼 없이 `HexColor` 자체만
    /// 사용한다.
    #[cfg(feature = "egui-compat")]
    pub fn to_egui(self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(self.r, self.g, self.b, self.a)
    }

    /// Convert to `egui::Color32` treating `(r, g, b, a)` as **already
    /// premultiplied sRGB bytes**.
    ///
    /// 거의 쓸 일이 없지만, egui 0.31의 `from_rgba_premultiplied`와 비트 단위로
    /// 동일한 결과가 필요할 때(예: 과거 시각 결과를 정확히 재현해야 하는 회귀
    /// 케이스) 사용한다.
    #[cfg(feature = "egui-compat")]
    pub fn to_egui_premultiplied(self) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(self.r, self.g, self.b, self.a)
    }

    /// Serialize to `#RRGGBB` (alpha=255) or `#RRGGBBAA` (otherwise).
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Parse `#RGB`, `#RRGGBB`, or `#RRGGBBAA` (leading `#` optional).
    /// 3-digit shorthand expands each nibble (e.g. `#abc` → `#aabbcc`).
    /// 6-digit form is opaque (alpha=255); 8-digit preserves alpha.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
                Some(Self::from_rgb(r * 17, g * 17, b * 17))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self::from_rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Self::from_rgba(r, g, b, a))
            }
            _ => None,
        }
    }
}

#[cfg(feature = "egui-compat")]
impl From<HexColor> for egui::Color32 {
    fn from(c: HexColor) -> Self {
        c.to_egui()
    }
}

impl Serialize for HexColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid hex color: {s}")))
    }
}

/// Per-surface-type color settings for focused / unfocused states.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceColors {
    pub focused_bg: HexColor,
    pub focused_fg: HexColor,
    pub unfocused_bg: HexColor,
    pub unfocused_fg: HexColor,
}

impl SurfaceColors {
    /// Terminal defaults: Catppuccin Mocha base/text.
    pub fn terminal_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),         // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(30, 30, 46),    // #1e1e2e
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Markdown defaults.
    pub fn markdown_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),         // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(24, 24, 37),    // #181825
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Explorer defaults.
    pub fn explorer_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),         // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(24, 24, 37),    // #181825
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }
}

impl Default for SurfaceColors {
    fn default() -> Self {
        Self::terminal_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_hex_round_trip() {
        let c = HexColor::from_rgb(0x12, 0x34, 0x56);
        assert_eq!(c.to_hex(), "#123456");
        assert_eq!(HexColor::from_hex("#123456"), Some(c));
        assert_eq!(HexColor::from_hex("123456"), Some(c));
    }

    #[test]
    fn from_hex_shorthand_3_digit() {
        // #abc → #aabbcc
        assert_eq!(
            HexColor::from_hex("#abc"),
            Some(HexColor::from_rgb(0xaa, 0xbb, 0xcc))
        );
        assert_eq!(
            HexColor::from_hex("f09"),
            Some(HexColor::from_rgb(0xff, 0x00, 0x99))
        );
    }

    #[test]
    fn from_hex_8_digit_preserves_alpha() {
        let c = HexColor::from_hex("#1234567f").unwrap();
        assert_eq!(c, HexColor::from_rgba(0x12, 0x34, 0x56, 0x7f));
        // round-trip
        assert_eq!(c.to_hex(), "#1234567f");
    }

    #[test]
    fn to_hex_drops_alpha_when_opaque() {
        let c = HexColor::from_rgba(0x12, 0x34, 0x56, 0xff);
        assert_eq!(c.to_hex(), "#123456");
    }

    #[test]
    fn from_hex_rejects_bad_lengths() {
        assert_eq!(HexColor::from_hex(""), None);
        assert_eq!(HexColor::from_hex("#12"), None);
        assert_eq!(HexColor::from_hex("#12345"), None);
        assert_eq!(HexColor::from_hex("#1234567"), None);
        assert_eq!(HexColor::from_hex("#123456789"), None);
        assert_eq!(HexColor::from_hex("#zzz"), None);
    }

    #[cfg(feature = "egui-compat")]
    #[test]
    fn straight_alpha_round_trip_via_egui() {
        // hover_overlay (DARK): white at ~8% alpha. egui는 gamma-aware premultiply를
        // 수행하므로 RGB 결과는 단순한 `r*a/255`가 아니다. alpha만 정확히 보존되는지,
        // 그리고 0/255 양극단에서 변형이 없는지만 검증.
        let c = HexColor::from_rgba(255, 255, 255, 20);
        let e = c.to_egui();
        assert_eq!(e.a(), 20);
        // gamma-aware premultiply: 흰색 + 8% alpha → premultiplied sRGB ~79
        assert!(e.r() > 20 && e.r() < 100);

        // hover_overlay (LATTE): black at ~8% alpha. premultiplied도 0이어야 함.
        let c = HexColor::from_rgba(0, 0, 0, 20);
        let e = c.to_egui();
        assert_eq!(e.r(), 0);
        assert_eq!(e.g(), 0);
        assert_eq!(e.b(), 0);
        assert_eq!(e.a(), 20);

        // 완전 불투명/투명은 fast-path가 적용되어 변형이 없어야 한다.
        let opaque = HexColor::from_rgb(0x12, 0x34, 0x56).to_egui();
        assert_eq!(opaque.r(), 0x12);
        assert_eq!(opaque.g(), 0x34);
        assert_eq!(opaque.b(), 0x56);
        assert_eq!(opaque.a(), 255);
    }

    #[cfg(feature = "egui-compat")]
    #[test]
    fn to_egui_premultiplied_bypasses_gamma() {
        // 과거의 `Color32::from_rgba_premultiplied(20, 20, 20, 20)`와 비트 동일.
        let c = HexColor::from_rgba(20, 20, 20, 20);
        let e = c.to_egui_premultiplied();
        assert_eq!(e.r(), 20);
        assert_eq!(e.g(), 20);
        assert_eq!(e.b(), 20);
        assert_eq!(e.a(), 20);
    }

    #[test]
    fn to_float_straight() {
        let c = HexColor::from_rgb(0xff, 0x80, 0x00);
        let f = c.to_float();
        assert!((f[0] - 1.0).abs() < 1e-6);
        assert!((f[1] - 128.0 / 255.0).abs() < 1e-6);
        assert!((f[2] - 0.0).abs() < 1e-6);
        assert!((f[3] - 1.0).abs() < 1e-6);
    }
}
