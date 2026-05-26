use serde::{Deserialize, Serialize};
use tasty_core::model::LogicalPx;
use tasty_core::theme::{MOCHA_FALLBACK_COLORS, PartialColors, ThemeApplyContext, ThemeColors};

pub use tasty_core::color::{HexColor, SurfaceColors};

/// Default font settings applied when a surface override field is `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSettings {
    pub font_family: String,
    pub font_size: f32,
    pub custom_font_path: String,
    pub line_height: f32,
    /// "auto" | "fixed"
    pub font_scale_mode: String,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 14.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: "fixed".to_string(),
        }
    }
}

/// Per-surface font override. `None` for any field falls back to `FontSettings`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FontOverride {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub custom_font_path: Option<String>,
    pub line_height: Option<f32>,
    pub font_scale_mode: Option<String>,
}

/// Resolved font values after applying a `FontOverride` to a `FontSettings`.
#[derive(Debug, Clone)]
pub struct EffectiveFont {
    pub font_family: String,
    pub font_size: f32,
    pub custom_font_path: String,
    pub line_height: f32,
    pub font_scale_mode: String,
}

impl EffectiveFont {
    /// Compute font size considering scale_factor and font_scale_mode.
    pub fn effective_font_size(&self, scale_factor: f32) -> f32 {
        match self.font_scale_mode.as_str() {
            "auto" => self.font_size * scale_factor,
            _ => self.font_size,
        }
    }
}

