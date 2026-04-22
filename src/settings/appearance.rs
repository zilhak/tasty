use crate::model::LogicalPx;
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
            focused_bg: HexColor::from_rgb(30, 30, 46),     // #1e1e2e
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(24, 24, 37),    // #181825
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Explorer defaults.
    pub fn explorer_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(30, 30, 46),     // #1e1e2e
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
    pub background_opacity: f32,
    pub sidebar_width: LogicalPx,
    /// UI scale: "small", "medium", or "large". Affects all egui UI elements.
    pub ui_scale: String,
    /// Font scaling mode when moving between monitors with different DPI.
    /// "auto" = font_size * scale_factor (same physical size across monitors)
    /// "fixed" = font_size as-is (more cells on high-DPI, current default)
    pub font_scale_mode: String,
    /// Path to a custom font file (.ttf/.otf) to load.
    /// When set, this font is loaded into the font database in addition to system fonts.
    pub custom_font_path: String,
    /// Line height multiplier relative to font size. Default 1.0 (tight, no gaps).
    /// Values > 1.0 add spacing between lines (e.g. 1.2 = 20% extra).
    pub line_height: f32,
    /// Per-surface-type color overrides.
    pub terminal_colors: SurfaceColors,
    pub markdown_colors: SurfaceColors,
    pub explorer_colors: SurfaceColors,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 14.0,
            theme: "catppuccin-mocha".to_string(),
            background_opacity: 1.0,
            sidebar_width: LogicalPx(180.0),
            ui_scale: "medium".to_string(),
            font_scale_mode: "fixed".to_string(),
            custom_font_path: String::new(),
            line_height: 1.0,
            terminal_colors: SurfaceColors::terminal_default(),
            markdown_colors: SurfaceColors::markdown_default(),
            explorer_colors: SurfaceColors::explorer_default(),
        }
    }
}

impl AppearanceSettings {
    /// Get the UI scale factor based on the ui_scale setting.
    pub fn ui_scale_factor(&self) -> f32 {
        match self.ui_scale.as_str() {
            "small" => 0.85,
            "large" => 1.2,
            _ => 1.0, // medium
        }
    }

    /// Compute the effective font size considering scale_factor and font_scale_mode.
    /// In "auto" mode, font is rasterized at font_size * scale_factor for DPI-aware rendering.
    /// In "fixed" mode, font_size is used as-is regardless of DPI.
    pub fn effective_font_size(&self, scale_factor: f32) -> f32 {
        match self.font_scale_mode.as_str() {
            "auto" => self.font_size * scale_factor,
            _ => self.font_size,
        }
    }

    /// Get the sidebar width adjusted for UI scale.
    pub fn scaled_sidebar_width(&self) -> LogicalPx {
        match self.ui_scale.as_str() {
            "small" => LogicalPx(150.0),
            "large" => LogicalPx(220.0),
            _ => LogicalPx(180.0),
        }
    }
}
