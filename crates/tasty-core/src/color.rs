//! Color types shared between theme and settings.
//!
//! `HexColor`는 `#RRGGBB` 문자열로 직렬화되는 색상 래퍼.
//! `SurfaceColors`는 surface 종류별 focused/unfocused 배경·전경 색상 묶음.

use serde::{Deserialize, Serialize};

/// Color wrapper that serializes as hex string ("#RRGGBB") in settings files,
/// but is stored as `egui::Color32` in memory for direct use with color picker.
#[derive(Debug, Clone, Copy)]
pub struct HexColor(pub egui::Color32);

impl HexColor {
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(egui::Color32::from_rgb(r, g, b))
    }

    /// Convert to GPU float format [r, g, b, a].
    pub fn to_float(self) -> [f32; 4] {
        crate::theme::Theme::to_float(self.0)
    }

    /// Convert to hex string "#RRGGBB".
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0.r(), self.0.g(), self.0.b())
    }

    /// Parse from hex string ("#RRGGBB" or "RRGGBB"). Returns None on invalid input.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Self(egui::Color32::from_rgb(r, g, b)))
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
        Self::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid hex color: {s}")))
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
            focused_bg: HexColor::from_rgb(0, 0, 0),       // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),  // #cdd6f4
            unfocused_bg: HexColor::from_rgb(30, 30, 46),   // #1e1e2e
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Markdown defaults.
    pub fn markdown_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),        // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(24, 24, 37),    // #181825
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Explorer defaults.
    pub fn explorer_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),        // #000000
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