impl FontSettings {
    /// Apply per-field override. `None` fields fall back to defaults.
    pub fn apply_override(&self, ov: &FontOverride) -> EffectiveFont {
        EffectiveFont {
            font_family: ov
                .font_family
                .clone()
                .unwrap_or_else(|| self.font_family.clone()),
            font_size: ov.font_size.unwrap_or(self.font_size),
            custom_font_path: ov
                .custom_font_path
                .clone()
                .unwrap_or_else(|| self.custom_font_path.clone()),
            line_height: ov.line_height.unwrap_or(self.line_height),
            font_scale_mode: ov
                .font_scale_mode
                .clone()
                .unwrap_or_else(|| self.font_scale_mode.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    /// 현재 선택된 테마 id (파일명 stem = `~/.tasty/themes/<id>.toml`).
    pub theme: String,
    /// 누적된 테마 기본값. 테마 변경 시 새 테마의 partial 이 이 위에 덮어쓰여진다.
    /// 첫 실행 시 mocha 풀 세트로 시드.
    pub theme_base: ThemeColors,
    /// 사용자가 픽커로 직접 손댄 흔적. 테마 변경 시 클리어.
    pub theme_overrides: PartialColors,
    /// 현재 라이트/다크 플래그. 테마 파일이 명시하면 그 값으로 갱신.
    pub theme_is_light: bool,
    pub background_opacity: f32,
    pub sidebar_width: LogicalPx,
    /// UI scale: "small", "medium", or "large". Affects all egui UI elements.
    pub ui_scale: String,
    /// Default font settings applied when a surface override is unset.
    pub default_font: FontSettings,
    /// Terminal surface font override (per-field).
    pub terminal_font: FontOverride,
    /// Markdown surface font override (per-field).
    pub markdown_font: FontOverride,
    /// Explorer surface font override (per-field).
    pub explorer_font: FontOverride,
    /// Per-surface-type color overrides.
    pub terminal_colors: SurfaceColors,
    pub markdown_colors: SurfaceColors,
    pub explorer_colors: SurfaceColors,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "mocha".to_string(),
            theme_base: MOCHA_FALLBACK_COLORS,
            theme_overrides: PartialColors::default(),
            theme_is_light: false,
            background_opacity: 1.0,
            sidebar_width: LogicalPx(180.0),
            ui_scale: "medium".to_string(),
            default_font: FontSettings::default(),
            terminal_font: FontOverride::default(),
            markdown_font: FontOverride::default(),
            explorer_font: FontOverride::default(),
            terminal_colors: SurfaceColors::terminal_default(),
            markdown_colors: SurfaceColors::markdown_default(),
            explorer_colors: SurfaceColors::explorer_default(),
        }
    }
}

impl ThemeApplyContext for AppearanceSettings {
    fn theme_id(&self) -> &str {
        &self.theme
    }
    fn set_theme_id(&mut self, id: &str) {
        self.theme = id.to_string();
    }
    fn theme_base(&self) -> &ThemeColors {
        &self.theme_base
    }
    fn theme_base_mut(&mut self) -> &mut ThemeColors {
        &mut self.theme_base
    }
    fn theme_overrides(&self) -> &PartialColors {
        &self.theme_overrides
    }
    fn theme_overrides_mut(&mut self) -> &mut PartialColors {
        &mut self.theme_overrides
    }
    fn theme_is_light(&self) -> bool {
        self.theme_is_light
    }
    fn set_theme_is_light(&mut self, v: bool) {
        self.theme_is_light = v;
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

    /// Get the sidebar width adjusted for UI scale.
    pub fn scaled_sidebar_width(&self) -> LogicalPx {
        match self.ui_scale.as_str() {
            "small" => LogicalPx(150.0),
            "large" => LogicalPx(220.0),
            _ => LogicalPx(180.0),
        }
    }

    /// Effective font for terminal surface.
    pub fn effective_terminal_font(&self) -> EffectiveFont {
        self.default_font.apply_override(&self.terminal_font)
    }

    /// Effective font for markdown surface.
    pub fn effective_markdown_font(&self) -> EffectiveFont {
        self.default_font.apply_override(&self.markdown_font)
    }

    /// Effective font for explorer surface.
    pub fn effective_explorer_font(&self) -> EffectiveFont {
        self.default_font.apply_override(&self.explorer_font)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_override_falls_back_to_defaults() {
        let defaults = FontSettings {
            font_family: "Default".to_string(),
            font_size: 14.0,
            custom_font_path: "/path/default.ttf".to_string(),
            line_height: 1.0,
            font_scale_mode: "fixed".to_string(),
        };
        let ov = FontOverride::default();
        let eff = defaults.apply_override(&ov);
        assert_eq!(eff.font_family, "Default");
        assert_eq!(eff.font_size, 14.0);
        assert_eq!(eff.custom_font_path, "/path/default.ttf");
        assert_eq!(eff.line_height, 1.0);
        assert_eq!(eff.font_scale_mode, "fixed");
    }

    #[test]
    fn apply_override_uses_overrides_when_set() {
        let defaults = FontSettings::default();
        let ov = FontOverride {
            font_family: Some("Override".to_string()),
            font_size: Some(20.0),
            custom_font_path: None,
            line_height: Some(1.5),
            font_scale_mode: Some("auto".to_string()),
        };
        let eff = defaults.apply_override(&ov);
        assert_eq!(eff.font_family, "Override");
        assert_eq!(eff.font_size, 20.0);
        assert_eq!(eff.custom_font_path, "");
        assert_eq!(eff.line_height, 1.5);
        assert_eq!(eff.font_scale_mode, "auto");
    }

    #[test]
    fn effective_font_size_modes() {
        let mut eff = EffectiveFont {
            font_family: String::new(),
            font_size: 16.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: "fixed".to_string(),
        };
        assert_eq!(eff.effective_font_size(2.0), 16.0);
        eff.font_scale_mode = "auto".to_string();
        assert_eq!(eff.effective_font_size(2.0), 32.0);
    }

    #[test]
    fn new_structured_form_deserializes() {
        let toml_str = r#"
[default_font]
font_family = "Cascadia"
font_size = 15.0

[terminal_font]
font_size = 18.0

[markdown_font]
font_family = "Iosevka"
"#;
        let parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.default_font.font_family, "Cascadia");
        assert_eq!(parsed.default_font.font_size, 15.0);
        assert_eq!(parsed.terminal_font.font_size, Some(18.0));
        assert_eq!(parsed.markdown_font.font_family.as_deref(), Some("Iosevka"));
        // Effective values: terminal_font overrides only size, markdown only family.
        let eff_term = parsed.effective_terminal_font();
        assert_eq!(eff_term.font_family, "Cascadia");
        assert_eq!(eff_term.font_size, 18.0);
        let eff_md = parsed.effective_markdown_font();
        assert_eq!(eff_md.font_family, "Iosevka");
        assert_eq!(eff_md.font_size, 15.0);
    }

    #[test]
    fn empty_toml_uses_defaults() {
        let parsed: AppearanceSettings = toml::from_str("").unwrap();
        assert_eq!(parsed.default_font.font_size, 14.0);
        assert!(parsed.terminal_font.font_size.is_none());
    }
}
