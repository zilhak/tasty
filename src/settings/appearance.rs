use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
    pub background_opacity: f32,
    pub sidebar_width: f32,
    /// UI scale: "small", "medium", or "large". Affects all non-terminal UI elements.
    pub ui_scale: String,
    /// Background color for the focused surface (hex, e.g. "#000000").
    /// Unfocused surfaces use the theme's default terminal_bg.
    pub focused_surface_bg: String,
    /// Font scaling mode when moving between monitors with different DPI.
    /// "auto" = font_size * scale_factor (same physical size across monitors)
    /// "fixed" = font_size as-is (more cells on high-DPI, current default)
    pub font_scale_mode: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 14.0,
            theme: "dark".to_string(),
            background_opacity: 1.0,
            sidebar_width: 180.0,
            ui_scale: "medium".to_string(),
            focused_surface_bg: "#000000".to_string(),
            font_scale_mode: "fixed".to_string(),
        }
    }
}

/// Parse a hex color string (#RRGGBB or RRGGBB) to [r, g, b, a] floats.
pub fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

impl AppearanceSettings {
    /// Parse focused_surface_bg hex string to GPU float format [r, g, b, a].
    /// Falls back to #000000 on invalid input.
    pub fn focused_surface_bg_float(&self) -> [f32; 4] {
        parse_hex_color(&self.focused_surface_bg).unwrap_or([0.0, 0.0, 0.0, 1.0])
    }

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
    pub fn scaled_sidebar_width(&self) -> f32 {
        let base = match self.ui_scale.as_str() {
            "small" => 150.0,
            "large" => 220.0,
            _ => 180.0,
        };
        base
    }
}
